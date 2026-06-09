use crate::network_topology::detect_default_gateway;
use futures::StreamExt as _;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, IcmpPacket, PingIdentifier, PingSequence, ICMP};
use tokio::sync::Mutex;
use tokio::time::timeout;

/// Ölçüm hedefi: lokal ağ geçişi (gateway) veya WAN (internet).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SegmentStats {
    pub target: String,        // "gateway" | "wan"
    pub target_ip: String,
    pub idle_p50_ms: f64,
    pub idle_p95_ms: f64,
    pub loaded_p95_ms: f64,
    pub delta_ms: f64,
    pub grade: String,
    /// Bidirectional (DL+UL es zamanli) faz: en kotu-durum latency.
    pub bidi_p95_ms: f64,
    pub bidi_delta_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BufferbloatResult {
    pub idle_latency_ms: f64,
    pub loaded_latency_ms: f64,
    pub latency_delta_ms: f64,
    pub grade: String,
    pub grade_description: String,
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub samples: Vec<LatencySample>,
    /// Segment başına detay (gateway = PC-Router, wan = PC-WAN).
    /// Hangi segmentin daha yüksek delta'ya sahip olduğu suçlu tarafı gösterir.
    pub segments: Vec<SegmentStats>,
    /// Teşhis açıklaması — hangi segment problemli, ne yapılmalı.
    pub diagnosis: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LatencySample {
    pub time_ms: u64,
    pub latency_ms: f64,
    pub phase: String, // "idle" | "download" | "upload" | "bidirectional"
}

// ----- Test parametreleri -----
//
// Endüstri standardı bufferbloat metodolojisine yakın bir pencere ile çalışır.
// ICMP ping target Cloudflare 1.1.1.1, satürasyon hedefi de speed.cloudflare.com
// (aynı path → modem'in giden hat'ında oluşan bufferbloat'ı yakalar).

const PING_INTERVAL_MS: u64 = 80; // ~12.5 ping/sn (dusuk-RTT'de daha yogun ornek -> p95 daha guvenilir)
const IDLE_DURATION_SECS: u64 = 5;
const LOAD_DURATION_SECS: u64 = 10;
// Satürasyon Mbps hesabında ilk WARMUP_SECS atlanır: TCP slow-start + H2 window
// ramp-up bu sürede hattı doldurmaz, düşük değerler ortalamayı aşağı çeker.
// Satürasyon ve latency ölçümü warmup sırasında da DEVAM eder; yalnızca Mbps
// hesabı warmup-sonrası pencereden yapılır (speedtest/throttle_baseline ile aynı).
const SAT_WARMUP_SECS: u64 = 4;

// ── Satürasyon kaynağı ──
// Bufferbloat için kritik: akış KESİNTİSİZ olmalı. Bağlantı cap'e takılıp
// yeniden kurulursa o anda throughput düşer → latency "yük yokken" ölçülür →
// bufferbloat olduğundan az görünür. refill_at_bytes cap'in ALTINDA olmalı ki
// worker cap'e varmadan yeni bağlantı açsın (N+1 pipeline).
struct SatSource {
    dl_url: String,
    ul_url: String,
    refill_at_bytes: u64, // DL worker bu kadar byte sonra yeni bağlantıya geçer
    cloudflare: bool,     // CF Referer/Origin header'ları gerekir
    // Latency probe hedefi: satürasyonla AYNI yola gitsin (yük-altı-gecikme mantığı).
    // Satürasyon CF ise 1.1.1.1, netveil ise netveil.net → ölçüm tutarlı.
    ping_host: String,
}

fn sat_source(server: Option<&str>) -> SatSource {
    match server {
        Some("netveil") => SatSource {
            // Dedicated 1Gbps, cap yok → 500MB/conn, %80'de refill (400MB)
            dl_url: "https://netveil.net/dl?bytes=524288000".to_string(),
            ul_url: "https://netveil.net/ul".to_string(),
            refill_at_bytes: 524_288_000 * 80 / 100,
            cloudflare: false,
            ping_host: "netveil.net".to_string(),
        },
        // default: Cloudflare — global CDN, her bölgede yakın PoP, kolay satürasyon.
        // /__down ~95MB cap'li → 50MB'de refill (cap'in çok altında, kesintisiz akış).
        _ => SatSource {
            dl_url: "https://speed.cloudflare.com/__down?bytes=104857600".to_string(), // 100MB iste, cap keser
            ul_url: "https://speed.cloudflare.com/__up".to_string(),
            refill_at_bytes: 52_428_800, // 50MB
            cloudflare: true,
            ping_host: "1.1.1.1".to_string(),
        },
    }
}

/// ping_host'u ICMP icin IPv4 adresine cozer. IP ise direkt parse,
/// hostname ise DNS resolve (netveil.net IP'si degisse de calisir).
/// Cozulemezse 1.1.1.1 fallback (test bos kalmasin).
async fn resolve_ping_host(host: &str) -> IpAddr {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return ip;
    }
    match tokio::net::lookup_host((host, 80)).await {
        Ok(mut addrs) => addrs
            .find_map(|a| match a.ip() {
                IpAddr::V4(v4) => Some(IpAddr::V4(v4)),
                _ => None,
            })
            .unwrap_or(IpAddr::from([1, 1, 1, 1])),
        Err(_) => IpAddr::from([1, 1, 1, 1]),
    }
}

// Paralel bağlantı sayısı — 1Gbps hatları satüre edebilmek için yüksek tutulmalı
const DL_PARALLEL: usize = 16;
const UL_PARALLEL: usize = 8;

#[tauri::command]
pub async fn run_bufferbloat_test(server: Option<String>) -> Result<BufferbloatResult, String> {
    // Sunucu mesgul mu? (IP-kilidi: ayni anda 1 kullanici test edebilir)
    if !crate::throttle_baseline::probe_slot(server.as_deref()).await {
        return Err("SERVER_BUSY".to_string());
    }
    let src = sat_source(server.as_deref());
    crate::dlog!("[bufferbloat] BAŞLADI (kaynak: {})", if src.cloudflare { "cloudflare" } else { "netveil" });
    let samples = Arc::new(Mutex::new(Vec::<LatencySample>::new()));
    let start_time = Instant::now();

    // Latency probe hedefi satürasyonla AYNI yola gitsin: CF→1.1.1.1, netveil→netveil.net.
    // Hostname ise DNS ile çözülür (netveil IP değişse de çalışır).
    let wan_ip: IpAddr = resolve_ping_host(&src.ping_host).await;
    crate::dlog!("[bufferbloat] ping hedefi: {} -> {}", src.ping_host, wan_ip);
    let gateway_ip = detect_default_gateway();
    match gateway_ip {
        Some(_g) => {
            crate::dlog!("[bufferbloat] gateway tespit edildi: {}", _g);
        }
        None => {
            crate::dlog!("[bufferbloat] gateway tespit edilemedi, sadece WAN test edilecek");
        }
    }

    let config = Config::builder().kind(ICMP::V4).build();
    let icmp_client = Client::new(&config).ok();

    // ---- 1. IDLE pencere (5 sn) — iki hedefe aynı anda ping ----
    let idle_results = run_idle_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time).await;

    // ---- 2. DOWNLOAD pencere (10 sn) ----
    let (download_mbps, dl_results) =
        run_loaded_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, true, idle_results.next_seq, &src).await;

    // ---- 3. UPLOAD pencere (10 sn) ----
    let (upload_mbps, ul_results) =
        run_loaded_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, false, dl_results.next_seq, &src).await;

    // ---- 4. BIDIRECTIONAL pencere (10 sn) — DL+UL es zamanli (en kotu-durum) ----
    let bidi_results =
        run_bidirectional_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, ul_results.next_seq, &src).await;

    // ---- Segment başına hesaplama ----
    let wan_segment = compute_segment(
        "wan",
        &wan_ip.to_string(),
        &idle_results.wan_lats,
        &dl_results.wan_lats,
        &ul_results.wan_lats,
        &bidi_results.wan_lats,
    );

    let gateway_segment = gateway_ip.map(|g| {
        compute_segment(
            "gateway",
            &g.to_string(),
            &idle_results.gw_lats,
            &dl_results.gw_lats,
            &ul_results.gw_lats,
            &bidi_results.gw_lats,
        )
    });

    // Teşhis: hangi segment suçlu?
    let diagnosis = diagnose(&wan_segment, gateway_segment.as_ref());

    // Geriye uyumluluk için: ana metrikler WAN'dan gelir (eski UI uyar)
    let (grade, grade_description) = bloat_grade(wan_segment.delta_ms);

    let mut segments = vec![wan_segment.clone()];
    if let Some(gw) = gateway_segment {
        segments.insert(0, gw); // önce gateway, sonra wan
    }

    let all_samples = samples.lock().await.clone();

    crate::dlog!("[bufferbloat] BITTİ. delta={:.1}ms grade={}", wan_segment.delta_ms, grade);
    crate::dlog!("[bufferbloat] teşhis: {}", diagnosis);

    Ok(BufferbloatResult {
        idle_latency_ms: round2(wan_segment.idle_p50_ms),
        loaded_latency_ms: round2(wan_segment.loaded_p95_ms),
        latency_delta_ms: round2(wan_segment.delta_ms),
        grade,
        grade_description,
        download_mbps: round2(download_mbps),
        upload_mbps: round2(upload_mbps),
        samples: all_samples,
        segments,
        diagnosis,
    })
}

struct PhaseLatencies {
    wan_lats: Vec<f64>,
    gw_lats: Vec<f64>,
    next_seq: usize,
}

async fn run_idle_phase(
    client: &Option<Client>,
    wan: IpAddr,
    gw: Option<IpAddr>,
    samples: &Arc<Mutex<Vec<LatencySample>>>,
    start: &Instant,
) -> PhaseLatencies {
    let (wan_lats, gw_lats) = match gw {
        Some(g) => tokio::join!(
            measure_icmp_window(client, wan, IDLE_DURATION_SECS, "idle", samples, start, 0),
            measure_icmp_window(client, g, IDLE_DURATION_SECS, "idle_gw", samples, start, 100_000),
        ),
        None => (
            measure_icmp_window(client, wan, IDLE_DURATION_SECS, "idle", samples, start, 0).await,
            Vec::new(),
        ),
    };
    PhaseLatencies { next_seq: wan_lats.len(), wan_lats, gw_lats }
}

async fn run_loaded_phase(
    client: &Option<Client>,
    wan: IpAddr,
    gw: Option<IpAddr>,
    samples: &Arc<Mutex<Vec<LatencySample>>>,
    start: &Instant,
    is_download: bool,
    seq_offset: usize,
    src: &SatSource,
) -> (f64, PhaseLatencies) {
    let phase_name = if is_download { "download" } else { "upload" };
    let phase_name_gw = if is_download { "download_gw" } else { "upload_gw" };

    // Satürasyon + 2 paralel ping akışı — hepsi aynı pencerede.
    let (mbps, wan_lats, gw_lats) = match gw {
        Some(g) => {
            let (m, w, gw_l) = tokio::join!(
                async {
                    if is_download { saturate_download(LOAD_DURATION_SECS, src).await }
                    else { saturate_upload(LOAD_DURATION_SECS, src).await }
                },
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
                measure_icmp_window(client, g, LOAD_DURATION_SECS, phase_name_gw, samples, start, seq_offset + 100_000),
            );
            (m, w, gw_l)
        }
        None => {
            let (m, w) = tokio::join!(
                async {
                    if is_download { saturate_download(LOAD_DURATION_SECS, src).await }
                    else { saturate_upload(LOAD_DURATION_SECS, src).await }
                },
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
            );
            (m, w, Vec::new())
        }
    };

    let next_seq = seq_offset + wan_lats.len();
    (mbps, PhaseLatencies { wan_lats, gw_lats, next_seq })
}

/// Bidirectional faz: download VE upload es zamanli doyurulur, latency ayni
/// pencerede olculur. Hattin her iki yonu birden doluyken olusan en kotu-durum
/// bufferbloat'i yakalar (çift yönlü / bidirectional mod mantigi).
async fn run_bidirectional_phase(
    client: &Option<Client>,
    wan: IpAddr,
    gw: Option<IpAddr>,
    samples: &Arc<Mutex<Vec<LatencySample>>>,
    start: &Instant,
    seq_offset: usize,
    src: &SatSource,
) -> PhaseLatencies {
    let phase_name = "bidirectional";
    let phase_name_gw = "bidirectional_gw";

    // DL + UL ayni anda satured + ping akislari — hepsi ayni pencerede.
    let (wan_lats, gw_lats) = match gw {
        Some(g) => {
            let (_dl, _ul, w, gw_l) = tokio::join!(
                saturate_download(LOAD_DURATION_SECS, src),
                saturate_upload(LOAD_DURATION_SECS, src),
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
                measure_icmp_window(client, g, LOAD_DURATION_SECS, phase_name_gw, samples, start, seq_offset + 100_000),
            );
            (w, gw_l)
        }
        None => {
            let (_dl, _ul, w) = tokio::join!(
                saturate_download(LOAD_DURATION_SECS, src),
                saturate_upload(LOAD_DURATION_SECS, src),
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
            );
            (w, Vec::new())
        }
    };

    let next_seq = seq_offset + wan_lats.len();
    PhaseLatencies { wan_lats, gw_lats, next_seq }
}

fn compute_segment(
    name: &str,
    target_ip: &str,
    idle: &[f64],
    dl: &[f64],
    ul: &[f64],
    bidi: &[f64],
) -> SegmentStats {
    let idle_p50 = percentile(idle, 50.0);
    let idle_p95 = percentile(idle, 95.0);
    let loaded_p95 = percentile(dl, 95.0).max(percentile(ul, 95.0));
    // Delta = yüklenen latency ile boşta median arası fark.
    // idle_p95 değil idle_p50 kullan — p95 zaten spike içeriyor,
    // loaded_p95'ten büyük çıkınca delta yanlışlıkla sıfırlanıyordu.
    let delta = (loaded_p95 - idle_p50).max(0.0);
    let (grade, _) = bloat_grade(delta);
    // Bidirectional: en kotu-durum (hat her iki yonde birden dolu).
    let bidi_p95 = percentile(bidi, 95.0);
    let bidi_delta = (bidi_p95 - idle_p50).max(0.0);
    SegmentStats {
        target: name.to_string(),
        target_ip: target_ip.to_string(),
        idle_p50_ms: round2(idle_p50),
        idle_p95_ms: round2(idle_p95),
        loaded_p95_ms: round2(loaded_p95),
        delta_ms: round2(delta),
        grade,
        bidi_p95_ms: round2(bidi_p95),
        bidi_delta_ms: round2(bidi_delta),
    }
}

/// Suçlu segmenti tespit et:
///   - WAN delta büyük, gateway delta küçük   → Router-WAN arası (ISP/modem)
///   - WAN delta büyük, gateway delta büyük   → PC-Router arası (Wi-Fi/LAN)
///   - WAN delta küçük, gateway delta küçük  → Sorun yok
///   - Gateway yoksa karar veremiyoruz, sadece WAN raporu
fn diagnose(wan: &SegmentStats, gateway: Option<&SegmentStats>) -> String {
    const THRESHOLD: f64 = 30.0; // 30ms üstü "belirgin bufferbloat"

    // Bidirectional (en kotu-durum) tek-yon delta'dan belirgin yuksekse uyari ekle.
    let bidi_note = if wan.bidi_delta_ms > wan.delta_ms + THRESHOLD {
        format!(
            " ⚡ Under bidirectional load (DL+UL simultaneous) latency rises to Δ{:.0}ms — \
             the link is significantly worse under concurrent load.",
            wan.bidi_delta_ms
        )
    } else {
        String::new()
    };

    let gw = match gateway {
        Some(g) => g,
        None => {
            return format!(
                "Gateway not detected. Only WAN measured: Δ{:.0}ms ({}). \
                 Segment separation not possible.",
                wan.delta_ms, wan.grade
            );
        }
    };

    if wan.delta_ms < THRESHOLD && gw.delta_ms < THRESHOLD {
        return format!(
            "✅ Link is healthy. Gateway Δ{:.0}ms, WAN Δ{:.0}ms — both segments are good.{}",
            gw.delta_ms, wan.delta_ms, bidi_note
        );
    }

    if gw.delta_ms < THRESHOLD && wan.delta_ms >= THRESHOLD {
        return format!(
            "📡 Culprit: Router ↔ WAN segment (ISP/modem). \
             Gateway Δ{:.0}ms (clean), WAN Δ{:.0}ms (problematic). \
             Suggestion: enable SQM/QoS on the router, contact your ISP, update modem firmware.{}",
            gw.delta_ms, wan.delta_ms, bidi_note
        );
    }

    if gw.delta_ms >= THRESHOLD && wan.delta_ms >= THRESHOLD {
        // İkisi de sorunluysa muhtemelen PC-Router arası (gateway zaten sorunluysa WAN da olur)
        return format!(
            "🖥️ Culprit: PC ↔ Router segment (Wi-Fi/Ethernet). \
             Gateway Δ{:.0}ms (problematic), WAN Δ{:.0}ms (problematic). \
             Suggestion: use a wired connection, prefer Wi-Fi 5GHz, stay close to the router, update NIC drivers.{}",
            gw.delta_ms, wan.delta_ms, bidi_note
        );
    }

    // gw ≥ threshold ama wan < threshold — garip durum (gateway pinging işini sevmeyebilir)
    format!(
        "⚠️ Gateway ping is variable (Δ{:.0}ms) but WAN is clean (Δ{:.0}ms). \
         The router may be de-prioritizing ICMP. Results may be misleading.",
        gw.delta_ms, wan.delta_ms
    )
}

/// Belirli süre boyunca ICMP ping atar, gelen latency'leri toplar.
/// `samples`'a anlık olarak da yazar (UI/log için).
async fn measure_icmp_window(
    client: &Option<Client>,
    ip: IpAddr,
    duration_secs: u64,
    phase: &str,
    samples: &Arc<Mutex<Vec<LatencySample>>>,
    start: &Instant,
    seq_offset: usize,
) -> Vec<f64> {
    let mut latencies: Vec<f64> = Vec::new();
    let win_start = Instant::now();
    let deadline = win_start + Duration::from_secs(duration_secs);
    let payload = [0u8; 32];
    let mut seq = seq_offset;
    // Ping SERI atilir (olcum araci kuyruga katki yapmasin -> bufferbloat sinyali bozulmaz).
    // Periyot tabani PING_INTERVAL_MS; dusuk-RTT'de bu taban gecerli (yogun ornek).
    // Yuksek-RTT'de (orn. VPN/yuk altinda 1.1.1.1) periyot dogal olarak RTT'ye yaklasir --
    // bu DOGRU davranis: yuksek RTT zaten olcmek istedigimiz sinyaldir, paralel ping
    // flood'u ile kuyrugu kendimiz doldurmayiz. MissedTickBehavior::Skip -> ping RTT'si
    // tabani astiginda biriken tick'leri patlatmaz (Burst) ve ekstra gecikme eklemez (Delay),
    // bir sonraki hizali tick'e gecer.
    let mut ticker = tokio::time::interval(Duration::from_millis(PING_INTERVAL_MS));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    while Instant::now() < deadline {
        ticker.tick().await;
        let t0 = Instant::now();
        let latency = match client {
            Some(c) => {
                let mut pinger = c.pinger(ip, PingIdentifier(crate::rand_id())).await;
                pinger.timeout(Duration::from_secs(1));
                match timeout(
                    Duration::from_secs(1),
                    pinger.ping(PingSequence((seq % 65535) as u16), &payload),
                )
                .await
                {
                    Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) => {
                        Some(t0.elapsed().as_secs_f64() * 1000.0)
                    }
                    _ => None,
                }
            }
            None => {
                // ICMP izni yok (admin değil) — TCP fallback
                tcp_rtt(ip).await
            }
        };

        if let Some(ms) = latency {
            latencies.push(ms);
            samples.lock().await.push(LatencySample {
                time_ms: start.elapsed().as_millis() as u64,
                latency_ms: round2(ms),
                phase: phase.to_string(),
            });
        }

        seq += 1;
    }

    latencies
}

/// ICMP yoksa TCP 443 handshake RTT'si (yaklaşık).
async fn tcp_rtt(ip: IpAddr) -> Option<f64> {
    use tokio::net::TcpStream;
    let addr = format!("{}:443", ip);
    let t0 = Instant::now();
    match timeout(Duration::from_secs(1), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => Some(t0.elapsed().as_secs_f64() * 1000.0),
        _ => None,
    }
}

/// Belirli süre boyunca seçili kaynaktan paralel indirme yapar (hattı doyurur).
/// Sample sayısı yerine SÜREYİ baz alır → hız ne olursa olsun test aynı süre.
/// Client = throttle_baseline::build_client() (16/64MB H2 window) → 1Gbit doyurulur,
/// yavaş hatta window cap olarak kalır (zarar vermez).
async fn saturate_download(duration_secs: u64, src: &SatSource) -> f64 {
    // refill_at_bytes kaynağa göre (CF cap'in altında, netveil 400MB) → N+1 pipeline
    // kesintisiz akış: worker bağlantı bitmeden/cap'e varmadan yenisini açar.
    let refill_at = src.refill_at_bytes;
    let dl_url = src.dl_url.clone();
    let cf = src.cloudflare;

    let client = match crate::throttle_baseline::build_client() {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    let total_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let stop     = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut handles = Vec::new();
    for _ in 0..DL_PARALLEL {
        let c       = client.clone();
        let counter = total_bytes.clone();
        let stp     = stop.clone();
        let url     = dl_url.clone();
        handles.push(tokio::spawn(async move {
            while !stp.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < deadline {
                let mut req = c.get(&url);
                if cf {
                    // Cloudflare /__down referer/origin bekler
                    req = req.header("Referer", "https://speed.cloudflare.com/")
                             .header("Origin", "https://speed.cloudflare.com");
                }
                // Ilk yanit icin de deadline korumasi (CF gec yanit verirse takilmasin).
                // Timeout = deadline'a kalan sure (pay YOK): faz sonunda saturasyon
                // ping window'unu asmasin -> faz gecislerinde deadzone olusmasin.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.as_millis() < 100 { break; }
                let resp = match tokio::time::timeout(remaining, req.send()).await {
                    Ok(Ok(r)) if r.status().is_success() => r,
                    _ => { tokio::time::sleep(Duration::from_millis(50)).await; continue; }
                };
                let mut stream = resp.bytes_stream();
                let mut conn_bytes: u64 = 0;
                loop {
                    if stp.load(std::sync::atomic::Ordering::Relaxed) || Instant::now() >= deadline {
                        stp.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                    match tokio::time::timeout(Duration::from_millis(500), stream.next()).await {
                        Ok(Some(Ok(chunk))) => {
                            let n = chunk.len() as u64;
                            counter.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
                            conn_bytes += n;
                            // refill eşiğine gelince yeni bağlantı için dış döngüye geç — pipeline
                            if conn_bytes >= refill_at { break; }
                        }
                        _ => break,
                    }
                }
            }

        }));
    }

    finish_saturation(handles, total_bytes, &stop).await
}

/// Workerlar bittikten sonra warmup-aware Mbps hesaplar.
/// SAT_WARMUP_SECS dolunca snapshot alır; Mbps = (son_byte - warmup_byte) /
/// (son_zaman - warmup_zaman). Satürasyon workerları bu süre boyunca çalışmaya
/// devam eder (latency ölçümü etkilenmez); yalnızca throughput penceresi kayar.
async fn finish_saturation(
    handles: Vec<tokio::task::JoinHandle<()>>,
    total_bytes: Arc<std::sync::atomic::AtomicU64>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
) -> f64 {
    use std::sync::atomic::Ordering;
    // Warmup penceresinin sonunda snapshot al (workerlar arka planda devam eder).
    tokio::time::sleep(Duration::from_secs(SAT_WARMUP_SECS)).await;
    let warmup_bytes = total_bytes.load(Ordering::Relaxed);
    let warmup_at = Instant::now();

    for h in handles { let _ = h.await; }
    stop.store(true, Ordering::Relaxed);

    let bytes = total_bytes.load(Ordering::Relaxed).saturating_sub(warmup_bytes);
    let elapsed = warmup_at.elapsed().as_secs_f64();
    if elapsed > 0.01 { (bytes as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 }
}

/// Belirli süre boyunca seçili kaynağa paralel upload yapar (hattı doyurur).
async fn saturate_upload(duration_secs: u64, src: &SatSource) -> f64 {
    let ul_url = src.ul_url.clone();
    let cf = src.cloudflare;

    let client = match crate::throttle_baseline::build_client() {
        Ok(c) => c,
        Err(_) => return 0.0,
    };

    let total_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let deadline    = Instant::now() + Duration::from_secs(duration_secs);
    let stop        = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut handles = Vec::new();
    for _ in 0..UL_PARALLEL {
        let c       = client.clone();
        let counter = total_bytes.clone();
        let stp     = stop.clone();
        let url     = ul_url.clone();
        handles.push(tokio::spawn(async move {
            // Sürekli akan stream — stop gelene kadar 64KB chunk üret
            const UL_CHUNK: usize = 65_536;
            while !stp.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < deadline {
                let c2   = c.clone();
                let cnt2 = counter.clone();
                let stp2 = stp.clone();
                let stream = futures::stream::unfold(0u64, move |sent| {
                    let cnt3 = cnt2.clone();
                    let stp3 = stp2.clone();
                    async move {
                        if stp3.load(std::sync::atomic::Ordering::Relaxed) { return None; }
                        let chunk = vec![0xAAu8; UL_CHUNK];
                        cnt3.fetch_add(UL_CHUNK as u64, std::sync::atomic::Ordering::Relaxed);
                        Some((Ok::<_, std::convert::Infallible>(chunk), sent + UL_CHUNK as u64))
                    }
                });
                let mut req = c2.post(&url)
                    .header("content-type", "application/octet-stream");
                if cf {
                    req = req.header("Referer", "https://speed.cloudflare.com/")
                             .header("Origin", "https://speed.cloudflare.com");
                }
                // KRITIK: send().await'i deadline'a kalan sureyle timeout'la sar.
                // Cloudflare /__up gonderilen veriyi yavas tuketip baglantiyi gec
                // kapatabilir; stop=true olsa bile send() build_client'in 60sn
                // timeout'una kadar asili kalir -> test cok gec biter. Kalan sure
                // + kucuk pay ile keserek bunu onleriz.
                // Timeout = kalan sure (pay YOK): saturasyon faz sonunda ping
                // window'unu asmasin -> faz gecisi deadzone'u olusmasin.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.as_millis() < 100 { break; }
                let send_fut = req.body(reqwest::Body::wrap_stream(stream)).send();
                let _ = tokio::time::timeout(remaining, send_fut).await;
            }
            stp.store(true, std::sync::atomic::Ordering::Relaxed);
        }));
    }

    finish_saturation(handles, total_bytes, &stop).await
}

/// Sıralı bir vektörden percentile değeri al (linear interpolation).
fn percentile(values: &[f64], pct: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted: Vec<f64> = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let rank = (pct / 100.0) * ((sorted.len() - 1) as f64);
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        let frac = rank - lo as f64;
        sorted[lo] * (1.0 - frac) + sorted[hi] * frac
    }
}

fn bloat_grade(delta_ms: f64) -> (String, String) {
    // Endüstri standardı gecikme skalası — p95 delta için ayarlı
    match delta_ms as u64 {
        0..=5 => (
            "A+".to_string(),
            "Excellent — No bufferbloat, the link is ideal for professional gaming".to_string(),
        ),
        6..=30 => (
            "A".to_string(),
            "Very Good — Minimal latency increase, suitable for competitive gaming".to_string(),
        ),
        31..=60 => (
            "B".to_string(),
            "Good — Slight bufferbloat, unnoticeable in most games".to_string(),
        ),
        61..=150 => (
            "C".to_string(),
            "Fair — Noticeable bufferbloat, felt in FPS/MOBA games".to_string(),
        ),
        151..=300 => (
            "D".to_string(),
            "Poor — Serious bufferbloat, you will experience lag spikes".to_string(),
        ),
        _ => (
            "F".to_string(),
            "Very Poor — Link is overloaded, unplayable, SQM required".to_string(),
        ),
    }
}

// rand_id -> crate::rand_id() kullanılıyor

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_bos_dizi_sifir() {
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn percentile_tek_eleman() {
        assert_eq!(percentile(&[42.0], 50.0), 42.0);
        assert_eq!(percentile(&[42.0], 95.0), 42.0);
    }

    #[test]
    fn percentile_medyan_ve_interpolasyon() {
        // [10,20,30] p50 = ortanca = 20
        assert_eq!(percentile(&[10.0, 20.0, 30.0], 50.0), 20.0);
        // p0 = min, p100 = max
        assert_eq!(percentile(&[10.0, 20.0, 30.0], 0.0), 10.0);
        assert_eq!(percentile(&[10.0, 20.0, 30.0], 100.0), 30.0);
        // siralanmamis girdi de dogru (fonksiyon iceride sort eder)
        assert_eq!(percentile(&[30.0, 10.0, 20.0], 50.0), 20.0);
        // [0,10] p50 = lineer interpolasyon = 5
        assert_eq!(percentile(&[0.0, 10.0], 50.0), 5.0);
    }

    #[test]
    fn bloat_grade_sinirlari() {
        assert_eq!(bloat_grade(0.0).0, "A+");
        assert_eq!(bloat_grade(5.0).0, "A+");   // 0..=5 ust sinir
        assert_eq!(bloat_grade(5.9).0, "A+");   // as u64 = 5 -> hala A+
        assert_eq!(bloat_grade(6.0).0, "A");    // 6..=30
        assert_eq!(bloat_grade(30.0).0, "A");
        assert_eq!(bloat_grade(31.0).0, "B");
        assert_eq!(bloat_grade(60.0).0, "B");
        assert_eq!(bloat_grade(61.0).0, "C");
        assert_eq!(bloat_grade(150.0).0, "C");
        assert_eq!(bloat_grade(151.0).0, "D");
        assert_eq!(bloat_grade(300.0).0, "D");
        assert_eq!(bloat_grade(301.0).0, "F");
        assert_eq!(bloat_grade(99999.0).0, "F");
    }

    #[test]
    fn bloat_grade_negatif_panik_yapmaz() {
        // delta normalde >=0 ama negatif gelse bile as u64 panik etmemeli
        let (g, _) = bloat_grade(-1.0);
        assert_eq!(g, "A+"); // negatif f64 as u64 = 0 -> A+
    }
}

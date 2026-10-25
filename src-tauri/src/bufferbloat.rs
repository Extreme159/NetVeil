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

// Gecerli ICMP/TCP yaniti asgari sayilari. Bos/az orneklik percentile 0.0 doner -> delta 0
// -> sahte "A+" olurdu; bu yuzden yetersiz orneklikte not verilmez, hata doner.
// Idle 5sn @80ms ~62 ornek, yuklu 10sn ~125 ornek bekler; esikler yuksek-RTT'ye tolerans verir.
const MIN_IDLE_SAMPLES: usize = 10;
const MIN_LOADED_SAMPLES: usize = 8;

/// Satürasyon sonucu: warmup-sonrasi Mbps + sunucu 429 (mesgul) gordu mu.
struct Saturation {
    mbps: f64,
    busy: bool,
    /// Yalnizca upload: sunucu 2xx onayi dogrulanamadiysa hata kodu (`SERVER_BUSY` veya
    /// `UPLOAD_UNCONFIRMED`). Onaysiz upload byte'lari olcum sayilmaz.
    confirm_err: Option<String>,
}

impl Saturation {
    fn failed() -> Self {
        Saturation { mbps: 0.0, busy: false, confirm_err: None }
    }

    /// Upload onayi dogrulanamadiysa mevcut yuk-uretimi hata yoluna cevirir.
    fn check_confirmed(&self) -> Result<(), String> {
        match &self.confirm_err {
            Some(e) => Err(e.clone()),
            None => Ok(()),
        }
    }
}

/// Faz icin yeterli gecerli latency ornegi var mi? Yoksa stabil hata kodu doner:
/// `BB_SAMPLES:<faz>:<gelen>:<gereken>` (faz: idle|download|upload|bidirectional; UI cevirir).
fn require_samples(lats: &[f64], min: usize, what: &str) -> Result<(), String> {
    if lats.len() >= min {
        Ok(())
    } else {
        Err(format!("BB_SAMPLES:{}:{}:{}", what, lats.len(), min))
    }
}

/// ICMP/TCP probe zaman asimi (ms). Yuklu fazda yanitsiz kalan WAN probe'u "gecikme >= bu
/// deger" gozlemidir (sansurlu); agir bufferbloat'ta yanitlar tam da bu yuzden gelmez.
const PROBE_TIMEOUT_MS: f64 = 1000.0;

/// Yuklu faz WAN gozlemleri: yanit + timeout toplami asgari sayiya ulasmalidir; yoksa
/// probe'lar hic gonderilememistir -> hata. Yanitsiz probe'lar PROBE_TIMEOUT_MS ile
/// sansurlenerek listeye eklenir (yuk altinda hic yanit gelmemesi = F, hata degil).
fn loaded_observations(lats: &[f64], timeouts: usize, min: usize, what: &str) -> Result<Vec<f64>, String> {
    if lats.len() + timeouts < min {
        // Stabil hata kodu: `BB_PROBES:<faz>:<gonderilen/yanitlanan>:<gereken>` (UI cevirir).
        return Err(format!("BB_PROBES:{}:{}:{}", what, lats.len() + timeouts, min));
    }
    let mut v = lats.to_vec();
    v.extend(std::iter::repeat(PROBE_TIMEOUT_MS).take(timeouts));
    Ok(v)
}

/// Tek bir hedef (WAN veya gateway) icin tum fazlarin ornek yeterliligi.
fn validate_samples(idle: &[f64], dl: &[f64], ul: &[f64], bidi: &[f64]) -> Result<(), String> {
    require_samples(idle, MIN_IDLE_SAMPLES, "idle")?;
    require_samples(dl, MIN_LOADED_SAMPLES, "download")?;
    require_samples(ul, MIN_LOADED_SAMPLES, "upload")?;
    require_samples(bidi, MIN_LOADED_SAMPLES, "bidirectional")
}

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
            dl_url: "https://api.netveil.net/dl?bytes=524288000".to_string(),
            ul_url: "https://api.netveil.net/ul".to_string(),
            refill_at_bytes: 524_288_000 * 80 / 100,
            cloudflare: false,
            ping_host: "api.netveil.net".to_string(),
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
const UL_CHUNK: usize = 65_536;
/// Worker basina en fazla istek (reddedilen istekten sonra yeniden deneme dahil).
const UL_MAX_ATTEMPTS: usize = 4;

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
    require_samples(&idle_results.wan_lats, MIN_IDLE_SAMPLES, "idle")?;

    // ---- 2. DOWNLOAD pencere (10 sn) ----
    let (dl_sat, dl_results) =
        run_loaded_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, true, idle_results.next_seq, &src).await;
    // Yuk uretilemediyse (429/HTTP hata/baglanti yok) "yuklu" latency gecerli degildir.
    crate::throttle_baseline::check_phase_result(dl_sat.mbps, dl_sat.busy, "bufferbloat_dl")?;
    let dl_wan = loaded_observations(&dl_results.wan_lats, dl_results.wan_timeouts, MIN_LOADED_SAMPLES, "download")?;
    let download_mbps = dl_sat.mbps;

    // ---- 3. UPLOAD pencere (10 sn) ----
    let (ul_sat, ul_results) =
        run_loaded_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, false, dl_results.next_seq, &src).await;
    // Onaysiz upload byte'lari (sunucu 2xx vermedi) yuk sayilmaz -> mevcut yuk-uretimi hata yolu.
    ul_sat.check_confirmed()?;
    crate::throttle_baseline::check_phase_result(ul_sat.mbps, ul_sat.busy, "bufferbloat_ul")?;
    let ul_wan = loaded_observations(&ul_results.wan_lats, ul_results.wan_timeouts, MIN_LOADED_SAMPLES, "upload")?;
    let upload_mbps = ul_sat.mbps;

    // ---- 4. BIDIRECTIONAL pencere (10 sn) — DL+UL es zamanli (en kotu-durum) ----
    let (bidi_dl_sat, bidi_ul_sat, bidi_results) =
        run_bidirectional_phase(&icmp_client, wan_ip, gateway_ip, &samples, &start_time, ul_results.next_seq, &src).await;
    bidi_ul_sat.check_confirmed()?;
    crate::throttle_baseline::check_phase_result(bidi_dl_sat.mbps, bidi_dl_sat.busy, "bufferbloat_bidi_dl")?;
    crate::throttle_baseline::check_phase_result(bidi_ul_sat.mbps, bidi_ul_sat.busy, "bufferbloat_bidi_ul")?;
    let bidi_wan = loaded_observations(&bidi_results.wan_lats, bidi_results.wan_timeouts, MIN_LOADED_SAMPLES, "bidirectional")?;

    // ---- Segment başına hesaplama ----
    let wan_segment = compute_segment(
        "wan",
        &wan_ip.to_string(),
        &idle_results.wan_lats,
        &dl_wan,
        &ul_wan,
        &bidi_wan,
    );

    // Gateway ICMP'ye cevap vermiyorsa (router ICMP'yi de-prioritize/blokluyor) bos orneklerden
    // "delta 0 / A+" uretilmez: gateway segmenti hic olculmemis sayilir.
    let gateway_segment = gateway_ip
        .filter(|_| validate_samples(&idle_results.gw_lats, &dl_results.gw_lats, &ul_results.gw_lats, &bidi_results.gw_lats).is_ok())
        .map(|g| {
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

/// Tek bir pencerenin sonucu: gecerli yanitlar + yanitsiz kalan (timeout) probe sayisi.
#[derive(Default)]
struct IcmpWindow {
    lats: Vec<f64>,
    timeouts: usize,
}

struct PhaseLatencies {
    wan_lats: Vec<f64>,
    /// WAN probe'larindan yanit gelmeyenler: yuklu fazda "en az PROBE_TIMEOUT_MS" gozlemidir.
    wan_timeouts: usize,
    gw_lats: Vec<f64>,
    next_seq: usize,
}

impl PhaseLatencies {
    fn from_windows(wan: IcmpWindow, gw: IcmpWindow, next_seq: usize) -> Self {
        PhaseLatencies { wan_lats: wan.lats, wan_timeouts: wan.timeouts, gw_lats: gw.lats, next_seq }
    }
}

async fn run_idle_phase(
    client: &Option<Client>,
    wan: IpAddr,
    gw: Option<IpAddr>,
    samples: &Arc<Mutex<Vec<LatencySample>>>,
    start: &Instant,
) -> PhaseLatencies {
    let (wan_w, gw_w) = match gw {
        Some(g) => tokio::join!(
            measure_icmp_window(client, wan, IDLE_DURATION_SECS, "idle", samples, start, 0),
            measure_icmp_window(client, g, IDLE_DURATION_SECS, "idle_gw", samples, start, 100_000),
        ),
        None => (
            measure_icmp_window(client, wan, IDLE_DURATION_SECS, "idle", samples, start, 0).await,
            IcmpWindow::default(),
        ),
    };
    let next_seq = wan_w.lats.len();
    PhaseLatencies::from_windows(wan_w, gw_w, next_seq)
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
) -> (Saturation, PhaseLatencies) {
    let phase_name = if is_download { "download" } else { "upload" };
    let phase_name_gw = if is_download { "download_gw" } else { "upload_gw" };

    // Satürasyon + 2 paralel ping akışı — hepsi aynı pencerede.
    let (mbps, wan_w, gw_w) = match gw {
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
            (m, w, IcmpWindow::default())
        }
    };

    let next_seq = seq_offset + wan_w.lats.len();
    (mbps, PhaseLatencies::from_windows(wan_w, gw_w, next_seq))
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
) -> (Saturation, Saturation, PhaseLatencies) {
    let phase_name = "bidirectional";
    let phase_name_gw = "bidirectional_gw";

    // DL + UL ayni anda satured + ping akislari — hepsi ayni pencerede.
    let (dl_sat, ul_sat, wan_w, gw_w) = match gw {
        Some(g) => {
            let (dl, ul, w, gw_l) = tokio::join!(
                saturate_download(LOAD_DURATION_SECS, src),
                saturate_upload(LOAD_DURATION_SECS, src),
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
                measure_icmp_window(client, g, LOAD_DURATION_SECS, phase_name_gw, samples, start, seq_offset + 100_000),
            );
            (dl, ul, w, gw_l)
        }
        None => {
            let (dl, ul, w) = tokio::join!(
                saturate_download(LOAD_DURATION_SECS, src),
                saturate_upload(LOAD_DURATION_SECS, src),
                measure_icmp_window(client, wan, LOAD_DURATION_SECS, phase_name, samples, start, seq_offset),
            );
            (dl, ul, w, IcmpWindow::default())
        }
    };

    let next_seq = seq_offset + wan_w.lats.len();
    (dl_sat, ul_sat, PhaseLatencies::from_windows(wan_w, gw_w, next_seq))
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
) -> IcmpWindow {
    let mut latencies: Vec<f64> = Vec::new();
    let mut timeouts: usize = 0;
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
        // true = prob gonderildi ama yanit gelmedi (timeout). Gonderim/soket hatasi DEGIL.
        let mut timed_out = false;
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
                    Err(_) | Ok(Err(surge_ping::SurgeError::Timeout { .. })) => {
                        timed_out = true;
                        None
                    }
                    _ => None,
                }
            }
            None => {
                // ICMP izni yok (admin değil) — TCP fallback
                let (l, to) = tcp_rtt(ip).await;
                timed_out = to;
                l
            }
        };
        if timed_out { timeouts += 1; }

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

    IcmpWindow { lats: latencies, timeouts }
}

/// ICMP yoksa TCP 443 handshake RTT'si (yaklaşık). Donus: (rtt, timeout_mu).
async fn tcp_rtt(ip: IpAddr) -> (Option<f64>, bool) {
    use tokio::net::TcpStream;
    let addr = format!("{}:443", ip);
    let t0 = Instant::now();
    match timeout(Duration::from_secs(1), TcpStream::connect(&addr)).await {
        Ok(Ok(_)) => (Some(t0.elapsed().as_secs_f64() * 1000.0), false),
        Ok(Err(_)) => (None, false),
        Err(_) => (None, true),
    }
}

/// Belirli süre boyunca seçili kaynaktan paralel indirme yapar (hattı doyurur).
/// Sample sayısı yerine SÜREYİ baz alır → hız ne olursa olsun test aynı süre.
/// Client = throttle_baseline::build_client() (16/64MB H2 window) → 1Gbit doyurulur,
/// yavaş hatta window cap olarak kalır (zarar vermez).
async fn saturate_download(duration_secs: u64, src: &SatSource) -> Saturation {
    // refill_at_bytes kaynağa göre (CF cap'in altında, netveil 400MB) → N+1 pipeline
    // kesintisiz akış: worker bağlantı bitmeden/cap'e varmadan yenisini açar.
    let refill_at = src.refill_at_bytes;
    let dl_url = src.dl_url.clone();
    let cf = src.cloudflare;

    let client = match crate::throttle_baseline::build_client() {
        Ok(c) => c,
        Err(_) => return Saturation::failed(),
    };

    let total_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let busy     = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let deadline = Instant::now() + Duration::from_secs(duration_secs);
    let stop     = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let mut handles = Vec::new();
    for _ in 0..DL_PARALLEL {
        let c       = client.clone();
        let counter = total_bytes.clone();
        let stp     = stop.clone();
        let url     = dl_url.clone();
        let bsy     = busy.clone();
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
                    other => {
                        // 429 = sunucu IP-kilidi (mesgul): yuk uretilemedi, "yavas hat" degil.
                        if let Ok(Ok(r)) = &other {
                            if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                bsy.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
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

    finish_saturation(
        deadline, handles, total_bytes, &stop, &busy,
        Duration::from_secs(crate::throttle_baseline::JOIN_GRACE_SECS), None,
    ).await
}

/// Workerlar bittikten sonra warmup-aware Mbps hesaplar.
/// SAT_WARMUP_SECS dolunca snapshot alır; Mbps = (son_byte - warmup_byte) /
/// (son_zaman - warmup_zaman). Satürasyon workerları bu süre boyunca çalışmaya
/// devam eder (latency ölçümü etkilenmez); yalnızca throughput penceresi kayar.
async fn finish_saturation(
    deadline: Instant,
    handles: Vec<tokio::task::JoinHandle<()>>,
    total_bytes: Arc<std::sync::atomic::AtomicU64>,
    stop: &Arc<std::sync::atomic::AtomicBool>,
    busy: &Arc<std::sync::atomic::AtomicBool>,
    join_grace: Duration,
    tally: Option<&crate::throttle_baseline::UploadTally>,
) -> Saturation {
    use std::sync::atomic::Ordering;
    // Warmup penceresinin sonunda snapshot al (workerlar arka planda devam eder).
    tokio::time::sleep(Duration::from_secs(SAT_WARMUP_SECS)).await;
    let warmup_bytes = total_bytes.load(Ordering::Relaxed);
    let warmup_at = Instant::now();

    // Workerlar faz penceresi (deadline) boyunca CALISMAYA devam eder (latency ornekleme suruyor).
    // Throughput penceresi deadline'da kapanir; temizlik grace'i ANCAK bundan sonra baslar.
    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    let measure_end = Instant::now();
    // Olculen byte'lar pencere SONUNDA sabitlenir: onay bekleme suresi (join) olcume eklenmez.
    let end_bytes = total_bytes.load(Ordering::Relaxed);
    stop.store(true, Ordering::Relaxed);
    match tally {
        // Upload: kuyruk bosalana kadar (istekler cozulunce erken biter) sinirli bekleme.
        Some(t) => {
            // Bekleme, fazin kendi olculen hizina gore adaptif (yavas uplink -> daha uzun, <=60s).
            let window_secs = SAT_WARMUP_SECS as f64 + measure_end.duration_since(warmup_at).as_secs_f64();
            let span = measure_end.duration_since(warmup_at).as_secs_f64();
            let rate_mbps = if span > 0.01 {
                (end_bytes.saturating_sub(warmup_bytes) as f64 * 8.0) / (span * 1_000_000.0)
            } else { 0.0 };
            let grace = crate::throttle_baseline::upload_confirm_grace(end_bytes, rate_mbps, window_secs);
            crate::dlog!("[bufferbloat] upload confirmation grace: {:.1}s (rate {:.1} Mbps)", grace.as_secs_f64(), rate_mbps);
            crate::throttle_baseline::join_uploads(handles, t, grace).await
        }
        None => crate::throttle_baseline::join_bounded(handles, join_grace).await,
    }

    if tally.is_some() {
        crate::dlog!("[bufferbloat] upload confirmation drain: {:.2}s after window end", measure_end.elapsed().as_secs_f64());
    }
    let bytes = end_bytes.saturating_sub(warmup_bytes);
    let elapsed = measure_end.duration_since(warmup_at).as_secs_f64();
    let mbps = if elapsed > 0.01 { (bytes as f64 * 8.0) / (elapsed * 1_000_000.0) } else { 0.0 };
    // Upload: onemli byte ureten her istek sunucudan 2xx almis olmali (aksi halde olcum gecersiz).
    let confirm_err = tally.and_then(|t| t.verdict().err());
    Saturation { mbps, busy: busy.load(Ordering::Relaxed), confirm_err }
}

/// Belirli süre boyunca seçili kaynağa paralel upload yapar (hattı doyurur).
async fn saturate_upload(duration_secs: u64, src: &SatSource) -> Saturation {
    let ul_url = src.ul_url.clone();
    let cf = src.cloudflare;

    let client = match crate::throttle_baseline::build_client() {
        Ok(c) => c,
        Err(_) => return Saturation::failed(),
    };

    let total_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let busy        = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let deadline    = Instant::now() + Duration::from_secs(duration_secs);
    let stop        = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Istek-basi byte + sunucu onayi (worker * UL_MAX_ATTEMPTS + deneme = slot).
    let tally = Arc::new(crate::throttle_baseline::UploadTally::new(UL_PARALLEL * UL_MAX_ATTEMPTS));

    let mut handles = Vec::new();
    for w in 0..UL_PARALLEL {
        let c       = client.clone();
        let counter = total_bytes.clone();
        let stp     = stop.clone();
        let url     = ul_url.clone();
        let bsy     = busy.clone();
        let tly     = tally.clone();
        handles.push(tokio::spawn(async move {
            // Olcum penceresi (deadline) ile onay penceresi (confirm_by) AYRI: govde deadline'da
            // kendiliginden biter (istek iptal edilmez) -> sunucu {received} ile onaylayabilir.
            // Istek basina ust sinir; gercek (adaptif) bekleme finish_saturation/join_uploads'ta.
            let confirm_by = deadline + Duration::from_secs(crate::throttle_baseline::UPLOAD_CONFIRM_MAX_SECS);
            let mut attempt = 0usize;
            while !stp.load(std::sync::atomic::Ordering::Relaxed) && Instant::now() < deadline {
                if attempt >= UL_MAX_ATTEMPTS { return; }
                let slot = w * UL_MAX_ATTEMPTS + attempt;
                attempt += 1;
                let c2 = c.clone();
                let stream = upload_body_stream(counter.clone(), tly.clone(), slot, stp.clone(), deadline);
                let mut req = c2.post(&url)
                    .header("content-type", "application/octet-stream");
                if cf {
                    req = req.header("Referer", "https://speed.cloudflare.com/")
                             .header("Origin", "https://speed.cloudflare.com");
                }
                // KRITIK: send() asili kalmasin (CF /__up gec kapatabilir; client timeout 60sn):
                // yanit en gec confirm_by'a kadar beklenir (deadline + sinirli onay payi).
                // Yanit gelmezse slot Pending kalir -> onaysiz sayilir.
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.as_millis() < 100 { break; }
                let wait = confirm_by.saturating_duration_since(Instant::now());
                // NetVeil /ul: 2xx + {"received": N >= %95 uretilen} sart; Cloudflare: yalnizca 2xx.
                let verify = !cf;
                let send_fut = async {
                    // Client timeout'u (60s) toplam istegi keser; adaptif onay beklemesine yer ac.
                    let r = req.body(reqwest::Body::wrap_stream(stream))
                        .timeout(Duration::from_secs(duration_secs + crate::throttle_baseline::UPLOAD_CONFIRM_MAX_SECS + 5))
                        .send().await;
                    crate::throttle_baseline::upload_response(r, verify).await
                };
                match tokio::time::timeout(wait, send_fut).await {
                    Ok(Ok((status, body))) => {
                        tly.record_result(slot, Ok((status, body)));
                        if !status.is_success() {
                            // Reddedilen upload (429/4xx/5xx) olcume girmez; yuk uretilemedi.
                            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                bsy.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            let rejected = tly.bytes(slot);
                            let _ = counter.fetch_update(
                                std::sync::atomic::Ordering::Relaxed,
                                std::sync::atomic::Ordering::Relaxed,
                                |v| Some(v.saturating_sub(rejected)),
                            );
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }
                    }
                    Ok(Err(_)) => {
                        // Transport hatasi: byte'lar sayildi ama onaylanmadi (slot onaysiz).
                        tly.record(slot, None);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                    Err(_) => break, // onay payi doldu, yanit yok
                }
            }
            stp.store(true, std::sync::atomic::Ordering::Relaxed);
        }));
    }

    finish_saturation(
        deadline, handles, total_bytes, &stop, &busy,
        Duration::from_secs(crate::throttle_baseline::UPLOAD_CONFIRM_GRACE_SECS), Some(&tally),
    ).await
}

/// Upload istek govdesi: 64KB chunk uretir; `stop` veya olcum penceresinin sonu (`deadline`)
/// gelince akis KENDILIGINDEN biter (istek iptal edilmez) -> sunucu govde sonunu gorup
/// 2xx ile onaylayabilir. Byte'lar hem paylasimli sayaca hem istek slotuna yazilir.
fn upload_body_stream(
    counter: Arc<std::sync::atomic::AtomicU64>,
    tally: Arc<crate::throttle_baseline::UploadTally>,
    slot: usize,
    stop: Arc<std::sync::atomic::AtomicBool>,
    deadline: Instant,
) -> impl futures::Stream<Item = Result<Vec<u8>, std::convert::Infallible>> + Send + 'static {
    futures::stream::unfold((), move |_| {
        let counter = counter.clone();
        let tally = tally.clone();
        let stop = stop.clone();
        async move {
            if stop.load(std::sync::atomic::Ordering::Relaxed) || Instant::now() >= deadline {
                return None;
            }
            let chunk = vec![0xAAu8; UL_CHUNK];
            counter.fetch_add(UL_CHUNK as u64, std::sync::atomic::Ordering::Relaxed);
            tally.add_bytes(slot, UL_CHUNK as u64);
            Some((Ok::<_, std::convert::Infallible>(chunk), ()))
        }
    })
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
    fn bos_orneklik_not_vermez() {
        // percentile(bos)=0 -> delta 0 -> A+ olurdu; dogrulama bunu engellemeli.
        assert!(require_samples(&[], MIN_IDLE_SAMPLES, "idle").is_err());
        assert!(require_samples(&[1.0; 9], MIN_IDLE_SAMPLES, "idle").is_err());
        assert!(require_samples(&[1.0; 10], MIN_IDLE_SAMPLES, "idle").is_ok());
    }

    #[test]
    fn validate_samples_her_faz_icin_yeterli_olmali() {
        let idle = vec![10.0; 20];
        let loaded = vec![40.0; 20];
        assert!(validate_samples(&idle, &loaded, &loaded, &loaded).is_ok());
        // Tek bir faz bos (ornegin gateway ICMP'ye cevap vermiyor) -> gecersiz
        assert!(validate_samples(&idle, &loaded, &[], &loaded).is_err());
        assert!(validate_samples(&[], &loaded, &loaded, &loaded).is_err());
        assert!(validate_samples(&idle, &loaded, &loaded, &[1.0; 3]).is_err());
        let msg = validate_samples(&idle, &[], &loaded, &loaded).unwrap_err();
        assert_eq!(msg, "BB_SAMPLES:download:0:8");
    }

    #[test]
    fn yuk_altinda_tum_probe_timeout_ise_not_f_hata_degil() {
        // Idle iyi (20 ornek ~10ms), yuk uretildi, ama yuklu fazda HICBIR probe yanit almadi
        // (agir bufferbloat): 100 timeout sansurlu 1000ms gozlem -> F, hata degil.
        let idle = vec![10.0; 20];
        let dl = loaded_observations(&[], 100, MIN_LOADED_SAMPLES, "download load").unwrap();
        let ul = loaded_observations(&[], 100, MIN_LOADED_SAMPLES, "upload load").unwrap();
        let bidi = loaded_observations(&[], 100, MIN_LOADED_SAMPLES, "bidirectional load").unwrap();
        assert_eq!(dl.len(), 100);
        let seg = compute_segment("wan", "1.1.1.1", &idle, &dl, &ul, &bidi);
        assert_eq!(seg.grade, "F");
        assert!(seg.delta_ms >= 900.0, "delta {}", seg.delta_ms);
        assert_eq!(bloat_grade(seg.delta_ms).0, "F");
    }

    #[test]
    fn karisik_yanit_ve_timeout_sansurlenir() {
        let v = loaded_observations(&[50.0, 60.0], 8, MIN_LOADED_SAMPLES, "x").unwrap();
        assert_eq!(v.len(), 10);
        assert_eq!(v.iter().filter(|x| **x == PROBE_TIMEOUT_MS).count(), 8);
    }

    #[test]
    fn probe_hic_gonderilemediyse_hata() {
        // Yanit da timeout da yok = probe gonderilemedi -> hata (not verilmez)
        assert!(loaded_observations(&[], 0, MIN_LOADED_SAMPLES, "download load").is_err());
        assert!(loaded_observations(&[10.0], 3, MIN_LOADED_SAMPLES, "download load").is_err());
    }

    #[test]
    fn yuk_uretilemediyse_hata_429_server_busy() {
        use crate::throttle_baseline::check_phase_result;
        assert!(check_phase_result(250.0, false, "x").is_ok());
        assert_eq!(check_phase_result(0.0, true, "x").unwrap_err(), "SERVER_BUSY");
        assert_eq!(check_phase_result(0.0, false, "x").unwrap_err(), "NO_DATA:x");
    }

    #[test]
    fn bloat_grade_negatif_panik_yapmaz() {
        // delta normalde >=0 ama negatif gelse bile as u64 panik etmemeli
        let (g, _) = bloat_grade(-1.0);
        assert_eq!(g, "A+"); // negatif f64 as u64 = 0 -> A+
    }

    // H3: govde deadline'da KENDILIGINDEN biter (istek iptal edilmez), byte'lar slota yazilir.
    #[tokio::test]
    async fn upload_govdesi_deadline_da_biter_ve_slota_sayar() {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        let counter = Arc::new(AtomicU64::new(0));
        let tally = Arc::new(crate::throttle_baseline::UploadTally::new(2));
        let stop = Arc::new(AtomicBool::new(false));
        let deadline = Instant::now() + Duration::from_millis(60);
        let mut s = Box::pin(upload_body_stream(counter.clone(), tally.clone(), 1, stop.clone(), deadline));
        let mut n = 0u64;
        while let Some(Ok(chunk)) = s.next().await {
            assert_eq!(chunk.len(), UL_CHUNK);
            n += 1;
        }
        assert!(Instant::now() >= deadline, "govde deadline'dan once bitti");
        assert!(n > 0);
        assert_eq!(counter.load(Ordering::Relaxed), n * UL_CHUNK as u64);
        assert_eq!(tally.bytes(1), n * UL_CHUNK as u64);
        assert_eq!(tally.bytes(0), 0);
        // stop bayragi da govdeyi bitirir
        let stop2 = Arc::new(AtomicBool::new(true));
        let mut s2 = Box::pin(upload_body_stream(counter, tally, 0, stop2, Instant::now() + Duration::from_secs(30)));
        assert!(s2.next().await.is_none());
    }

    // H3: onaysiz (transport hatasi/yanit yok) onemli upload byte'i olcumu gecersiz kilar.
    #[test]
    fn upload_onaysiz_istek_olcumu_gecersiz_kilar() {
        use reqwest::StatusCode;
        let t = crate::throttle_baseline::UploadTally::new(UL_PARALLEL * UL_MAX_ATTEMPTS);
        for w in 0..UL_PARALLEL {
            t.add_bytes(w * UL_MAX_ATTEMPTS, 1_000_000);
        }
        // Hicbir istek onaylanmadi (yanit yok) -> gecersiz
        assert_eq!(t.verdict().unwrap_err(), crate::throttle_baseline::ERR_UPLOAD_UNCONFIRMED);
        for w in 1..UL_PARALLEL {
            t.record(w * UL_MAX_ATTEMPTS, Some(StatusCode::OK));
        }
        // 1/8 (%12.5) onaysiz -> hala gecersiz
        assert_eq!(t.verdict().unwrap_err(), crate::throttle_baseline::ERR_UPLOAD_UNCONFIRMED);
        t.record(0, Some(StatusCode::OK));
        assert!(t.verdict().is_ok());
    }

    // Canli testler hatti doyurur; birbirleriyle veya baska olcumlerle paralel CALISTIRMAYIN.
    // cargo test --lib bufferbloat::tests::qa_live -- --ignored --nocapture --test-threads=1
    async fn qa_live_full_window(source: &str) {
        let start = Instant::now();
        // Faz sureleri + iki upload onay beklemesi (upload ve bidirectional) icin en kotu durum
        // (UPLOAD_CONFIRM_MAX_SECS) + kurulum payi (DNS/gateway/probe_slot) = 5+30+120+25 = 180s.
        let wall = Duration::from_secs(
            IDLE_DURATION_SECS + 3 * LOAD_DURATION_SECS
                + 2 * crate::throttle_baseline::UPLOAD_CONFIRM_MAX_SECS + 25,
        );
        let result = timeout(
            wall,
            run_bufferbloat_test(Some(source.to_string())),
        ).await;
        let elapsed = start.elapsed();
        let result = result.unwrap_or_else(|_| panic!("{source}: {}s wall deadline exceeded", wall.as_secs()))
            .unwrap_or_else(|e| panic!("{source}: {e}; elapsed={elapsed:?}"));
        println!(
            "QA_BUFFERBLOAT source={source} elapsed_s={:.3} grade={} dl_mbps={} ul_mbps={} idle_ms={} loaded_ms={} delta_ms={} samples={}",
            elapsed.as_secs_f64(), result.grade, result.download_mbps, result.upload_mbps,
            result.idle_latency_ms, result.loaded_latency_ms, result.latency_delta_ms, result.samples.len(),
        );
        assert!(elapsed >= Duration::from_secs(IDLE_DURATION_SECS + 3 * LOAD_DURATION_SECS),
            "{source}: measurement window ended early: {elapsed:?}");
        assert!(matches!(result.grade.as_str(), "A+" | "A" | "B" | "C" | "D" | "F"));
        assert!(result.download_mbps.is_finite() && result.download_mbps > 0.0);
        assert!(result.upload_mbps.is_finite() && result.upload_mbps > 0.0);
        assert!(result.segments.iter().any(|s| s.target == "wan"));
        for phase in ["idle", "download", "upload", "bidirectional"] {
            let samples: Vec<_> = result.samples.iter().filter(|s| s.phase == phase).collect();
            println!("QA_PHASE source={source} phase={phase} replies={} first_ms={:?} last_ms={:?}",
                samples.len(), samples.first().map(|s| s.time_ms), samples.last().map(|s| s.time_ms));
        }
    }

    #[tokio::test]
    #[ignore = "Live full-window NetVeil measurement; saturates the network"]
    async fn qa_live_bufferbloat_netveil() {
        qa_live_full_window("netveil").await;
    }

    #[tokio::test]
    #[ignore = "Live full-window Cloudflare measurement; saturates the network"]
    async fn qa_live_bufferbloat_cloudflare() {
        qa_live_full_window("cloudflare").await;
    }

    #[tokio::test]
    #[ignore = "Live NetVeil same-IP lease check; run serially with other live tests"]
    async fn qa_live_netveil_16_download_leases() {
        let client = crate::throttle_baseline::build_client().unwrap();
        let start = Instant::now();
        // Hold all response bodies until all headers arrive, so leases can overlap.
        let responses = timeout(Duration::from_secs(15), futures::future::join_all(
            (0..DL_PARALLEL).map(|_| client.get("https://api.netveil.net/dl?bytes=65536").send()),
        )).await.expect("16 parallel download headers exceeded deadline");
        for (i, response) in responses.into_iter().enumerate() {
            let response = response.unwrap_or_else(|e| panic!("stream {i}: {e}"));
            assert_eq!(response.status(), reqwest::StatusCode::OK, "stream {i}");
            let body = timeout(Duration::from_secs(5), response.bytes()).await
                .expect("download body deadline").expect("download body");
            assert_eq!(body.len(), 65536, "stream {i}");
        }
        println!("QA_LEASE streams={DL_PARALLEL} status=200 bytes_each=65536 elapsed_s={:.3}",
            start.elapsed().as_secs_f64());
    }
}

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use tauri::Emitter;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, IcmpPacket, PingIdentifier, PingSequence, ICMP};
use tokio::time::timeout;

use std::sync::OnceLock;

const PING_TARGETS_URL: &str = "https://netveil.net/ping-targets";

// Açılışta bir kez çekilir, sonraki tüm ping testleri bunu kullanır.
static PING_TARGETS: OnceLock<Vec<PingTarget>> = OnceLock::new();

// GeForce NOW edge OnceLock — PING_TARGETS'tan ayrı, karışmaz.
static GFN_TARGETS: OnceLock<Vec<PingTarget>> = OnceLock::new();
const GFN_API_URL: &str = "https://prod.cloudmatchbeta.nvidiagrid.net/v2/serverInfo";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ServerPingResult {
    /// "services" | "games" — UI alt-tab filtresi için.
    pub category: String,
    /// Servisin/oyunun adı: CS2, Cloudflare, Google, Meta, Netflix vs.
    pub name: String,
    pub region: String,
    /// Gerçek ping hedefi (ping için kullanılır, UI'da gösterilmez).
    pub host: String,
    /// UI'da gösterilecek adres.
    pub display: String,
    pub ip: String,
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub jitter_ms: f64,
    pub packet_loss: f64,
    pub grade: String,
    pub reachable: bool,
}

/// Ping hedef tanımı — sunucudan çekilen JSON ile eşleşir, aynı zamanda gomülü default.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingTarget {
    pub category: String,
    pub name:     String,
    pub region:   String,
    /// Gerçek ping hedefi — ICMP/TCP'de kullanılır.
    pub host:     String,
    /// UI'da gösterilecek adres.
    pub display:  String,
    /// TCP-only hedefler için port (örn. GFN edge'leri ICMP bloklar).
    /// None → normal ICMP + fallback akışı. Some(port) → doğrudan TCP-only ping.
    #[serde(default)]
    pub tcp_port: Option<u16>,
    /// Services kategorisinde UI grup başlığı (Sosyal Medya, İletişim, vb.). games'te kullanılmaz.
    #[serde(default)]
    pub group: Option<String>,
}

/// Gomülü varsayılan ping listesi — sunucuya ulaşılamazsa bu kullanılır.
const DEFAULT_TARGETS_JSON: &str = include_str!("default_ping_targets.json");

/// Uygulama açılışında çağrılır — sunucudan listeyi çek, OnceLock'a yaz.
/// Bir kez set edildikten sonra tekrar sunucuya bağlanmaz.
pub async fn init_ping_targets() {
    if PING_TARGETS.get().is_some() {
        return; // zaten yüklendi
    }
    let targets = fetch_from_server().await;
    let _ = PING_TARGETS.set(targets); // ilk set kazanır, hata ignore
}

/// Cache'deki listeyi döner. OnceLock henüz dolmadıysa default döner ama YAZMAZ.
/// Böylece init_ping_targets() sonucu her zaman OnceLock'a yazabilir.
/// GFN edge'lerini de ekler (get_gfn_targets() ile).
fn get_ping_targets() -> Vec<PingTarget> {
    let mut targets = if let Some(t) = PING_TARGETS.get() {
        t.clone()
    } else {
        // init henüz bitmedi — default döndür, OnceLock'a yazma
        crate::dlog!("[ping] init henüz bitmedi, geçici default kullanılıyor");
        load_default_targets()
    };
    // GFN edge'lerini category="games" listesine ekle
    targets.extend(get_gfn_targets());
    targets
}

async fn fetch_from_server() -> Vec<PingTarget> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return load_default_targets(),
    };

    match client.get(PING_TARGETS_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<Vec<PingTarget>>().await {
                Ok(targets) if !targets.is_empty() => {
                    crate::dlog!("[ping] sunucu listesi yüklendi: {} hedef", targets.len());
                    targets
                }
                _ => {
                    crate::dlog!("[ping] sunucu JSON parse hatası, default kullanılıyor");
                    load_default_targets()
                }
            }
        }
        _ => {
            crate::dlog!("[ping] sunucuya ulaşılamadı, default kullanılıyor");
            load_default_targets()
        }
    }
}

fn load_default_targets() -> Vec<PingTarget> {
    serde_json::from_str(DEFAULT_TARGETS_JSON).unwrap_or_default()
}

// ─── GeForce NOW edge ─────────────────────────────────────────────────────────

/// Hardcode GFN fallback (NVIDIA'nın işlettiği US/EU/CA edge'leri).
/// Partner bölgeler (tr-/kr-/jp-/in-/au-/sa-) çarıştırılmaz.
fn gfn_hardcode_fallback() -> Vec<PingTarget> {
    let regions: &[(&str, &str)] = &[
        ("us-california-north", "US California North"),
        ("us-california-south", "US California South"),
        ("us-oregon",          "US Oregon"),
        ("us-arizona",         "US Arizona"),
        ("us-texas",           "US Texas"),
        ("us-illinois",        "US Illinois"),
        ("us-florida",         "US Florida"),
        ("us-georgia",         "US Georgia"),
        ("us-virginia",        "US Virginia"),
        ("us-new-jersey",      "US New Jersey"),
        ("ca-quebec",          "CA Quebec"),
        ("eu-united-kingdom-1", "EU United Kingdom 1"),
        ("eu-united-kingdom-2", "EU United Kingdom 2"),
        ("eu-sweden",          "EU Sweden"),
        ("eu-netherlands-north", "EU Netherlands North"),
        ("eu-netherlands-south", "EU Netherlands South"),
        ("eu-germany",         "EU Germany"),
        ("eu-france-1",        "EU France 1"),
        ("eu-france-2",        "EU France 2"),
        ("eu-poland",          "EU Poland"),
        ("eu-bulgaria",        "EU Bulgaria"),
    ];
    regions
        .iter()
        .map(|(region, display)| PingTarget {
            category: "games".to_string(),
            name:     "GeForce NOW".to_string(),
            region:   display.to_string(),
            host:     format!("{}.cloudmatchbeta.nvidiagrid.net", region),
            display:  display.to_string(),
            tcp_port: Some(443),
            group:    None,
        })
        .collect()
}

/// GFN API'den edge listesini çek. Hata/timeout → None (caller fallback kullanır).
async fn fetch_gfn_from_nvidia() -> Option<Vec<PingTarget>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = client.get(GFN_API_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }

    // Yanıt: {"metaData": [{"key":"Germany","value":"https://eu-germany.cloudmatch..."},...] }
    let json: serde_json::Value = resp.json().await.ok()?;
    let meta = json.get("metaData")?.as_array()?;

    let mut targets = Vec::new();
    for item in meta {
        let val = item.get("value")?.as_str()?;
        // https://eu-germany.cloudmatch... → hostname çıkar
        let host = val
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()?;
        // Sadece us-/eu-/ca- prefix'li bölgeler (partner reddet)
        let valid_prefix = host.starts_with("us-")
            || host.starts_with("eu-")
            || host.starts_with("ca-");
        if !valid_prefix {
            continue;
        }
        // display: prefix'den sonraki bölümü okunaklı yap
        let region_part = host
            .split('.')
            .next()
            .unwrap_or(host)
            .replace('-', " ");
        // İlk sözcüğü bölge kısaltmasına dönüştür (us → US, eu → EU, ca → CA)
        let display = {
            let mut parts: Vec<&str> = region_part.splitn(2, ' ').collect();
            if let Some(first) = parts.first_mut() {
                // capitalize prefix
                let upper = first.to_uppercase();
                let rest = parts.get(1).copied().unwrap_or("");
                if rest.is_empty() {
                    upper
                } else {
                    // Titlecase rest
                    let titled: String = rest
                        .split_whitespace()
                        .map(|w| {
                            let mut c = w.chars();
                            match c.next() {
                                None => String::new(),
                                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("{} {}", upper, titled)
                }
            } else {
                region_part
            }
        };
        targets.push(PingTarget {
            category: "games".to_string(),
            name:     "GeForce NOW".to_string(),
            region:   display.clone(),
            host:     host.to_string(),
            display,
            tcp_port: Some(443),
            group:    None,
        });
    }

    if targets.is_empty() {
        None
    } else {
        Some(targets)
    }
}

/// Uygulama açılışında çağrılır — GFN edge listesini API'den çek, OnceLock'a yaz.
/// Hata/timeout → seçme (get_gfn_targets() hardcode fallback döner).
pub async fn init_gfn_targets() {
    if GFN_TARGETS.get().is_some() {
        return;
    }
    if let Some(targets) = fetch_gfn_from_nvidia().await {
        let _ = GFN_TARGETS.set(targets);
        crate::dlog!("[gfn] edge listesi API'den yüklendi");
    } else {
        crate::dlog!("[gfn] API hata, hardcode fallback kullanılacak");
    }
}

/// GFN edge listesini döner. OnceLock dolmadıysa hardcode fallback, YAZMAZ.
fn get_gfn_targets() -> Vec<PingTarget> {
    if let Some(targets) = GFN_TARGETS.get() {
        targets.clone()
    } else {
        crate::dlog!("[gfn] OnceLock boş, hardcode fallback kullanılıyor");
        gfn_hardcode_fallback()
    }
}

// ─── end GeForce NOW ──────────────────────────────────────────────────────────

const WARMUP_HOST: &str = "1.1.1.1";
const PING_CONCURRENCY: usize = 6; // eş zamanlı max ICMP akışı (farklı hedefler — tek noktayı flood etmez)

/// UI'ın skeleton'ını oluşturabilmesi için açılışta çekilen listeyi döner.
#[tauri::command]
pub fn get_ping_targets_list() -> Vec<PingTarget> {
    get_ping_targets().clone()
}

#[tauri::command]
pub async fn ping_game_servers(app: tauri::AppHandle, category: Option<String>) -> Result<Vec<ServerPingResult>, String> {
    // ── Warm-up: 1.1.1.1'e 5 ping — router/firewall ARP+ICMP cache'ini ısıt ────────────
    // Bazı cihazlar ilk pakette yapay gecikme yaratır; warmup bunu engeller.
    ping_warmup().await;

    // Cache'den al, isteğe bağlı kategori filtresi uygula
    let all_targets = get_ping_targets();
    let targets: Vec<PingTarget> = match &category {
        Some(cat) => all_targets.into_iter().filter(|t| &t.category == cat).collect(),
        None      => all_targets,
    };
    let app = std::sync::Arc::new(app);
    let mut results = Vec::with_capacity(targets.len());

    // 3'erli gruplar halinde eş zamanlı ping.
    // Her hedef bittiğinde anında emit edilir — grup bitmesini beklemez.
    // join_all grubun en yaşlısını bekler ama 3 küçük olduğundan bu sorun değil;
    // emit için spawn kullanılır, her task kendi bitiminde yollar.
    use futures::stream::{self, StreamExt};
    let stream_results: Vec<ServerPingResult> = stream::iter(targets)
        .map(|target| {
            let app = app.clone();
            async move {
                let result = ping_server(&target).await;
                let _ = app.emit("ping:result", &result);
                result
            }
        })
        .buffer_unordered(PING_CONCURRENCY)
        .collect()
        .await;

    results.extend(stream_results);
    Ok(results)
}

/// 1.1.1.1'e 5 warm-up ping — sonucu kullanmaz, sadece network stack'i ısıtır.
async fn ping_warmup() {
    use surge_ping::{Client, Config, PingIdentifier, PingSequence, ICMP};
    let ip: std::net::IpAddr = match WARMUP_HOST.parse() {
        Ok(i) => i,
        Err(_) => return,
    };
    let config = Config::builder().kind(ICMP::V4).build();
    let client = match Client::new(&config) {
        Ok(c) => c,
        Err(_) => return,
    };
    let payload = [0u8; 32];
    for seq in 0..5u16 {
        let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
        pinger.timeout(Duration::from_millis(800));
        let _ = tokio::time::timeout(
            Duration::from_millis(800),
            pinger.ping(PingSequence(seq), &payload),
        ).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    crate::dlog!("[ping] warmup tamamlandı");
}

/// TCP-only ping — GFN edge'leri gibi ICMP bloklu hedefler için.
/// Tek port (443), 80 fallback YOK (mevcut tcp_fallback_ping'den farklı).
/// 3 warm-up + 10 ölçüm, her biri yeni TcpStream::connect ile RTT ölçür.
async fn tcp_only_ping(server: &PingTarget, ip: &str, port: u16) -> ServerPingResult {
    use tokio::net::TcpStream;

    const COUNT: usize = 10;
    const WARMUP_COUNT: usize = 3;
    let addr = format!("{}:{}", ip, port);

    // İlk probe: host ulaşılabilir mi?
    let probe = timeout(
        Duration::from_millis(1000),
        TcpStream::connect(&addr),
    ).await;
    let host_reachable = matches!(probe, Ok(Ok(_)));

    if !host_reachable {
        return ServerPingResult {
            category:    server.category.to_string(),
            name:        server.name.to_string(),
            region:      server.region.to_string(),
            host:        server.host.to_string(),
            display:     server.display.to_string(),
            ip:          ip.to_string(),
            avg_ms:      0.0, min_ms: 0.0, max_ms: 0.0, jitter_ms: 0.0,
            packet_loss: 100.0,
            grade:       "N/A".to_string(),
            reachable:   false,
        };
    }

    // Warm-up (sayılmaz)
    for _ in 0..WARMUP_COUNT {
        let _ = timeout(
            Duration::from_millis(800),
            TcpStream::connect(&addr),
        ).await;
        tokio::time::sleep(Duration::from_millis(80)).await;
    }

    // Ölçüm
    let mut latencies = Vec::with_capacity(COUNT);
    let mut lost = 0usize;
    for _ in 0..COUNT {
        let start = Instant::now();
        let result = timeout(
            Duration::from_millis(800),
            TcpStream::connect(&addr),
        ).await;
        match result {
            Ok(Ok(_)) => latencies.push(start.elapsed().as_secs_f64() * 1000.0),
            _ => lost += 1,
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    compute_result(server, ip, latencies, lost, COUNT)
}

async fn ping_server(server: &PingTarget) -> ServerPingResult {
    // tcp_port varsa ICMP'yi atla, doğrudan TCP-only ping yap
    // (GFN edge'leri ICMP bloklar, normal ICMP akışı geçersiz)
    if let Some(port) = server.tcp_port {
        let ip_str = match resolve_host(&server.host).await {
            Some(ip) => ip,
            None => {
                return ServerPingResult {
                    category: server.category.to_string(),
                    name:     server.name.to_string(),
                    region:   server.region.to_string(),
                    host:     server.host.to_string(),
                    display:  server.display.to_string(),
                    ip: "Çözümlenemedi".to_string(),
                    avg_ms: 0.0, min_ms: 0.0, max_ms: 0.0,
                    jitter_ms: 0.0, packet_loss: 100.0,
                    grade: "F".to_string(), reachable: false,
                };
            }
        };
        return tcp_only_ping(server, &ip_str, port).await;
    }

    // Host'u IP'ye \u00e7\u00f6z
    let ip_str = match resolve_host(&server.host).await {
        Some(ip) => ip,
        None => {
            return ServerPingResult {
                category: server.category.to_string(),
                name: server.name.to_string(),
                region: server.region.to_string(),
                host: server.host.to_string(),
                display: server.display.to_string(),
                ip: "Çözümlenemedi".to_string(),
                avg_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
                jitter_ms: 0.0,
                packet_loss: 100.0,
                grade: "F".to_string(),
                reachable: false,
            };
        }
    };

    let ip: IpAddr = match ip_str.parse() {
        Ok(ip) => ip,
        Err(_) => {
            return ServerPingResult {
                category: server.category.to_string(),
                name: server.name.to_string(),
                region: server.region.to_string(),
                host: server.host.to_string(),
                display: server.display.to_string(),
                ip: ip_str,
                avg_ms: 0.0,
                min_ms: 0.0,
                max_ms: 0.0,
                jitter_ms: 0.0,
                packet_loss: 100.0,
                grade: "F".to_string(),
                reachable: false,
            };
        }
    };

    // 10 ping + 3 warm-up = 13 toplam.
    // Her node kendi ICMP client'ını sıfırdan açıyor (surge_ping Client::new).
    // Windows'ta raw socket ilk açılışta kernel scheduling gecikmesi yaratabilir;
    // 2 warmup bunu absorbe etmek için yeterli değil, 3 daha güvenli.
    // WARMUP_COUNT warm-up (sayılmaz) + adaptif ölçüm.
    // Warm-up: Windows raw socket ilk açılış gecikmesini absorbe eder.
    // Adaptif: temel PING_COUNT örnek; jitter (σ) eşiği aşılırsa PING_MAX_COUNT'a
    // kadar ek örnek alınır (RIPE Atlas benzeri — rapor önerisi).
    const PING_COUNT: usize = 10;
    const PING_MAX_COUNT: usize = 20;
    const ADAPTIVE_JITTER_THRESHOLD_MS: f64 = 5.0;
    const WARMUP_COUNT: u16 = 3;
    let mut latencies: Vec<f64> = Vec::new();
    let mut lost = 0usize;
    let mut measured = 0usize;

    let config = Config::builder()
        .kind(match ip {
            IpAddr::V4(_) => ICMP::V4,
            IpAddr::V6(_) => ICMP::V6,
        })
        .build();

    let client = match Client::new(&config) {
        Ok(c) => c,
        Err(_) => {
            // ICMP izin yoksa TCP fallback ile basit ölçüm yap
            return tcp_fallback_ping(server, &ip_str).await;
        }
    };

    let payload = [0u8; 32];
    let mut seq: u16 = 0;

    // ── Warm-up (sonuca dahil edilmez) ──
    for _ in 0..WARMUP_COUNT {
        let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
        pinger.timeout(Duration::from_millis(800));
        let _ = timeout(
            Duration::from_millis(800),
            pinger.ping(PingSequence(seq), &payload),
        )
        .await;
        seq = seq.wrapping_add(1);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ── Ölçüm: temel PING_COUNT, jitter belirsizse PING_MAX_COUNT'a kadar ──
    loop {
        let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
        pinger.timeout(Duration::from_millis(800));
        let start = Instant::now();
        let result = timeout(
            Duration::from_millis(800),
            pinger.ping(PingSequence(seq), &payload),
        )
        .await;
        seq = seq.wrapping_add(1);
        measured += 1;

        match result {
            Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) => {
                latencies.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            _ => lost += 1,
        }

        if measured >= PING_COUNT {
            let unstable = jitter_stddev(&latencies) > ADAPTIVE_JITTER_THRESHOLD_MS;
            if !unstable || measured >= PING_MAX_COUNT {
                break;
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    compute_result(server, &ip_str, latencies, lost, measured)
}

/// Jitter = RTT örneklerinin standart sapması (σ). RFC 3550'nin tercih ettiği,
/// pairwise farktan daha sağlam yöntem. <2 örnek için 0 döner.
fn jitter_stddev(samples: &[f64]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    let variance =
        samples.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / samples.len() as f64;
    variance.sqrt()
}

async fn tcp_fallback_ping(server: &PingTarget, ip: &str) -> ServerPingResult {
    // ICMP engelliyse TCP 80/443 ile basit RTT ölç
    // Önemli: TCP bağlanamama != paket kaybı. Bir host ICMP/TCP'yi firewall'layabilir
    // ama erişilebilir olabilir. Bu yüzden:
    // - İlk bağlantı denemesi başarısızsa host unreachable say, loss=0 dön
    // - İlk bağlantı başarılıysa sonraki kaybı gerçek loss say
    use tokio::net::TcpStream;

    const COUNT: usize = 10;
    const WARMUP_COUNT: usize = 3;
    const TOTAL: usize = COUNT + WARMUP_COUNT;

    // Warmup'ta bağlantı kurup kuramadığımızı tespit et
    let addr443 = format!("{}:443", ip);
    let addr80  = format!("{}:80",  ip);
    let probe = tokio::join!(
        timeout(Duration::from_millis(1000), TcpStream::connect(&addr443)),
        timeout(Duration::from_millis(1000), TcpStream::connect(&addr80)),
    );
    let host_reachable = matches!(probe, (Ok(Ok(_)), _) | (_, Ok(Ok(_))));

    // Host hiç cevap vermiyorsa unreachable dön — loss hesaplama
    if !host_reachable {
        return ServerPingResult {
            category:    server.category.to_string(),
            name:        server.name.to_string(),
            region:      server.region.to_string(),
            host:        server.host.to_string(),
            display:     server.display.to_string(),
            ip:          ip.to_string(),
            avg_ms:      0.0, min_ms: 0.0, max_ms: 0.0, jitter_ms: 0.0,
            packet_loss: 100.0,
            grade:       "N/A".to_string(),
            reachable:   false,
        };
    }

    // Host erişilebilir — şimdi gerçek ölçüm
    let mut latencies = Vec::new();
    let mut lost = 0;

    for i in 0..TOTAL {
        let is_warmup = i < WARMUP_COUNT;
        let start = Instant::now();
        let outcome: Option<f64> = match tokio::join!(
            timeout(Duration::from_millis(800), TcpStream::connect(&addr443)),
            timeout(Duration::from_millis(800), TcpStream::connect(&addr80)),
        ) {
            (Ok(Ok(_)), _) | (_, Ok(Ok(_))) => Some(start.elapsed().as_secs_f64() * 1000.0),
            _ => None,
        };

        if !is_warmup {
            match outcome {
                Some(ms) => latencies.push(ms),
                None => lost += 1,
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    compute_result(server, ip, latencies, lost, COUNT)
}

fn compute_result(
    server: &PingTarget,
    ip: &str,
    latencies: Vec<f64>,
    lost: usize,
    total: usize,
) -> ServerPingResult {
    if latencies.is_empty() {
        return ServerPingResult {
            category: server.category.to_string(),
            name: server.name.to_string(),
            region: server.region.to_string(),
            host: server.host.to_string(),
            display: server.display.to_string(),
            ip: ip.to_string(),
            avg_ms: 0.0,
            min_ms: 0.0,
            max_ms: 0.0,
            jitter_ms: 0.0,
            packet_loss: 100.0,
            grade: "F".to_string(),
            reachable: false,
        };
    }

    let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let min = latencies.iter().cloned().fold(f64::MAX, f64::min);
    let max = latencies.iter().cloned().fold(f64::MIN, f64::max);

    // Jitter = standard deviation (σ) — RFC 3550 EWMA'dan daha kararlı ve
    // tek bir spike'ın ortalamayı bozmasına karşı sağlam.
    //   σ = sqrt( sum( (xi - mean)² ) / N )
    // mean absolute pairwise difference'den (eski formul) daha standart
    // ve oyun/network arşivlerinde kabul gören tanım.
    let jitter = if latencies.len() > 1 {
        let variance: f64 = latencies
            .iter()
            .map(|x| (x - avg).powi(2))
            .sum::<f64>()
            / latencies.len() as f64;
        variance.sqrt()
    } else {
        0.0
    };

    let packet_loss = (lost as f64 / total as f64) * 100.0;
    let grade = ping_grade(avg, jitter, packet_loss);

    ServerPingResult {
        category: server.category.to_string(),
        name: server.name.to_string(),
        region: server.region.to_string(),
        host: server.host.to_string(),
        display: server.display.to_string(),
        ip: ip.to_string(),
        avg_ms: round2(avg),
        min_ms: round2(min),
        max_ms: round2(max),
        jitter_ms: round2(jitter),
        packet_loss: round2(packet_loss),
        grade,
        reachable: true,
    }
}

fn ping_grade(avg_ms: f64, jitter_ms: f64, loss_pct: f64) -> String {
    if loss_pct > 5.0 {
        return "F".to_string();
    }
    if avg_ms < 20.0 && jitter_ms < 3.0 {
        "A+".to_string()
    } else if avg_ms < 40.0 && jitter_ms < 5.0 {
        "A".to_string()
    } else if avg_ms < 60.0 && jitter_ms < 10.0 {
        "B".to_string()
    } else if avg_ms < 100.0 && jitter_ms < 20.0 {
        "C".to_string()
    } else if avg_ms < 150.0 {
        "D".to_string()
    } else {
        "F".to_string()
    }
}

async fn resolve_host(host: &str) -> Option<String> {
    // Zaten IP ise direkt dön
    if let Ok(ip) = host.parse::<IpAddr>() {
        // IPv6 IP girilmişse yine de kabul et ama oyun ping'i için uyarı logla
        return Some(ip.to_string());
    }

    // DNS çözümleme — IPv4 öncelikli (-4 davranışı)
    use dns_lookup::lookup_host;
    match tokio::task::spawn_blocking({
        let host = host.to_string();
        move || lookup_host(&host)
    })
    .await
    {
        Ok(Ok(ips)) => {
            // Önce IPv4 adreslerini dene
            let ipv4 = ips.iter().find(|ip| ip.is_ipv4());
            let chosen = ipv4.or_else(|| ips.first());
            chosen.map(|ip| ip.to_string())
        }
        _ => None,
    }
}

// rand_id -> crate::rand_id() kullanılıyor

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

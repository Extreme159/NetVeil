use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, IcmpPacket, PingIdentifier, PingSequence, ICMP};
use tauri::{AppHandle, Emitter};
use tokio::time::timeout;

/// Frontend'e gönderilen anlık hız güncellemesi.
#[derive(Debug, Serialize, Clone)]
pub struct SpeedProgress {
    pub phase: String, // "download" | "upload" | "ping"
    pub mbps: f64,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpeedTestResult {
    pub download_mbps: f64,
    pub upload_mbps: f64,
    /// None = ping olculemedi (ICMP+TCP basarisiz); 0 ms ile karistirilmasin.
    pub ping_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
    pub server: String,
    pub download_peak_mbps: f64,
    pub upload_peak_mbps: f64,
}

// ----- Test parametreleri -----
//
// Cloudflare speed.cloudflare.com — edge CDN, her bölgede yakın PoP var.
// Bazı diğer hız testi servisleri resmi public API vermiyor;
// Cloudflare endpoint'i documented değil ama yıllardır stabil ve kendi
// speed.cloudflare.com sitesi de bunu kullanıyor.

const DL_URL_TPL: &str = "https://speed.cloudflare.com/__down?bytes=";
const UL_URL: &str = "https://speed.cloudflare.com/__up";
const META_URL: &str = "https://speed.cloudflare.com/meta";
const PING_TARGET: &str = "1.1.1.1";

// Multi-connection sayıları — tek TCP cap'i aşıp gerçek hat hızını yakalamak için.
// 8: netveil yolu (throttle_baseline) ile aynı — 1Gbps'te düşük ölçümü önler.
// Per-conn byte (64MB) CF /__down cap'inin (~95MB) altında; worker bitince reconnect.
const DL_PARALLEL: usize = 8;
// Upload küçük chunk'lar (256KB) ile gider — büyük chunk'larda counter
// POST bitene kadar 0 kalıp sonra sıçrıyor, bar dalgalanıyordu. Küçük chunk
// + fazla paralel = düzgün akış.
const UL_PARALLEL: usize = 4;

// Süre pencereleri
const WARMUP_SECS: u64 = 2;
const DL_DURATION_SECS: u64 = 12; // 2sn warm-up + 10sn ölçüm
const UL_DURATION_SECS: u64 = 12;
const PING_COUNT: usize = 15;

// Cloudflare'in /__down endpoint'i ~95MB cap'li (üstüne 403 atıyor).
// 64MB güvenli sınır; worker connection bitince yenisini açar, akış sürer.
const DL_BYTES_PER_CONN: u64 = 67_108_864;

#[tauri::command]
pub async fn run_speed_test(app: AppHandle, server: Option<String>) -> Result<SpeedTestResult, String> {
    // netveil.net sunucusu secildiyse throttle_baseline modulu kullanilir.
    // Ayni olcum motoru, ayni warmup/p90 mantigi -- kod tekrari yok.
    if server.as_deref() == Some("netveil") {
        // Sunucu mesgul mu? (IP-kilidi: ayni anda 1 kullanici)
        if !crate::throttle_baseline::probe_slot(Some("netveil")).await {
            return Err("SERVER_BUSY".to_string());
        }
        return run_speed_test_netveil(app).await;
    }
    crate::dlog!("[speedtest] BAŞLADI");

    // ---- 1. Sunucu meta (hangi Cloudflare PoP'a bağlandık?) ----
    let server = fetch_server_location().await;
    crate::dlog!("[speedtest] sunucu: {}", server);

    // ---- 2. Ping & jitter (Cloudflare DNS) ----
    let ping = measure_ping_jitter().await;
    crate::dlog!("[speedtest] ping/jitter: {:?}", ping);

    // ---- 3. Download testi ----
    crate::dlog!("[speedtest] download başlıyor...");
    let (download_mbps, download_peak_mbps) = run_download_test(&app).await?;
    crate::dlog!("[speedtest] download bitti: {:.1} Mbps (peak {:.1})", download_mbps, download_peak_mbps);
    // Download final (p90) degerini UI'a kilitle -> upload sonrasi rakam degismesin.
    let _ = app.emit("speedtest:phase_done", SpeedProgress {
        phase: "download".to_string(), mbps: round2(download_mbps), elapsed_ms: 0,
    });

    // ---- 4. Upload testi ----
    crate::dlog!("[speedtest] upload başlıyor...");
    let (upload_mbps, upload_peak_mbps) = run_upload_test(&app).await?;
    crate::dlog!("[speedtest] upload bitti: {:.1} Mbps (peak {:.1})", upload_mbps, upload_peak_mbps);

    Ok(SpeedTestResult {
        download_mbps: round2(download_mbps),
        upload_mbps: round2(upload_mbps),
        ping_ms: ping.map(|(p, _)| round2(p)),
        jitter_ms: ping.map(|(_, j)| round2(j)),
        server,
        download_peak_mbps: round2(download_peak_mbps),
        upload_peak_mbps: round2(upload_peak_mbps),
    })
}

/// Cloudflare'in /meta endpoint'i bağlandığımız edge PoP'u söyler.
async fn fetch_server_location() -> String {
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(_) => return "Cloudflare".to_string(),
    };

    let resp = match client.get(META_URL).send().await {
        Ok(r) => r,
        Err(_) => return "Cloudflare".to_string(),
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return "Cloudflare".to_string(),
    };

    // {"city":"Istanbul","country":"TR","colo":"IST",...}
    let city = json.get("city").and_then(|v| v.as_str()).unwrap_or("");
    let colo = json.get("colo").and_then(|v| v.as_str()).unwrap_or("");
    if !city.is_empty() && !colo.is_empty() {
        format!("Cloudflare {} ({})", city, colo)
    } else if !colo.is_empty() {
        format!("Cloudflare {}", colo)
    } else {
        "Cloudflare".to_string()
    }
}

/// ICMP ping ile latency + jitter (σ). Yaygın hız testi mantığı.
/// None = hic yanit alinamadi (ICMP ve TCP fallback basarisiz): 0 ms DEGIL, "olculemedi".
async fn measure_ping_jitter() -> Option<(f64, f64)> {
    let ip: IpAddr = PING_TARGET.parse().ok()?;
    let config = Config::builder().kind(ICMP::V4).build();
    let client = match Client::new(&config) {
        Ok(c) => c,
        Err(_) => return tcp_ping_fallback(ip).await,
    };

    let mut latencies: Vec<f64> = Vec::new();
    let payload = [0u8; 32];

    // Warm-up + 1 (ARP cache)
    for seq in 0..(PING_COUNT + 1) {
        let is_warmup = seq == 0;
        let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
        pinger.timeout(Duration::from_secs(2));

        let t0 = Instant::now();
        let res = timeout(
            Duration::from_secs(2),
            pinger.ping(PingSequence((seq % 65535) as u16), &payload),
        )
        .await;

        if !is_warmup {
            if let Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) = res {
                latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ICMP hic cevap vermediyse (ICMP bloklu) TCP-connect fallback'e dus (netveil yoluyla ayni)
    if latencies.is_empty() {
        return tcp_ping_fallback(ip).await;
    }

    let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let jitter = if latencies.len() > 1 {
        let var = latencies.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / latencies.len() as f64;
        var.sqrt()
    } else {
        0.0
    };

    Some((avg, jitter))
}

async fn tcp_ping_fallback(ip: IpAddr) -> Option<(f64, f64)> {
    use tokio::net::TcpStream;
    let mut lats = Vec::new();
    for _ in 0..PING_COUNT {
        let t0 = Instant::now();
        let addr = format!("{}:443", ip);
        if let Ok(Ok(_)) = timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
            lats.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if lats.is_empty() {
        return None;
    }
    let avg = lats.iter().sum::<f64>() / lats.len() as f64;
    let var = lats.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / lats.len() as f64;
    Some((avg, var.sqrt()))
}

/// Download — 8 paralel connection, warm-up sonrası ölçüm.
/// Sliding window peak (en yüksek 1sn'lik throughput) da hesaplanır.
async fn run_download_test(app: &AppHandle) -> Result<(f64, f64), String> {
    use futures::StreamExt;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Referer", "https://speed.cloudflare.com/".parse().expect("static URL"));
    headers.insert("Origin", "https://speed.cloudflare.com".parse().expect("static URL"));
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(DL_DURATION_SECS + 10))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .default_headers(headers)
        .build()
    {
        Ok(c) => c,
        Err(e) => return Err(e.to_string()),
    };

    // Atomic counter — sample'lar zamanla sliding window peak için
    let total_bytes = Arc::new(AtomicU64::new(0));
    let test_start = Instant::now();
    let warmup_deadline = test_start + Duration::from_secs(WARMUP_SECS);
    let deadline = test_start + Duration::from_secs(DL_DURATION_SECS);

    // Worker'ları başlat
    let mut handles = Vec::new();
    for _ in 0..DL_PARALLEL {
        let c = client.clone();
        let counter = total_bytes.clone();
        let url = format!("{}{}", DL_URL_TPL, DL_BYTES_PER_CONN);
        handles.push(tokio::spawn(async move {
            while Instant::now() < deadline {
                match c.get(&url).send().await {
                    Ok(resp) => {
                        let status = resp.status();
                        if !status.is_success() {
                            crate::dlog!("[DL] HTTP hata: {}", status);
                            tokio::time::sleep(Duration::from_millis(500)).await;
                            continue;
                        }
                        let mut stream = resp.bytes_stream();
                        let mut got_bytes = false;
                        while let Some(chunk) = stream.next().await {
                            match chunk {
                                Ok(bytes) => {
                                    got_bytes = true;
                                    counter.fetch_add(bytes.len() as u64, Ordering::Relaxed);
                                }
                                Err(_e) => {
                                    crate::dlog!("[DL] stream hata: {:?}", _e);
                                    break;
                                }
                            }
                            if Instant::now() >= deadline { break; }
                        }
                        if !got_bytes {
                            crate::dlog!("[DL] hic byte gelmedi");
                        }
                    }
                    Err(_e) => {
                        crate::dlog!("[DL] baglanti hatasi: {:?}", _e);
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        }));
    }

    // Sampler task — 200ms aralıklarla snapshot al + frontend'e emit et
    let samples_counter = total_bytes.clone();
    let app_for_sampler = app.clone();
    let sampler = tokio::spawn(async move {
        let mut samples: Vec<(Instant, u64)> = Vec::new();
        samples.push((Instant::now(), 0));
        let mut warmup_ref: Option<(Instant, u64)> = None; // warmup bittiğindeki referans noktası
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let now = Instant::now();
            let cur = samples_counter.load(Ordering::Relaxed);
            samples.push((now, cur));

            // Warm-up döneminde emit etme
            if now < warmup_deadline { continue; }

            // Warmup referans noktasını bir kez ayarla
            if warmup_ref.is_none() {
                warmup_ref = Some((now, cur));
                continue; // ilk ölçüm noktasını referans olarak kullan, emit etme
            }

            // Son 5 sample üzerinden sliding window
            let window_start = samples.len().saturating_sub(6);
            let (t0, b0) = samples[window_start];
            let dt = now.duration_since(t0).as_secs_f64();
            let db = cur.saturating_sub(b0);
            if dt > 0.1 && db > 0 {
                let mbps = (db as f64 * 8.0) / (dt * 1_000_000.0);
                let payload = SpeedProgress {
                    phase: "download".to_string(),
                    mbps: round2(mbps),
                    elapsed_ms: now.duration_since(test_start).as_millis() as u64,
                };
                let _ = app_for_sampler.emit("speedtest:progress", &payload);
                crate::dlog!("[speedtest] DL @{}ms = {:.1} Mbps", payload.elapsed_ms, payload.mbps);
            }
        }
        samples
    });

    // Grace YALNIZ faz penceresi (deadline) bittikten sonra: aktif workerlar erken abort edilmez;
    // 22sn HTTP timeout'una kadar asili kalanlar deadline + grace'te abort edilir.
    crate::throttle_baseline::join_after_deadline(deadline, handles, Duration::from_secs(crate::throttle_baseline::JOIN_GRACE_SECS)).await;
    let samples = sampler.await.unwrap_or_default();

    // Warm-up sonrası ortalama
    // Warmup dönemindeki sample'ları çıkar — aksi halde düşük başlangıç değerleri
    // peak/p90 hesabını olumsuz etkiler ve sonuç şişer.
    let post_warmup_samples: Vec<(Instant, u64)> = samples
        .iter()
        .copied()
        .filter(|(t, _)| *t > warmup_deadline)
        .collect();

    // p90 sliding window — tüm ölçüm pencerelerinin 90. yüzdelik değeri.
    // Hem düşük başlangıç spike'larını hem yüksek ani zirveleri eler,
    // gerçek sürekli hat kapasitesini yansıtır.
    let peak_mbps = sliding_peak_mbps(&post_warmup_samples, Duration::from_secs(1));
    let p90_mbps  = sliding_p90_mbps(&post_warmup_samples);
    // p90 > peak'in %50'si ise p90 kullan, değilse (warmup uzadiysa) peak'in %90'i
    let result_mbps = if p90_mbps > peak_mbps * 0.5 { p90_mbps } else { peak_mbps * 0.9 };

    // Hic gecerli byte akmadiysa (tum istekler HTTP hata/baglanti hatasi) 0 Mbps "sonuc" degil hata.
    crate::throttle_baseline::check_phase_result(result_mbps, false, "download")?;
    Ok((result_mbps, peak_mbps))
}

/// Upload — 4 paralel POST, chunk'lı stream.
async fn run_upload_test(app: &AppHandle) -> Result<(f64, f64), String> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(UL_DURATION_SECS + 10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return Err(e.to_string()),
    };

    // 256KB chunk — her POST kısa sürer, counter düzenli artar,
    // sampler bar'ı oynamadan smooth okur.
    let chunk: Arc<Vec<u8>> = Arc::new(vec![0u8; 262_144]);
    let total_bytes = Arc::new(AtomicU64::new(0));
    let test_start = Instant::now();
    let warmup_deadline = test_start + Duration::from_secs(WARMUP_SECS);
    let deadline = test_start + Duration::from_secs(UL_DURATION_SECS);

    let mut handles = Vec::new();
    for _ in 0..UL_PARALLEL {
        let c = client.clone();
        let counter = total_bytes.clone();
        let chunk = chunk.clone();
        handles.push(tokio::spawn(async move {
            while Instant::now() < deadline {
                let body = (*chunk).clone();
                let len = body.len() as u64;
                match c.post(UL_URL).body(body).send().await {
                    // Yalniz basarili (2xx) yanit sayilir: 4xx/5xx ile reddedilen upload olcume girmez.
                    Ok(resp) if resp.status().is_success() => {
                        counter.fetch_add(len, Ordering::Relaxed);
                    }
                    Ok(_rejected) => {
                        crate::dlog!("[UL] HTTP hata: {}", _rejected.status());
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        }));
    }

    let samples_counter = total_bytes.clone();
    let app_for_sampler = app.clone();
    let sampler = tokio::spawn(async move {
        let mut samples: Vec<(Instant, u64)> = Vec::new();
        samples.push((Instant::now(), 0));
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let now = Instant::now();
            let cur = samples_counter.load(Ordering::Relaxed);
            samples.push((now, cur));

            // Warm-up döneminde emit etme
            if now < warmup_deadline { continue; }

            // Son 6 sample sliding window
            let window_start = samples.len().saturating_sub(6);
            let (t0, b0) = samples[window_start];
            let dt = now.duration_since(t0).as_secs_f64();
            let db = cur.saturating_sub(b0);
            if dt > 0.1 && db > 0 {
                let mbps = (db as f64 * 8.0) / (dt * 1_000_000.0);
                let elapsed_ms = now.duration_since(test_start).as_millis() as u64;
                let _ = app_for_sampler.emit(
                    "speedtest:progress",
                    SpeedProgress {
                        phase: "upload".to_string(),
                        mbps: round2(mbps),
                        elapsed_ms,
                    },
                );
                crate::dlog!("[speedtest] UL @{}ms = {:.1} Mbps", elapsed_ms, round2(mbps));
            }
        }
        samples
    });

    crate::throttle_baseline::join_after_deadline(deadline, handles, Duration::from_secs(crate::throttle_baseline::JOIN_GRACE_SECS)).await;
    let samples = sampler.await.unwrap_or_default();

    // Warmup dönemindeki sample'ları çıkar
    let post_warmup_samples: Vec<(Instant, u64)> = samples
        .iter()
        .copied()
        .filter(|(t, _)| *t > warmup_deadline)
        .collect();

    let peak_mbps   = sliding_peak_mbps(&post_warmup_samples, Duration::from_secs(1));
    let p90_mbps    = sliding_p90_mbps(&post_warmup_samples);
    let result_mbps = if p90_mbps > peak_mbps * 0.5 { p90_mbps } else { peak_mbps * 0.9 };

    crate::throttle_baseline::check_phase_result(result_mbps, false, "upload")?;
    Ok((result_mbps, peak_mbps))
}

/// Sliding window içinde en yüksek throughput'u (Mbps) bul.
fn sliding_peak_mbps(samples: &[(Instant, u64)], window: Duration) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mut peak = 0.0f64;
    for i in 0..samples.len() {
        let (t_end, b_end) = samples[i];
        // Window'un başlangıcına en yakın sample'ı ara
        let target = match t_end.checked_sub(window) {
            Some(t) => t,
            None => continue,
        };
        // En küçük t >= target olan sample (lineer tara, sample küçük)
        let mut start_idx = i;
        for j in (0..=i).rev() {
            if samples[j].0 <= target {
                start_idx = j;
                break;
            }
            start_idx = j;
        }
        let (t_start, b_start) = samples[start_idx];
        let dt = t_end.duration_since(t_start).as_secs_f64();
        if dt < 0.3 {
            continue;
        }
        let db = b_end.saturating_sub(b_start);
        let mbps = (db as f64 * 8.0) / (dt * 1_000_000.0);
        if mbps > peak {
            peak = mbps;
        }
    }
    peak
}

/// Tüm 200ms ölçüm pencerelerinin 90. yüzdelik Mbps değeri.
/// Ani spike'ları ve başlangıç düşük değerlerini eler.
fn sliding_p90_mbps(samples: &[(Instant, u64)]) -> f64 {
    if samples.len() < 4 { return 0.0; }
    let window = Duration::from_secs(1);
    let mut values: Vec<f64> = Vec::new();
    for i in 1..samples.len() {
        let (t_end, b_end) = samples[i];
        let t_start_target = match t_end.checked_sub(window) { Some(t) => t, None => continue };
        let mut start_idx = i;
        for j in (0..=i).rev() {
            if samples[j].0 <= t_start_target { start_idx = j; break; }
            start_idx = j;
        }
        let (t_start, b_start) = samples[start_idx];
        let dt = t_end.duration_since(t_start).as_secs_f64();
        if dt < 0.3 { continue; }
        let db = b_end.saturating_sub(b_start);
        values.push((db as f64 * 8.0) / (dt * 1_000_000.0));
    }
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() as f64 * 0.90) as usize).min(values.len() - 1);
    values[idx]
}


// -- netveil.net hiz testi --------------------------------------------------
// throttle_baseline ile ayni olcum motoru: warmup 4s, p90 hesabi, 16MB H2 window.
// speedtest:progress event'i emit eder -- SpeedTestTab canli bar icin.
async fn run_speed_test_netveil(app: AppHandle) -> Result<SpeedTestResult, String> {
    use futures::StreamExt as _;
    use std::time::Instant;

    crate::dlog!("[speedtest/netveil] basliyor");

    let ping = measure_ping_jitter_netveil().await;
    // 429 = sunucu IP-kilidi (baska kullanici test ediyor): dusuk/sifir hiz degil SERVER_BUSY.
    let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // throttle_baseline ile identik parametreler
    const DL_URL:    &str   = "https://api.netveil.net/dl";
    const DL_BYTES:  u64    = 524_288_000;
    const DL_PAR:    usize  = 8;
    const UL_PAR:    usize  = 6;
    const WARMUP:    f64    = 4.0;
    const TOTAL:     f64    = 12.0;
    const EMIT_MS:   u64    = 200;
    const WINDOW:    usize  = 15;

    let client = match crate::throttle_baseline::build_client() {
        Ok(c) => c,
        Err(e) => return Err(e.to_string()),
    };

    // ---- Download ----
    let dl_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let dl_stop    = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let dl_start   = Instant::now();

    let mut dl_handles = Vec::new();
    for _ in 0..DL_PAR {
        let c = dl_counter.clone(); let s = dl_stop.clone(); let cl = client.clone();
        let b = busy.clone();
        let url = format!("{}?bytes={}", DL_URL, DL_BYTES);
        dl_handles.push(tokio::spawn(async move {
            while !s.load(Ordering::Relaxed) {
                let resp = match cl.get(&url).send().await {
                    Ok(r) if r.status().is_success() => r,
                    other => {
                           if let Ok(r) = &other {
                               if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                                   b.store(true, Ordering::Relaxed);
                               }
                           }
                           if s.load(Ordering::Relaxed) { break; }
                           tokio::time::sleep(Duration::from_millis(50)).await; continue; }
                };
                let mut stream = resp.bytes_stream();
                loop {
                    if s.load(Ordering::Relaxed) { return; }
                    match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
                        Ok(Some(Ok(chunk))) => { c.fetch_add(chunk.len() as u64, Ordering::Relaxed); }
                        Ok(None) => break,
                        Ok(Some(Err(_))) => break,
                        Err(_) => { if s.load(Ordering::Relaxed) { return; } }
                    }
                }
            }
        }));
    }

    let (dl_result, dl_peak) = netveil_measure_loop(
        &app, dl_counter.clone(), dl_stop.clone(), dl_start, "download", WARMUP, TOTAL, EMIT_MS, WINDOW
    ).await;
    crate::throttle_baseline::join_bounded(dl_handles, Duration::from_secs(crate::throttle_baseline::JOIN_GRACE_SECS)).await;
    crate::throttle_baseline::check_phase_result(dl_result, busy.load(Ordering::Relaxed), "download")?;
    // Download final (p90) degerini UI'a kilitle -> upload sonrasi rakam degismesin.
    let _ = app.emit("speedtest:phase_done", SpeedProgress {
        phase: "download".to_string(), mbps: round2(dl_result), elapsed_ms: 0,
    });

    // ---- Upload ----
    let ul_counter = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let ul_stop    = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ul_start   = Instant::now();
    let ul_tally   = Arc::new(crate::throttle_baseline::UploadTally::new(UL_PAR));

    let mut ul_handles = Vec::new();
    for idx in 0..UL_PAR {
        let c = ul_counter.clone(); let s = ul_stop.clone(); let cl = client.clone();
        let tally = ul_tally.clone();
        ul_handles.push(tokio::spawn(async move {
            let cs = c.clone(); let ss = s.clone(); let ts = tally.clone();
            let stream = futures::stream::unfold((), move |_| {
                let c2 = cs.clone(); let s2 = ss.clone(); let t2 = ts.clone();
                async move {
                    if s2.load(Ordering::Relaxed) { return None; }
                    tokio::task::yield_now().await;
                    if s2.load(Ordering::Relaxed) { return None; }
                    let chunk = vec![0xAAu8; 16_384];
                    c2.fetch_add(16_384, Ordering::Relaxed);
                    t2.add_bytes(idx, 16_384);
                    Some((Ok::<_, std::convert::Infallible>(chunk), ()))
                }
            });
            let resp = cl.post("https://api.netveil.net/ul")
                .header("content-type", "application/octet-stream")
                .body(reqwest::Body::wrap_stream(stream))
                // Client timeout'u (60s) toplam istegi keser; adaptif onay beklemesine yer ac.
                .timeout(Duration::from_secs_f64(TOTAL) + Duration::from_secs(crate::throttle_baseline::UPLOAD_CONFIRM_MAX_SECS + 5))
                .send().await;
            // Byte'lar govde uretilirken sayilir; onemli byte ureten bir worker sunucudan 2xx
            // almadiysa (429/4xx/5xx/baglanti hatasi/yanit yok) olcum gecerli degildir.
            // + NetVeil /ul govdesi {"received": N}: N >= %95 uretilen byte degilse onaysiz.
            tally.record_result(idx, crate::throttle_baseline::upload_response(resp, true).await);
        }));
    }

    let (ul_result, ul_peak) = netveil_measure_loop(
        &app, ul_counter.clone(), ul_stop.clone(), ul_start, "upload", WARMUP, TOTAL, EMIT_MS, WINDOW
    ).await;
    // Sunucu yaniti govde bitince (stop) gelir; kuyruk bosalana kadar olcum penceresinden SONRA
    // sinirli bekleme (olculen byte/sure degismez, istekler cozulunce erken biter).
    // Bekleme suresi fazin kendi olculen hizina gore adaptif (yavas uplink -> daha uzun, <=60s).
    let ul_grace = crate::throttle_baseline::upload_confirm_grace(ul_counter.load(Ordering::Relaxed), ul_result, TOTAL);
    crate::throttle_baseline::join_uploads(ul_handles, &ul_tally, ul_grace).await;
    ul_tally.verdict()?;
    crate::throttle_baseline::check_phase_result(ul_result, false, "upload")?;

    crate::dlog!("[speedtest/netveil] DL: {:.1} (peak {:.1}) UL: {:.1} (peak {:.1})",
        dl_result, dl_peak, ul_result, ul_peak);

    Ok(SpeedTestResult {
        download_mbps:      round2(dl_result),
        upload_mbps:        round2(ul_result),
        ping_ms:            ping.map(|(p, _)| round2(p)),
        jitter_ms:          ping.map(|(_, j)| round2(j)),
        server:             "netveil.net · Frankfurt".to_string(),
        download_peak_mbps: round2(dl_peak),
        upload_peak_mbps:   round2(ul_peak),
    })
}

// throttle_baseline::measure_loop ile ayni mantik; speedtest:progress emit eder.
#[allow(clippy::too_many_arguments)]
async fn netveil_measure_loop(
    app: &AppHandle,
    counter: Arc<std::sync::atomic::AtomicU64>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    start: std::time::Instant,
    phase: &str,
    warmup: f64,
    total_secs: f64,
    emit_ms: u64,
    window_buckets: usize,
) -> (f64, f64) {
    let mut interval = tokio::time::interval(Duration::from_millis(emit_ms));
    let mut warmup_done = false;
    let mut warmup_bytes: u64 = 0;
    let mut measure_start = std::time::Instant::now();
    let mut prev: u64 = 0;
    let mut peak: f64 = 0.0;
    let mut buckets: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    let mut all_samples: Vec<(std::time::Instant, u64)> = Vec::new();

    loop {
        interval.tick().await;
        let elapsed = start.elapsed().as_secs_f64();
        let total   = counter.load(Ordering::Relaxed);

        if !warmup_done && elapsed >= warmup {
            warmup_done = true; warmup_bytes = total;
            measure_start = std::time::Instant::now(); prev = total;
            all_samples.push((std::time::Instant::now(), total));
        }
        if warmup_done {
            all_samples.push((std::time::Instant::now(), total));
            let delta = total.saturating_sub(prev); prev = total;
            buckets.push_back(delta);
            while buckets.len() > window_buckets { buckets.pop_front(); }
            let wb: u64 = buckets.iter().sum();
            let ws = buckets.len() as f64 * (emit_ms as f64 / 1000.0);
            let mbps = if ws > 0.01 { wb as f64 * 8.0 / (ws * 1_000_000.0) } else { 0.0 };
            if mbps > peak { peak = mbps; }
            let _ = app.emit("speedtest:progress", SpeedProgress {
                phase: phase.to_string(), mbps: round2(mbps),
                elapsed_ms: (elapsed * 1000.0) as u64,
            });
        }
        if elapsed >= total_secs { stop.store(true, Ordering::Relaxed); break; }
    }

    // p90 -- throttle_baseline::compute_p90 ile ayni algoritma
    let p90 = {
        let window = Duration::from_secs(1);
        let mut values: Vec<f64> = Vec::new();
        for i in 1..all_samples.len() {
            let (t_end, b_end) = all_samples[i];
            let target = match t_end.checked_sub(window) { Some(t) => t, None => continue };
            let mut si = 0;
            for j in (0..i).rev() { if all_samples[j].0 <= target { si = j; break; } }
            let (t_start, b_start) = all_samples[si];
            let dt = t_end.duration_since(t_start).as_secs_f64();
            if dt < 0.3 { continue; }
            values.push(b_end.saturating_sub(b_start) as f64 * 8.0 / (dt * 1_000_000.0));
        }
        if values.len() >= 4 {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            values[((values.len() as f64 * 0.90) as usize).min(values.len() - 1)]
        } else { 0.0 }
    };

    let me = measure_start.elapsed().as_secs_f64();
    let tf = counter.load(Ordering::Relaxed);
    let avg = if me > 0.5 { tf.saturating_sub(warmup_bytes) as f64 * 8.0 / (me * 1_000_000.0) } else { 0.0 };
    let result = if p90 > avg * 0.5 { p90 } else { avg };
    (result, peak)
}

/// netveil.net RTT/jitter olcumu — GERCEK ICMP (CF yoluyla tutarli).
/// Eski HTTP GET yontemi TLS+HTTP overhead iceriyordu (gercek RTT degil).
/// netveil.net IP'ye resolve edilir, ICMP ping atilir; ICMP yoksa TCP-connect(443) fallback.
async fn measure_ping_jitter_netveil() -> Option<(f64, f64)> {
    // netveil.net -> IPv4 (ICMP V4 icin; CF yolu sabit IP kullaniyor)
    let ip: IpAddr = match tokio::net::lookup_host("api.netveil.net:443").await {
        Ok(mut addrs) => match addrs.find(|a| a.is_ipv4()) {
            Some(a) => a.ip(),
            None => return None,
        },
        Err(_) => return None,
    };

    let config = Config::builder().kind(ICMP::V4).build();
    let client = match Client::new(&config) {
        Ok(c) => c,
        Err(_) => return tcp_ping_fallback(ip).await,
    };

    let mut latencies: Vec<f64> = Vec::new();
    let payload = [0u8; 32];

    // Warm-up + PING_COUNT (ilk seq warmup, ARP/route cache)
    for seq in 0..(PING_COUNT + 1) {
        let is_warmup = seq == 0;
        let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
        pinger.timeout(Duration::from_secs(2));

        let t0 = Instant::now();
        let res = timeout(
            Duration::from_secs(2),
            pinger.ping(PingSequence((seq % 65535) as u16), &payload),
        )
        .await;

        if !is_warmup {
            if let Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) = res {
                latencies.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // ICMP hic cevap vermediyse (ICMP bloklu) TCP-connect fallback'e dus
    if latencies.is_empty() {
        return tcp_ping_fallback(ip).await;
    }

    let avg = latencies.iter().sum::<f64>() / latencies.len() as f64;
    let var = latencies.iter().map(|x| (x - avg).powi(2)).sum::<f64>() / latencies.len() as f64;
    Some((avg, var.sqrt()))
}

// rand_id -> crate::rand_id() kullanılıyor

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

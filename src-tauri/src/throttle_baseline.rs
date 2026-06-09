/// Baseline HTTPS hiz testi -- netveil.net Frankfurt dedicated sunucu
///
/// Cloudflare'den bagimsiz, kendi sunucumuza TLS uzerinden gercek throughput olcumu.
/// ISP throttling tespiti icin referans hiz bu modulden alinir.
/// Ayni zamanda netveil.net speedtest icin de ortak kaynak olarak kullanilir.
///
/// - Download: 8 paralel HTTP/2 stream, 500MB/conn (stop flag ile 12s'de kesilir)
/// - Upload:   6 paralel HTTP/2 POST, infinite stream
/// - Warmup:   ilk 4 saniye atilir (TCP buffer auto-tuning + H2 window yerlesme)
/// - Olcum:    8 saniye (toplam 12s)
/// - Sonuc:    p90 (tum 1s pencerelerin 90. yuzdeligI) -- yaygın hız testi aracları gibi
#[cfg(feature = "premium")]
use futures::StreamExt as _;
#[cfg(feature = "premium")]
use serde::{Deserialize, Serialize};
#[cfg(feature = "premium")]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(feature = "premium")]
use std::sync::Arc;
use std::time::Duration;
#[cfg(feature = "premium")]
use std::time::Instant;
#[cfg(feature = "premium")]
use tauri::{AppHandle, Emitter};

#[cfg(feature = "premium")]
const SERVER_DL: &str = "https://netveil.net/dl";
#[cfg(feature = "premium")]
const SERVER_UL: &str = "https://netveil.net/ul";
#[cfg(feature = "premium")]
const DL_BYTES_PER_CONN: u64 = 524_288_000; // 500MB -- 1Gbps'te ~4sn, reconnect azaltir
#[cfg(feature = "premium")]
const DL_PARALLEL: usize = 8;
#[cfg(feature = "premium")]
const UL_PARALLEL: usize = 6;
#[cfg(feature = "premium")]
const WARMUP_SECS: f64 = 4.0;  // TCP buffer auto-tuning + H2 window yerlesme icin
#[cfg(feature = "premium")]
const MEASURE_SECS: f64 = 8.0;
#[cfg(feature = "premium")]
const TOTAL_SECS: f64 = WARMUP_SECS + MEASURE_SECS; // 12s toplam
#[cfg(feature = "premium")]
const EMIT_MS: u64 = 200;
#[cfg(feature = "premium")]
const WINDOW_BUCKETS: usize = 15; // 3s sliding window (15 * 200ms)

#[cfg(feature = "premium")]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BaselineResult {
    pub download_mbps: f64,
    pub upload_mbps: f64,
    pub download_peak_mbps: f64,
    pub upload_peak_mbps: f64,
    pub server: String,
}

#[cfg(feature = "premium")]
#[derive(Debug, Serialize, Clone)]
struct BaselineProgress {
    phase: String,      // "download" | "upload"
    mbps: f64,
    peak_mbps: f64,
    status: String,     // "running" | "done" | "error"
}

#[cfg(feature = "premium")]
pub async fn run_baseline(app: &AppHandle) -> Result<BaselineResult, String> {
    crate::dlog!("[baseline] basliyor -- netveil.net Frankfurt");

    let (dl_p90, dl_peak) = run_download(app).await?;
    crate::dlog!("[baseline] download: {:.1} Mbps p90 (peak {:.1})", dl_p90, dl_peak);

    let (ul_p90, ul_peak) = run_upload(app).await?;
    crate::dlog!("[baseline] upload: {:.1} Mbps p90 (peak {:.1})", ul_p90, ul_peak);

    Ok(BaselineResult {
        download_mbps:      round2(dl_p90),
        upload_mbps:        round2(ul_p90),
        download_peak_mbps: round2(dl_peak),
        upload_peak_mbps:   round2(ul_peak),
        server: "netveil.net · Frankfurt".to_string(),
    })
}

// --- Download ----------------------------------------------------------------

#[cfg(feature = "premium")]
async fn run_download(app: &AppHandle) -> Result<(f64, f64), String> {
    let counter = Arc::new(AtomicU64::new(0));
    let stop    = Arc::new(AtomicBool::new(false));
    let start   = Instant::now();

    let mut handles = Vec::new();
    for _ in 0..DL_PARALLEL {
        let c = counter.clone();
        let s = stop.clone();
        handles.push(tokio::spawn(download_worker(c, s)));
    }

    let (p90, peak) = measure_loop(app, counter.clone(), stop.clone(), start, "download").await;
    for h in handles { let _ = h.await; }

    if p90 < 0.1 { return Err("Sunucuya baglanilam adi (download)".to_string()); }
    Ok((p90, peak))
}

#[cfg(feature = "premium")]
async fn download_worker(counter: Arc<AtomicU64>, stop: Arc<AtomicBool>) {
    let url = format!("{}?bytes={}", SERVER_DL, DL_BYTES_PER_CONN);
    let client = match build_client() { Ok(c) => c, Err(_) => return };

    while !stop.load(Ordering::Relaxed) {
        let resp = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            _ => {
                if stop.load(Ordering::Relaxed) { break; }
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let mut stream = resp.bytes_stream();
        loop {
            if stop.load(Ordering::Relaxed) { return; }
            match tokio::time::timeout(Duration::from_millis(300), stream.next()).await {
                Ok(Some(Ok(chunk))) => { counter.fetch_add(chunk.len() as u64, Ordering::Relaxed); }
                Ok(None) => break,
                Ok(Some(Err(_))) => break,
                Err(_) => { if stop.load(Ordering::Relaxed) { return; } }
            }
        }
    }
}

// --- Upload ------------------------------------------------------------------

#[cfg(feature = "premium")]
async fn run_upload(app: &AppHandle) -> Result<(f64, f64), String> {
    let counter = Arc::new(AtomicU64::new(0));
    let stop    = Arc::new(AtomicBool::new(false));
    let start   = Instant::now();

    let mut handles = Vec::new();
    for _ in 0..UL_PARALLEL {
        let c = counter.clone();
        let s = stop.clone();
        handles.push(tokio::spawn(upload_worker(c, s)));
    }

    let (p90, peak) = measure_loop(app, counter.clone(), stop.clone(), start, "upload").await;
    for h in handles { let _ = h.await; }

    if p90 < 0.1 { return Err("Sunucuya baglanilam adi (upload)".to_string()); }
    Ok((p90, peak))
}

#[cfg(feature = "premium")]
async fn upload_worker(counter: Arc<AtomicU64>, stop: Arc<AtomicBool>) {
    // 16KB chunk + yield_now her chunkta: HTTP/2 backpressure minimize edilir.
    const UL_CHUNK: usize = 16_384;

    let client = match build_client() { Ok(c) => c, Err(_) => return };
    let counter_stream = counter.clone();
    let stop_stream    = stop.clone();

    let stream = futures::stream::unfold((), move |_| {
        let c = counter_stream.clone();
        let s = stop_stream.clone();
        async move {
            if s.load(Ordering::Relaxed) { return None; }
            tokio::task::yield_now().await;
            if s.load(Ordering::Relaxed) { return None; }
            let chunk = vec![0xAAu8; UL_CHUNK];
            c.fetch_add(UL_CHUNK as u64, Ordering::Relaxed);
            Some((Ok::<_, std::convert::Infallible>(chunk), ()))
        }
    });

    let _ = client
        .post(SERVER_UL)
        .header("content-type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
}

// --- Olcum dongusu -----------------------------------------------------------
//
// Her 200ms'de counter okur, sample kaydeder.
// Display icin: WINDOW_BUCKETS * 200ms sliding window.
// Sonuc icin: p90 (tum 1s pencerelerin 90. yuzdeligI).
//
// Neden p90?
//   Ham ortalama baslangictaki TCP ramp-up'i iceriyor -> dusuk cikar.
//   p90: ani spike ve baslangic diplerini eler, gercek surekli kapasiteyi yansitir.
//   yaygın hız testi aracları da p90/p95 kullanir.

#[cfg(feature = "premium")]
async fn measure_loop(
    app: &AppHandle,
    counter: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    start: Instant,
    phase: &str,
) -> (f64, f64) {
    let mut interval = tokio::time::interval(Duration::from_millis(EMIT_MS));

    let mut warmup_done   = false;
    let mut warmup_bytes: u64 = 0;
    let mut measure_start = Instant::now();
    let mut prev_bytes: u64 = 0;
    let mut peak_mbps: f64 = 0.0;

    // Display sliding window
    let mut buckets = std::collections::VecDeque::<u64>::new();

    // p90 icin tum warmup-sonrasi sample'lar: (zaman, kumulatif_byte)
    let mut all_samples: Vec<(Instant, u64)> = Vec::new();

    loop {
        interval.tick().await;
        let elapsed = start.elapsed().as_secs_f64();
        let total   = counter.load(Ordering::Relaxed);

        if !warmup_done && elapsed >= WARMUP_SECS {
            warmup_done   = true;
            warmup_bytes  = total;
            measure_start = Instant::now();
            prev_bytes    = total;
            all_samples.clear();
            all_samples.push((Instant::now(), total));
            crate::dlog!("[baseline:{}] warmup bitti -- olcum basliyor", phase);
        }

        if warmup_done {
            all_samples.push((Instant::now(), total));

            // Display: sliding window delta
            let delta = total.saturating_sub(prev_bytes);
            prev_bytes = total;
            buckets.push_back(delta);
            while buckets.len() > WINDOW_BUCKETS { buckets.pop_front(); }

            let window_bytes: u64 = buckets.iter().sum();
            let window_secs = buckets.len() as f64 * (EMIT_MS as f64 / 1000.0);
            let current_mbps = bits_to_mbps(window_bytes, window_secs);
            if current_mbps > peak_mbps { peak_mbps = current_mbps; }

            let _ = app.emit("throttle:baseline", BaselineProgress {
                phase: phase.to_string(),
                mbps: round2(current_mbps),
                peak_mbps: round2(peak_mbps),
                status: "running".to_string(),
            });
        }

        if elapsed >= TOTAL_SECS {
            stop.store(true, Ordering::Relaxed);
            break;
        }
    }

    // p90 hesabi
    let p90_mbps = compute_p90(&all_samples, Duration::from_secs(1));

    // Fallback: ham ortalama (sample az oldugunda)
    let measure_elapsed = measure_start.elapsed().as_secs_f64();
    let total_final     = counter.load(Ordering::Relaxed);
    let avg_mbps = if measure_elapsed > 0.5 {
        bits_to_mbps(total_final.saturating_sub(warmup_bytes), measure_elapsed)
    } else { 0.0 };

    let result_mbps = if p90_mbps > avg_mbps * 0.5 { p90_mbps } else { avg_mbps };

    crate::dlog!("[baseline:{}] avg={:.1} p90={:.1} peak={:.1} -> result={:.1}",
        phase, avg_mbps, p90_mbps, peak_mbps, result_mbps);

    let _ = app.emit("throttle:baseline", BaselineProgress {
        phase: phase.to_string(),
        mbps: round2(result_mbps),
        peak_mbps: round2(peak_mbps),
        status: "done".to_string(),
    });

    (result_mbps, peak_mbps)
}

// --- p90 yardimcisi ----------------------------------------------------------
// Verilen sample dizisinden 1s'lik kayan pencereler olusturur,
// her pencere icin Mbps hesaplar, 90. yuzdeligI dondurur.

#[cfg(feature = "premium")]
fn compute_p90(samples: &[(Instant, u64)], window: Duration) -> f64 {
    if samples.len() < 4 { return 0.0; }
    let mut values: Vec<f64> = Vec::new();
    for i in 1..samples.len() {
        let (t_end, b_end) = samples[i];
        let target = match t_end.checked_sub(window) { Some(t) => t, None => continue };
        let mut start_idx = 0;
        for j in (0..i).rev() {
            if samples[j].0 <= target { start_idx = j; break; }
        }
        let (t_start, b_start) = samples[start_idx];
        let dt = t_end.duration_since(t_start).as_secs_f64();
        if dt < 0.3 { continue; }
        let db = b_end.saturating_sub(b_start);
        values.push(bits_to_mbps(db, dt));
    }
    if values.is_empty() { return 0.0; }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((values.len() as f64 * 0.90) as usize).min(values.len() - 1);
    values[idx]
}

// --- Yardimcilar -------------------------------------------------------------

/// Sunucuda aktif test slotu musait mi diye hafif bir kontrol yapar.
/// Bir kullanici test ederken sunucu IP-kilidi nedeniyle baska IP'ye 429 doner.
/// true  = slot alindi/musait (test baslatilabilir)
/// false = sunucu mesgul (baska kullanici aktif test ediyor)
/// Sunucuya ulasilamazsa (ag hatasi) true doner — testin kendi hata yolu devralir.
pub async fn probe_slot(server: Option<&str>) -> bool {
    let url = match server {
        Some("netveil") => "https://netveil.net/dl?bytes=1",
        _ => return true, // Cloudflare'da boyle bir kilit yok; probe gereksiz
    };
    let client = match build_client() {
        Ok(c) => c,
        Err(_) => return true,
    };
    match tokio::time::timeout(Duration::from_secs(5), client.get(url).send()).await {
        Ok(Ok(resp)) => resp.status() != reqwest::StatusCode::TOO_MANY_REQUESTS, // 429 = mesgul
        _ => true, // ag hatasi/timeout -> testin normal hata yolu ilgilensin
    }
}

pub fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .use_rustls_tls()
        // H2 flow control window: 16MB stream / 64MB connection
        // 50ms RTT'de: 16MB*8/0.05/1e6 = 2684 Mbps/stream -> 1Gbps icin yeterli
        .http2_initial_stream_window_size(16 * 1024 * 1024)
        .http2_initial_connection_window_size(64 * 1024 * 1024)
        .http2_adaptive_window(true)
        .timeout(Duration::from_secs(60))
        .pool_max_idle_per_host(0)
        .build()
}

#[cfg(feature = "premium")]
fn bits_to_mbps(bytes: u64, secs: f64) -> f64 {
    if secs < 0.01 { return 0.0; }
    (bytes as f64 * 8.0) / (secs * 1_000_000.0)
}

#[cfg(feature = "premium")]
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

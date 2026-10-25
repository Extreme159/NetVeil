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
const SERVER_DL: &str = "https://api.netveil.net/dl";
#[cfg(feature = "premium")]
const SERVER_UL: &str = "https://api.netveil.net/ul";
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
    let busy    = Arc::new(AtomicBool::new(false));
    let start   = Instant::now();

    let mut handles = Vec::new();
    for _ in 0..DL_PARALLEL {
        let c = counter.clone();
        let s = stop.clone();
        let b = busy.clone();
        handles.push(tokio::spawn(download_worker(c, s, b)));
    }

    let (p90, peak) = measure_loop(app, counter.clone(), stop.clone(), start, "download").await;
    // Faz penceresi bitti: asili kalan istekler 60sn HTTP timeout'unu beklemesin.
    join_bounded(handles, Duration::from_secs(JOIN_GRACE_SECS)).await;

    if p90 < MIN_VALID_MBPS {
        // 429 = sunucu IP-kilidi (baska kullanici test ediyor) -> dusuk hiz DEGIL, mesgul.
        return Err(if busy.load(Ordering::Relaxed) { "SERVER_BUSY".to_string() }
                   else { format!("{}download", ERR_NO_DATA_PREFIX) });
    }
    Ok((p90, peak))
}

#[cfg(feature = "premium")]
async fn download_worker(counter: Arc<AtomicU64>, stop: Arc<AtomicBool>, busy: Arc<AtomicBool>) {
    let url = format!("{}?bytes={}", SERVER_DL, DL_BYTES_PER_CONN);
    let client = match build_client() { Ok(c) => c, Err(_) => return };

    while !stop.load(Ordering::Relaxed) {
        let resp = match client.get(&url).send().await {
            Ok(r) if r.status().is_success() => r,
            other => {
                if let Ok(r) = &other {
                    if r.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                        busy.store(true, Ordering::Relaxed);
                    }
                }
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
    let tally   = Arc::new(UploadTally::new(UL_PARALLEL));
    let start   = Instant::now();

    let mut handles = Vec::new();
    for idx in 0..UL_PARALLEL {
        let c = counter.clone();
        let s = stop.clone();
        let t = tally.clone();
        handles.push(tokio::spawn(upload_worker(idx, c, s, t)));
    }

    // measure_loop faz penceresi bitince stop'u kaldirir; join ANCAK bundan sonra baslar.
    let (p90, peak) = measure_loop(app, counter.clone(), stop.clone(), start, "upload").await;
    // Sunucu /ul yaniti govde bitince (stop) gonderir. Kuyruktaki byte'lar bosalana kadar
    // (olcum penceresinden SONRA, olculen byte/sure degismez) sinirli sure beklenir.
    let grace = upload_confirm_grace(counter.load(Ordering::Relaxed), p90, TOTAL_SECS);
    join_uploads(handles, &tally, grace).await;

    // Byte'lar govde uretilirken sayildi; onemli byte ureten bir worker sunucudan 2xx almadiysa
    // (429/4xx/5xx/baglanti hatasi/yanit yok) olcum dogrulanmamis byte icerir -> gecersiz.
    tally.verdict()?;
    if p90 < MIN_VALID_MBPS {
        return Err(format!("{}upload", ERR_NO_DATA_PREFIX));
    }
    Ok((p90, peak))
}

#[cfg(feature = "premium")]
async fn upload_worker(idx: usize, counter: Arc<AtomicU64>, stop: Arc<AtomicBool>, tally: Arc<UploadTally>) {
    // 16KB chunk + yield_now her chunkta: HTTP/2 backpressure minimize edilir.
    const UL_CHUNK: usize = 16_384;

    let client = match build_client() { Ok(c) => c, Err(_) => return };
    let counter_stream = counter.clone();
    let stop_stream    = stop.clone();
    let tally_stream   = tally.clone();

    let stream = futures::stream::unfold((), move |_| {
        let c = counter_stream.clone();
        let s = stop_stream.clone();
        let t = tally_stream.clone();
        async move {
            if s.load(Ordering::Relaxed) { return None; }
            tokio::task::yield_now().await;
            if s.load(Ordering::Relaxed) { return None; }
            let chunk = vec![0xAAu8; UL_CHUNK];
            c.fetch_add(UL_CHUNK as u64, Ordering::Relaxed);
            t.add_bytes(idx, UL_CHUNK as u64);
            Some((Ok::<_, std::convert::Infallible>(chunk), ()))
        }
    });

    let resp = client
        .post(SERVER_UL)
        .header("content-type", "application/octet-stream")
        .body(reqwest::Body::wrap_stream(stream))
        // Client timeout'u (60s) toplam istegi keser; adaptif onay beklemesine yer ac.
        .timeout(Duration::from_secs_f64(TOTAL_SECS) + Duration::from_secs(UPLOAD_CONFIRM_MAX_SECS + 5))
        .send()
        .await;
    // NetVeil /ul: 2xx + {"received": N} (N >= %95 uretilen byte) olmadan onay sayilmaz.
    tally.record_result(idx, upload_response(resp, true).await);
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

/// Faz bitisinden sonra worker task'lerin kapanmasi icin taninan ek sure.
pub const JOIN_GRACE_SECS: u64 = 2;

/// Bir olcum fazinin "gecerli veri akti" sayilmasi icin asgari Mbps.
pub const MIN_VALID_MBPS: f64 = 0.1;

/// Worker task'leri en fazla `grace` kadar bekler; kapanmayanlari abort eder.
/// Asili HTTP istekleri (60sn client timeout) faz penceresini asamaz.
pub async fn join_bounded(handles: Vec<tokio::task::JoinHandle<()>>, grace: Duration) {
    let deadline = tokio::time::Instant::now() + grace;
    for mut h in handles {
        if tokio::time::timeout_at(deadline, &mut h).await.is_err() {
            h.abort();
        }
    }
}

/// Upload yanit onayi icin olcum penceresinden SONRA taninan en uzun sure. Govde uretimi
/// durduktan sonra kuyrukta kalan byte'larin (yavas uplink, HTTP/2+TLS tamponlari) bosalmasi
/// saglikli hatlarda ~7 sn surer; sinir asilirsa yanit gelmemis = onaysiz.
pub const UPLOAD_CONFIRM_GRACE_SECS: u64 = 15;

/// Adaptif onay beklemesinin ust siniri (sn). Per-request HTTP timeout'u da buna gore uzatilir.
pub const UPLOAD_CONFIRM_MAX_SECS: u64 = 60;

/// Surucu tahmini uzerine eklenen guvenlik payi (sn).
const UPLOAD_CONFIRM_MARGIN_SECS: f64 = 3.0;

/// Adaptif onay beklemesi: kuyruktaki backlog kabaca sabit bayt oldugundan bosalma suresi
/// uplink hiziyla ters orantilidir. `drain = toplam_uretilen_bit / olculen_hiz - pencere`;
/// grace = clamp(drain + 3s, 15s, 60s). Hiz bilinmiyorsa (<=0/NaN) veya bayt yoksa sabit 15s.
pub fn upload_confirm_grace(total_bytes: u64, rate_mbps: f64, window_secs: f64) -> Duration {
    let min = UPLOAD_CONFIRM_GRACE_SECS as f64;
    let max = UPLOAD_CONFIRM_MAX_SECS as f64;
    if total_bytes == 0 || !rate_mbps.is_finite() || rate_mbps <= 0.0 || !window_secs.is_finite() {
        return Duration::from_secs_f64(min);
    }
    let drain = (total_bytes as f64 * 8.0) / (rate_mbps * 1_000_000.0) - window_secs;
    Duration::from_secs_f64((drain + UPLOAD_CONFIRM_MARGIN_SECS).clamp(min, max))
}

/// Faz penceresi (`deadline`) dolana KADAR bekler, sonra worker'lari `grace` ile birlestirir.
/// Temizlik grace'i olcum penceresi icinde calismamali: aktif workerlari erken abort etmez.
pub async fn join_after_deadline(
    deadline: std::time::Instant,
    handles: Vec<tokio::task::JoinHandle<()>>,
    grace: Duration,
) {
    tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await;
    join_bounded(handles, grace).await;
}

/// Stabil hata kodu: upload byte'lari sunucu 2xx onayiyla dogrulanamadi (UI'da cevrilir).
pub const ERR_UPLOAD_UNCONFIRMED: &str = "UPLOAD_UNCONFIRMED";

/// Bir upload worker'inin sunucu yaniti durumu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadOutcome {
    /// Yanit gelmedi (abort / zaman asimi / transport hatasi ayri `Failed`).
    Pending,
    Confirmed,
    Busy,
    Failed,
}

/// Upload worker'larinin worker-basi byte + sunucu onay sayaci. Govde uretilirken sayilan
/// byte'lar, ancak onemli miktarda byte ureten HER worker 2xx ile onaylandiysa gecerlidir.
pub struct UploadTally {
    slots: Vec<UploadSlot>,
}

struct UploadSlot {
    bytes: std::sync::atomic::AtomicU64,
    outcome: std::sync::atomic::AtomicU8,
}

impl UploadTally {
    pub fn new(workers: usize) -> Self {
        Self {
            slots: (0..workers)
                .map(|_| UploadSlot {
                    bytes: std::sync::atomic::AtomicU64::new(0),
                    outcome: std::sync::atomic::AtomicU8::new(0),
                })
                .collect(),
        }
    }

    /// Worker `idx` govde uretirken (paylasimli sayaca ek olarak) kendi byte'ini sayar.
    pub fn add_bytes(&self, idx: usize, n: u64) {
        if let Some(s) = self.slots.get(idx) {
            s.bytes.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Slot'un o ana kadar uretilen byte'i.
    pub fn bytes(&self, idx: usize) -> u64 {
        self.slots.get(idx).map_or(0, |s| s.bytes.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// `status`: None = transport hatasi / yanit yok.
    pub fn record(&self, idx: usize, status: Option<reqwest::StatusCode>) {
        let code = match status {
            Some(s) if s.is_success() => 1,
            Some(s) if s == reqwest::StatusCode::TOO_MANY_REQUESTS => 2,
            _ => 3,
        };
        if let Some(s) = self.slots.get(idx) {
            s.outcome.store(code, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Yanit isleme: `(durum, govde)` -> 2xx ve govde verildiyse NetVeil `{"received": N}`
    /// sozlesmesi dogrulanir (N >= %95 uretilen byte), yoksa yalnizca durum kodu. Transport hatasi
    /// (veya govde okunamadi) = onaysiz.
    pub fn record_result(&self, idx: usize, res: Result<(reqwest::StatusCode, Option<Vec<u8>>), reqwest::Error>) {
        match res {
            Ok((status, Some(body))) if status.is_success() => {
                let code = if received_confirms(&body, self.bytes(idx)) { 1 } else { 3 };
                if let Some(s) = self.slots.get(idx) {
                    s.outcome.store(code, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Ok((status, _)) => self.record(idx, Some(status)),
            Err(_) => self.record(idx, None),
        }
    }

    fn snapshot(&self) -> Vec<(u64, UploadOutcome)> {
        use std::sync::atomic::Ordering::Relaxed;
        self.slots
            .iter()
            .map(|s| {
                let o = match s.outcome.load(Relaxed) {
                    1 => UploadOutcome::Confirmed,
                    2 => UploadOutcome::Busy,
                    3 => UploadOutcome::Failed,
                    _ => UploadOutcome::Pending,
                };
                (s.bytes.load(Relaxed), o)
            })
            .collect()
    }

    pub fn verdict(&self) -> Result<(), String> {
        upload_validity(&self.snapshot())
    }

    /// Onemli byte ureten (>%5) hicbir istek `Pending` degilse true: beklemeye gerek kalmaz.
    pub fn all_significant_resolved(&self) -> bool {
        significant_all_resolved(&self.snapshot())
    }
}

/// Saf: NetVeil `/ul` govdesi `{"received": N}` ve N >= %95 * uretilen byte mi?
/// Govde cozulemezse / alan yoksa / eksik sayiysa false (2xx tek basina kanit degildir).
pub fn received_confirms(body: &[u8], generated: u64) -> bool {
    let received = serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("received").and_then(|r| r.as_u64()));
    match received {
        Some(n) => n.saturating_mul(100) >= generated.saturating_mul(95),
        None => false,
    }
}

/// Yaniti okur: `verify_received` ise 2xx govdesi de (NetVeil sozlesmesi icin) okunur.
pub async fn upload_response(
    resp: Result<reqwest::Response, reqwest::Error>,
    verify_received: bool,
) -> Result<(reqwest::StatusCode, Option<Vec<u8>>), reqwest::Error> {
    let resp = resp?;
    let status = resp.status();
    if verify_received && status.is_success() {
        let body = resp.bytes().await?;
        Ok((status, Some(body.to_vec())))
    } else {
        Ok((status, None))
    }
}

/// Upload worker'lari bekler: olcum penceresinden SONRA, en fazla `grace`. Onemli byte ureten
/// tum istekler cozuldugunde (veya tum task'ler bittiginde) beklemeyi birakir; kalanlar abort
/// edilir (yanit gelmemis kalir -> onaysiz).
pub async fn join_uploads(handles: Vec<tokio::task::JoinHandle<()>>, tally: &UploadTally, grace: Duration) {
    let deadline = tokio::time::Instant::now() + grace;
    loop {
        if handles.iter().all(|h| h.is_finished()) || tally.all_significant_resolved() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    for h in handles {
        if !h.is_finished() {
            h.abort();
        }
    }
}

/// Bir worker'in toplam byte'in bu oranindan (%5) fazlasini uretmesi "onemli" sayilir.
const SIGNIFICANT_SHARE_PCT: u64 = 5;

/// Saf karar (worker basina `(byte, durum)`):
///  - hic 2xx yoksa: 429 goren varsa SERVER_BUSY, yoksa onay hatasi;
///  - 2xx var ama onaylanmayan (failed+busy+pending) slotlarin TOPLAM byte'i uretilenin >%5'i
///    ise olcum dogrulanmamis byte icerir -> gecersiz (yalnizca 429 nedeniyle ise SERVER_BUSY);
///  - aksi halde gecerli.
pub fn upload_validity(workers: &[(u64, UploadOutcome)]) -> Result<(), String> {
    let any_ok = workers.iter().any(|(_, o)| *o == UploadOutcome::Confirmed);
    let any_busy = workers.iter().any(|(_, o)| *o == UploadOutcome::Busy);
    if !any_ok {
        return Err(if any_busy { "SERVER_BUSY".to_string() } else { ERR_UPLOAD_UNCONFIRMED.to_string() });
    }
    // AGREGA kural: onaylanmayan (Failed + Busy + Pending) slotlarin toplam byte'i, toplam
    // uretilenin %5'ini asarsa gecersiz. Slot basina esik yok: yeniden denemeler (worker basina
    // birden fazla slot) katkiyi parcalayip esigin altina indiremez.
    let total: u64 = workers.iter().map(|(b, _)| *b).sum();
    let unconfirmed: u64 = workers.iter().filter(|(_, o)| *o != UploadOutcome::Confirmed).map(|(b, _)| *b).sum();
    if !exceeds_share(unconfirmed, total) {
        return Ok(());
    }
    // Reddin nedeni yalnizca 429 ise SERVER_BUSY; baska onaysiz byte da esigi asiyorsa onay hatasi.
    let non_busy: u64 = workers.iter().filter(|(_, o)| !matches!(*o, UploadOutcome::Confirmed | UploadOutcome::Busy)).map(|(b, _)| *b).sum();
    if exceeds_share(non_busy, total) {
        Err(ERR_UPLOAD_UNCONFIRMED.to_string())
    } else {
        Err("SERVER_BUSY".to_string())
    }
}

/// `part` toplamin %5'inden buyuk mu?
fn exceeds_share(part: u64, total: u64) -> bool {
    part.saturating_mul(100) > total.saturating_mul(SIGNIFICANT_SHARE_PCT)
}

/// Saf: beklemeye devam etmek sonucu degistirebilir mi? (`upload_validity` ile ayni AGREGA %5 esigi)
pub fn significant_all_resolved(workers: &[(u64, UploadOutcome)]) -> bool {
    let total: u64 = workers.iter().map(|(b, _)| *b).sum();
    let pending: u64 = workers.iter().filter(|(_, o)| *o == UploadOutcome::Pending).map(|(b, _)| *b).sum();
    let settled_bad: u64 = workers.iter().filter(|(_, o)| matches!(*o, UploadOutcome::Failed | UploadOutcome::Busy)).map(|(b, _)| *b).sum();
    // Beklemeyi birak: bekleyen yok; ya da bekleyenler hangi sonucla biterse bitsin onaysiz pay
    // %5'i asamaz; ya da cozulmus onaysizlar zaten %5'i asti (sonuc kesin red).
    pending == 0
        || !exceeds_share(settled_bad + pending, total)
        || exceeds_share(settled_bad, total)
}

/// Stabil hata kodu on eki: faz boyunca veri akmadi. Ardindan faz belirteci gelir
/// (`NO_DATA:download`); UI kodu cevirir.
pub const ERR_NO_DATA_PREFIX: &str = "NO_DATA:";

/// Bir faz sonucunu dogrular: veri akmadiysa hata doner. 429 goruldugunde
/// (`busy`) bu "dusuk hiz" degil mevcut `SERVER_BUSY` hatasidir.
/// `what`: stabil faz belirteci (ornek `download`, `upload`, `bufferbloat_ul`).
pub fn check_phase_result(mbps: f64, busy: bool, what: &str) -> Result<(), String> {
    if mbps >= MIN_VALID_MBPS {
        Ok(())
    } else if busy {
        Err("SERVER_BUSY".to_string())
    } else {
        Err(format!("{}{}", ERR_NO_DATA_PREFIX, what))
    }
}

/// Sunucuda aktif test slotu musait mi diye hafif bir kontrol yapar.
/// Bir kullanici test ederken sunucu IP-kilidi nedeniyle baska IP'ye 429 doner.
/// true  = slot alindi/musait (test baslatilabilir)
/// false = sunucu mesgul (baska kullanici aktif test ediyor)
/// Sunucuya ulasilamazsa (ag hatasi) true doner — testin kendi hata yolu devralir.
pub async fn probe_slot(server: Option<&str>) -> bool {
    let url = match server {
        Some("netveil") => "https://api.netveil.net/dl?bytes=1",
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant as StdInstant;

    #[test]
    fn upload_validity_en_az_bir_2xx_gerekir() {
        use UploadOutcome::*;
        assert!(upload_validity(&[(100, Confirmed), (0, Failed)]).is_ok());
        assert_eq!(upload_validity(&[(100, Busy), (50, Failed)]).unwrap_err(), "SERVER_BUSY");
        assert_eq!(upload_validity(&[(100, Failed), (100, Pending)]).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // Hic yanit gelmeyen (abort edilen) workerlar da onay sayilmaz
        assert!(upload_validity(&[]).is_err());
        assert!(upload_validity(&[(0, Pending)]).is_err());
    }

    // H2: tek bir 2xx, onaylanmamis worker'in (>%5) byte'larini aklamaz.
    #[test]
    fn upload_validity_onaysiz_onemli_worker_olcumu_gecersiz_kilar() {
        use UploadOutcome::*;
        // 5 onayli (100'er) + 1 onaysiz (100 = %16.7) -> gecersiz
        let mut w = vec![(100u64, Confirmed); 5];
        w.push((100, Failed));
        assert_eq!(upload_validity(&w).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // onaysiz yanit hic gelmeyen (Pending) onemli worker da gecersiz kilar
        let mut w = vec![(100u64, Confirmed); 5];
        w.push((100, Pending));
        assert_eq!(upload_validity(&w).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // onaysiz worker 429 ise SERVER_BUSY semantigi korunur
        let mut w = vec![(100u64, Confirmed); 5];
        w.push((100, Busy));
        assert_eq!(upload_validity(&w).unwrap_err(), "SERVER_BUSY");
        // 429 + baska hata karisikken onay hatasi
        let mut w = vec![(100u64, Confirmed); 4];
        w.push((100, Busy));
        w.push((100, Failed));
        assert_eq!(upload_validity(&w).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // onaysiz worker toplamin <=%5'i ise (ihmal edilebilir) gecerli
        let mut w = vec![(1000u64, Confirmed); 5];
        w.push((250, Failed)); // 250/5250 = %4.8
        assert!(upload_validity(&w).is_ok());
        // tam %5 sinirinda (esik 'buyuktur') gecerli
        assert!(upload_validity(&[(95, Confirmed), (5, Failed)]).is_ok());
        assert!(upload_validity(&[(94, Confirmed), (6, Failed)]).is_err());
    }

    #[test]
    fn upload_tally_durum_kodlari() {
        use reqwest::StatusCode;
        let t = UploadTally::new(3);
        t.add_bytes(0, 100); t.add_bytes(1, 100); t.add_bytes(2, 100);
        t.record(0, Some(StatusCode::INTERNAL_SERVER_ERROR));
        t.record(1, None);
        assert!(t.verdict().is_err());
        t.record(1, Some(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(t.verdict().unwrap_err(), "SERVER_BUSY");
        t.record(2, Some(StatusCode::OK));
        // 2xx var ama worker 0/1 onemli byte uretti ve onaylanmadi -> gecersiz
        assert!(t.verdict().is_err());
        t.record(0, Some(StatusCode::OK));
        t.record(1, Some(StatusCode::OK));
        assert!(t.verdict().is_ok());
    }

    // C1: temizlik grace'i olcum penceresi icinde calismamali. Worker pencerenin sonuna
    // kadar (deadline'dan once) calisir; grace 100ms < worker suresi olsa bile ABORT edilmemeli.
    #[tokio::test]
    async fn join_after_deadline_aktif_workeri_erken_abort_etmez() {
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let d2 = done.clone();
        let deadline = StdInstant::now() + Duration::from_millis(300);
        let h = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await; // deadline'dan once biter
            d2.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        join_after_deadline(deadline, vec![h], Duration::from_millis(100)).await;
        assert!(done.load(std::sync::atomic::Ordering::Relaxed), "worker erken abort edildi");
        assert!(StdInstant::now() >= deadline);
    }

    // Asili worker deadline + grace sonrasi abort edilir (faz sonsuza kadar asilmaz).
    #[tokio::test]
    async fn join_after_deadline_asili_workeri_grace_sonrasi_keser() {
        let deadline = StdInstant::now() + Duration::from_millis(100);
        let h = tokio::spawn(async { tokio::time::sleep(Duration::from_secs(30)).await; });
        let t0 = StdInstant::now();
        join_after_deadline(deadline, vec![h], Duration::from_millis(150)).await;
        assert!(t0.elapsed() < Duration::from_secs(2), "asili worker abort edilmedi: {:?}", t0.elapsed());
    }

    #[tokio::test]
    async fn join_bounded_asili_workeri_keser() {
        let h = tokio::spawn(async { tokio::time::sleep(Duration::from_secs(30)).await; });
        let t0 = StdInstant::now();
        join_bounded(vec![h], Duration::from_millis(100)).await;
        assert!(t0.elapsed() < Duration::from_secs(2));
    }

    // U2: NetVeil /ul sozlesmesi: 2xx + received >= %95 uretilen byte.
    #[test]
    fn received_confirms_sozlesmesi() {
        assert!(received_confirms(br#"{"received":1000}"#, 1000));
        assert!(received_confirms(br#"{"received":950}"#, 1000)); // tam %95 sinirda gecerli
        assert!(!received_confirms(br#"{"received":949}"#, 1000)); // kisa sayim
        assert!(!received_confirms(br#"{"received":0}"#, 1000));
        assert!(!received_confirms(b"not json", 1000)); // cozulemeyen govde
        assert!(!received_confirms(b"", 1000));
        assert!(!received_confirms(br#"{"other":1000}"#, 1000)); // alan yok
        assert!(!received_confirms(br#"{"received":"1000"}"#, 1000)); // sayi degil
        assert!(received_confirms(br#"{"received":5}"#, 0));
    }

    #[test]
    fn tally_200_ama_kisa_received_reddedilir() {
        use reqwest::StatusCode;
        let t = UploadTally::new(2);
        t.add_bytes(0, 1000);
        t.add_bytes(1, 1000);
        t.record_result(0, Ok((StatusCode::OK, Some(br#"{"received":1000}"#.to_vec()))));
        // 200 ama received kisa -> Failed (>%5 katkida bulunan onaysiz worker)
        t.record_result(1, Ok((StatusCode::OK, Some(br#"{"received":400}"#.to_vec()))));
        assert_eq!(t.verdict().unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // cozulemeyen govde de reddedilir
        t.record_result(1, Ok((StatusCode::OK, Some(b"<html>".to_vec()))));
        assert_eq!(t.verdict().unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // tam sayim -> gecerli
        t.record_result(1, Ok((StatusCode::OK, Some(br#"{"received":1000}"#.to_vec()))));
        assert!(t.verdict().is_ok());
        // govdesiz 2xx (Cloudflare yolu) yalnizca durum koduyla onaylanir
        let cf = UploadTally::new(1);
        cf.add_bytes(0, 10);
        cf.record_result(0, Ok((StatusCode::OK, None)));
        assert!(cf.verdict().is_ok());
        // non-2xx / 429 semantigi korunur
        let b = UploadTally::new(1);
        b.add_bytes(0, 10);
        b.record_result(0, Ok((StatusCode::TOO_MANY_REQUESTS, None)));
        assert_eq!(b.verdict().unwrap_err(), "SERVER_BUSY");
    }

    // Adaptif onay beklemesi: 12 Mbps -> ~15s (min), 3 Mbps -> uzar (<=60s), bilinmeyen -> 15s.
    #[test]
    fn upload_confirm_grace_adaptif() {
        let secs = |b: u64, r: f64, w: f64| upload_confirm_grace(b, r, w).as_secs_f64();
        // 24 MB, 12 Mbps, 10s pencere: drain = 16 - 10 = 6 (+3 = 9) -> min 15
        assert_eq!(secs(24_000_000, 12.0, 10.0), 15.0);
        // ayni bayt, 3 Mbps: 64 - 10 + 3 = 57 (15..=60 araliginda)
        let slow = secs(24_000_000, 3.0, 10.0);
        assert!((slow - 57.0).abs() < 0.01, "{slow}");
        assert!(slow > 15.0 && slow <= 60.0);
        // cok yavas hat -> 60s ust sinir
        assert_eq!(secs(24_000_000, 0.5, 10.0), 60.0);
        // bilinmeyen / gecersiz hiz veya bayt -> sabit 15s
        assert_eq!(secs(24_000_000, 0.0, 10.0), 15.0);
        assert_eq!(secs(24_000_000, -1.0, 10.0), 15.0);
        assert_eq!(secs(24_000_000, f64::NAN, 10.0), 15.0);
        assert_eq!(secs(0, 12.0, 10.0), 15.0);
        // hizli hat -> min'in altina inmez
        assert_eq!(secs(1_000_000_000, 900.0, 12.0), 15.0);
    }

    // M1: yeniden deneme slotlari katkiyi parcalayamaz; esik AGREGA onaysiz paya uygulanir.
    #[test]
    fn upload_validity_parcalanmis_slotlar_agrega_esik() {
        use UploadOutcome::*;
        // 8 worker x 4 slot = 32 slot, her biri 100 byte (%3.125). Yalniz 1 slot onayli.
        let mut w = vec![(100u64, Failed); 16];
        w.extend(vec![(100u64, Pending); 15]);
        w.push((100, Confirmed));
        assert_eq!(upload_validity(&w).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        // tamami onayli -> gecerli
        assert!(upload_validity(&vec![(100u64, Confirmed); 32]).is_ok());
        // 32 slottan 2'si onaysiz (%6.25 > %5) -> gecersiz; 1'i (%3.125) -> gecerli
        let mut two = vec![(100u64, Confirmed); 30];
        two.extend(vec![(100u64, Failed); 2]);
        assert_eq!(upload_validity(&two).unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
        let mut one = vec![(100u64, Confirmed); 31];
        one.push((100, Failed));
        assert!(upload_validity(&one).is_ok());
        // 429 kaynakli parcali red -> SERVER_BUSY
        let mut busy = vec![(100u64, Confirmed); 29];
        busy.extend(vec![(100u64, Busy); 3]);
        assert_eq!(upload_validity(&busy).unwrap_err(), "SERVER_BUSY");
    }

    // M1: erken cikis yalniz bekleyen byte sonucu degistiremeyecekse.
    #[test]
    fn significant_all_resolved_agrega_erken_cikis() {
        use UploadOutcome::*;
        // yalniz %3.125 bekliyor (hepsi basarisiz olsa bile <=%5) -> hemen birak
        let mut w = vec![(100u64, Confirmed); 31];
        w.push((100, Pending));
        assert!(significant_all_resolved(&w));
        // parcalanmis bekleyenler toplamda %5'i asiyor -> beklemeye devam
        let mut many = vec![(100u64, Confirmed); 29];
        many.extend(vec![(100u64, Pending); 3]);
        assert!(!significant_all_resolved(&many));
        // cozulmus onaysizlar zaten %5'i astiysa sonuc kesin red -> birak
        let mut bad = vec![(100u64, Confirmed); 29];
        bad.extend(vec![(100u64, Failed); 2]);
        bad.push((100, Pending));
        assert!(significant_all_resolved(&bad));
        assert!(upload_validity(&bad).is_err());
    }

    #[test]
    fn significant_all_resolved_esigi() {
        use UploadOutcome::*;
        assert!(significant_all_resolved(&[(100, Confirmed), (100, Failed)]));
        assert!(!significant_all_resolved(&[(100, Confirmed), (100, Pending)]));
        // toplamin <=%5'i olan bekleyen istek beklemeyi tutmaz
        assert!(significant_all_resolved(&[(1000, Confirmed), (50, Pending)]));
        assert!(significant_all_resolved(&[(0, Pending), (0, Pending)]));
    }

    // U1/U3: gecikmeli ama tam onay gecerli; erken biter; sinir asilirsa onaysiz.
    #[tokio::test]
    async fn gecikmeli_onay_gecerli_ve_erken_birakir() {
        use reqwest::StatusCode;
        let tally = std::sync::Arc::new(UploadTally::new(6));
        let mut handles = Vec::new();
        for i in 0..6 {
            tally.add_bytes(i, 1000);
            let t = tally.clone();
            handles.push(tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(300 + 20 * i as u64)).await;
                t.record_result(i, Ok((StatusCode::OK, Some(br#"{"received":1000}"#.to_vec()))));
            }));
        }
        let t0 = StdInstant::now();
        join_uploads(handles, &tally, Duration::from_secs(10)).await;
        assert!(t0.elapsed() >= Duration::from_millis(300), "onay beklenmedi");
        assert!(t0.elapsed() < Duration::from_secs(3), "tum istekler cozulunce beklenmeye devam edildi");
        assert!(tally.verdict().is_ok());
    }

    #[tokio::test]
    async fn sinir_asilirsa_bekleyen_onemli_istek_onaysiz() {
        use reqwest::StatusCode;
        let tally = std::sync::Arc::new(UploadTally::new(2));
        tally.add_bytes(0, 1000);
        tally.add_bytes(1, 1000);
        tally.record_result(0, Ok((StatusCode::OK, Some(br#"{"received":1000}"#.to_vec()))));
        let h = tokio::spawn(async { tokio::time::sleep(Duration::from_secs(30)).await; });
        let t0 = StdInstant::now();
        join_uploads(vec![h], &tally, Duration::from_millis(300)).await;
        assert!(t0.elapsed() < Duration::from_secs(2));
        assert_eq!(tally.verdict().unwrap_err(), ERR_UPLOAD_UNCONFIRMED);
    }
}

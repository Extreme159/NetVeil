//! Saf ICMP ping aracı — ayrı "Ping" sekmesi için.
//! Traceroute mimarisini izler: command → spawn → stream (event emit).
//! IPv4/IPv6, preset/manuel host, sabit sayı veya sürekli mod, reverse DNS.

use serde::Serialize;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use surge_ping::{Client, Config, PingIdentifier, PingSequence, ICMP};
use tauri::{AppHandle, Emitter};

const EVENT_REPLY: &str = "ping:reply";
const EVENT_DONE: &str = "ping:done";
const EVENT_ERROR: &str = "ping:error";

const PING_TIMEOUT_MS: u64 = 2000;
const PING_INTERVAL_MS: u64 = 1000; // ping'ler arası bekleme (1 sn — klasik ping)
const MAX_COUNT: u32 = 10_000; // sürekli mod üst sınırı (güvenlik)

// Sürekli mod için durdurma bayrağı (aynı anda tek ping testi varsayımı).
// Run generation: every start and every stop bumps it. A ping task is "current" only while
// its own generation equals this value - a stopped/superseded task exits and emits nothing, so
// it can neither revive after a quick restart nor leak events into the new run's listeners.
static RUN_GEN: AtomicU64 = AtomicU64::new(0);

fn is_current(run_gen: u64) -> bool {
    RUN_GEN.load(Ordering::SeqCst) == run_gen
}

#[derive(Serialize, Clone)]
struct PingReply {
    seq: u32,
    /// Yanıt geldiyse RTT (ms), timeout ise None.
    rtt_ms: Option<f64>,
    /// Çözülen hedef IP (her replyde aynı, UI başlık için).
    target_ip: String,
    /// Çalışan istatistikler (her replyde güncel).
    sent: u32,
    received: u32,
    loss_pct: f64,
    min_ms: f64,
    avg_ms: f64,
    max_ms: f64,
    jitter_ms: f64, // ardışık RTT farklarının ortalaması (mutlak)
}

#[derive(Serialize, Clone)]
struct PingDone {
    target_ip: String,
    hostname: Option<String>,
    sent: u32,
    received: u32,
    loss_pct: f64,
    min_ms: f64,
    avg_ms: f64,
    max_ms: f64,
    jitter_ms: f64,
}

#[tauri::command]
pub async fn run_ping_test(
    app: AppHandle,
    host: String,
    ip_version: String,      // "4" | "6" | "auto"
    count: Option<u32>,      // None veya 0 → sürekli (stop'a kadar)
    resolve_hostname: bool,
) -> Result<(), String> {
    // Önceki testi durdur (tek aktif test).
    let run_gen = RUN_GEN.fetch_add(1, Ordering::SeqCst) + 1;

    let app2 = app.clone();
    tokio::task::spawn(async move {
        if let Err(e) = stream_ping(app2, host, ip_version, count, resolve_hostname, run_gen).await {
            if is_current(run_gen) {
                let _ = app.emit(EVENT_ERROR, e);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_ping_test() -> Result<(), String> {
    RUN_GEN.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

async fn stream_ping(
    app: AppHandle,
    host: String,
    ip_version: String,
    count: Option<u32>,
    resolve_hostname: bool,
    run_gen: u64,
) -> Result<(), String> {
    let target = resolve_target(&host, &ip_version).await?;
    let target_ip = target.to_string();

    // Reverse DNS (opsiyonel) — paralel başlat, done'da kullanılır.
    let hostname = if resolve_hostname {
        reverse_dns(&target_ip).await
    } else {
        None
    };

    let config = Config::builder()
        .kind(if target.is_ipv6() { ICMP::V6 } else { ICMP::V4 })
        .build();
    let client = Client::new(&config).map_err(|e| format!("ICMP istemci hatası: {e}"))?;
    let mut pinger = client.pinger(target, PingIdentifier(crate::rand_id())).await;
    pinger.timeout(Duration::from_millis(PING_TIMEOUT_MS));

    // count: Some(n>0) → n kez; None veya Some(0) → sürekli (MAX_COUNT sınırlı).
    let total = match count {
        Some(n) if n > 0 => n.min(MAX_COUNT),
        _ => MAX_COUNT,
    };

    // Stopped or superseded while DNS / reverse DNS / socket setup was in flight: do not start.
    if !is_current(run_gen) {
        return Ok(());
    }

    let mut sent = 0u32;
    let mut received = 0u32;
    let mut rtts: Vec<f64> = Vec::new();
    let mut last_rtt: Option<f64> = None;
    let mut jitter_acc = 0.0f64;
    let mut jitter_n = 0u32;

    for seq in 0..total {
        if !is_current(run_gen) {
            return Ok(());
        }

        sent += 1;
        let rtt_ms = match pinger
            .ping(PingSequence((seq % 65535) as u16), &[0u8; 32])
            .await
        {
            Ok((_, dur)) => {
                let ms = dur.as_secs_f64() * 1000.0;
                received += 1;
                rtts.push(ms);
                // Jitter: ardışık RTT'ler arası mutlak fark ortalaması.
                if let Some(prev) = last_rtt {
                    jitter_acc += (ms - prev).abs();
                    jitter_n += 1;
                }
                last_rtt = Some(ms);
                Some(ms)
            }
            Err(_) => None, // timeout / unreachable — kayıp say
        };

        let (min_ms, avg_ms, max_ms) = stats(&rtts);
        let loss_pct = if sent > 0 {
            ((sent - received) as f64 / sent as f64) * 100.0
        } else {
            0.0
        };
        let jitter_ms = if jitter_n > 0 { jitter_acc / jitter_n as f64 } else { 0.0 };

        // Stopped while the ping was in flight: emit nothing for a stale run.
        if !is_current(run_gen) {
            return Ok(());
        }

        let _ = app.emit(
            EVENT_REPLY,
            PingReply {
                seq: seq + 1,
                rtt_ms,
                target_ip: target_ip.clone(),
                sent,
                received,
                loss_pct: round2(loss_pct),
                min_ms: round2(min_ms),
                avg_ms: round2(avg_ms),
                max_ms: round2(max_ms),
                jitter_ms: round2(jitter_ms),
            },
        );

        // Son ping değilse aralık kadar bekle.
        if seq + 1 < total && is_current(run_gen) {
            tokio::time::sleep(Duration::from_millis(PING_INTERVAL_MS)).await;
        }
    }

    // The sleep above may have been interrupted by Stop/restart: a stale run emits no DONE.
    if !is_current(run_gen) {
        return Ok(());
    }

    let (min_ms, avg_ms, max_ms) = stats(&rtts);
    let loss_pct = if sent > 0 {
        ((sent - received) as f64 / sent as f64) * 100.0
    } else {
        0.0
    };
    let jitter_ms = if jitter_n > 0 { jitter_acc / jitter_n as f64 } else { 0.0 };

    let _ = app.emit(
        EVENT_DONE,
        PingDone {
            target_ip,
            hostname,
            sent,
            received,
            loss_pct: round2(loss_pct),
            min_ms: round2(min_ms),
            avg_ms: round2(avg_ms),
            max_ms: round2(max_ms),
            jitter_ms: round2(jitter_ms),
        },
    );
    Ok(())
}

/// RTT vektöründen (min, avg, max). Boşsa hepsi 0.
fn stats(rtts: &[f64]) -> (f64, f64, f64) {
    if rtts.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let min = rtts.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let avg = rtts.iter().sum::<f64>() / rtts.len() as f64;
    (min, avg, max)
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

/// Host'u IP'ye çözer. ip_version: "4" zorla IPv4, "6" zorla IPv6, diğer → IPv4 öncelikli auto.
async fn resolve_target(host: &str, ip_version: &str) -> Result<IpAddr, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ip);
    }
    let host_owned = host.to_string();
    let want = ip_version.to_string();

    tokio::task::spawn_blocking(move || {
        let addrs: Vec<IpAddr> = dns_lookup::lookup_host(&host_owned)
            .map_err(|e| format!("DNS çözümlenemedi ({host_owned}): {e}"))?;
        let pick = match want.as_str() {
            "6" => addrs.iter().find(|a| a.is_ipv6()).copied(),
            "4" => addrs.iter().find(|a| a.is_ipv4()).copied(),
            _ => addrs
                .iter()
                .find(|a| a.is_ipv4())
                .copied()
                .or_else(|| addrs.first().copied()),
        };
        pick.ok_or_else(|| format!("'{host_owned}' için uygun IP bulunamadı"))
    })
    .await
    .map_err(|e| format!("DNS görevi başarısız: {e}"))?
}

/// Reverse DNS (PTR) çözümü — bloklayıcı, blocking thread'de.
async fn reverse_dns(ip: &str) -> Option<String> {
    let Ok(addr) = ip.parse::<IpAddr>() else { return None };
    let ip_str = ip.to_string();
    tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&addr).ok())
        .await
        .ok()
        .flatten()
        .filter(|name| name != &ip_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Start -> Stop -> immediate Start: neither earlier run may be current again.
    #[test]
    fn stale_run_is_not_current_after_stop_and_restart() {
        let first = RUN_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(is_current(first));
        RUN_GEN.fetch_add(1, Ordering::SeqCst); // stop_ping_test
        assert!(!is_current(first), "stopped run must be stale (e.g. still resolving DNS)");
        let second = RUN_GEN.fetch_add(1, Ordering::SeqCst) + 1; // restart
        assert!(!is_current(first), "stopped run must not revive on restart");
        assert!(is_current(second));
    }
}

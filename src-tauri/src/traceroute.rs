// Hop-by-hop traceroute — streaming.
//
// Önceki sürüm sistem `tracert`/`traceroute` binary'sine bağımlıydı ve stdout
// metnini parse ediyordu; bu yöntem dile bağımlıydı (TR/EN Windows farklı çıktı).
//
// Bu sürüm Windows'ta IP Helper API'sini kullanır:
//   - IPv4: `IcmpSendEcho`, IPv6: `Icmp6SendEcho2`.
//   - `IP_OPTION_INFORMATION.Ttl` ile her prob'un TTL'i (IPv6'da hop limit) ayarlanır.
//   - Ara router TTL=0'a düşünce ICMP TimeExceeded döndürür; API bunu
//     IP_TTL_EXPIRED_TRANSIT (11013) statüsü ve ara router IP'si ile raporlar.
//   - Hedefe ulaşıldığında IP_SUCCESS (0) döner → trace tamamlanır.
//
// NEDEN raw socket DEĞİL: Windows'ta SOCK_RAW ICMP yönetici (admin) yetkisi ister.
// NetVeil normal kullanıcıyla çalışan bir masaüstü uygulaması olduğundan raw socket
// kullanıcıların çoğunda traceroute'u kırardı. `IcmpSendEcho` admin gerektirmez ve
// tam da bu iş (TTL tabanlı hop keşfi) için tasarlanmıştır — dilden ve harici
// binary'den bağımsızdır. Bu, raporun "raw ICMP / harici tracert bağımlılığını kaldır"
// hedefini doğru ve güvenli biçimde karşılar.
//
// Her hop için PROBES_PER_HOP kadar deneme yapılır; RTT olarak en düşük (min) alınır
// (ortalama en iyi+en kötü hop'u maskeler; min daha temsilidir — rapor önerisi).
// Ülke tespiti hop emit edildikten SONRA paralel çekilir (sıralı bekleme yok).

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr};
#[cfg(windows)]
use std::net::Ipv6Addr;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Maksimum hop sayısı (TTL üst sınırı).
const MAX_HOPS: u8 = 30;
/// Hop başına gönderilen probe sayısı (RTT için min alınır).
const PROBES_PER_HOP: u8 = 3;
/// Tek probe için timeout (ms).
const PROBE_TIMEOUT_MS: u32 = 800;
/// Hedef yanıt vermiyorsa sonsuza kadar gitmeyi önleyen ardışık tam-timeout limiti.
const MAX_CONSECUTIVE_TIMEOUTS: u8 = 5;
/// ICMP payload öneki — her probe'a rastgele+sayaç nonce eklenir (bkz. `make_probe_payload`).
const PROBE_PAYLOAD_PREFIX: &[u8] = b"netveil-traceroute";
/// Aynı anda en fazla bu kadar reverse DNS (PTR) sorgusu çalışır.
const PTR_CONCURRENCY: usize = 4;
/// Tek bir PTR sorgusunu beklemek için üst sınır (sonra hostname'siz bırakılır).
const PTR_LOOKUP_TIMEOUT: Duration = Duration::from_secs(2);
/// PTR eşzamanlılık sınırı SÜREÇ GENELİDİR: zaman aşımı/Stop sonrası terk edilen bloklayıcı sorgular
/// izni tutmaya devam eder; yeni koşu (generation) taze izinlerle sınırı aşamasın.
static PTR_SLOTS: std::sync::OnceLock<Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();

fn ptr_slots() -> Arc<tokio::sync::Semaphore> {
    PTR_SLOTS
        .get_or_init(|| Arc::new(tokio::sync::Semaphore::new(PTR_CONCURRENCY)))
        .clone()
}
/// Probe başına benzersiz nonce sayacı (süreç boyunca artar).
static PROBE_SEQ: AtomicU32 = AtomicU32::new(0);

// IP Helper status kodları (ipexport.h — windows crate bunları sabit olarak yaymıyor).
const IP_SUCCESS: u32 = 0;
const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HopResult {
    pub hop: u8,
    pub ip: String,
    pub hostname: Option<String>,
    pub rtt_ms: Option<f64>, // en düşük RTT (min)
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub timed_out: bool,
    pub country: Option<String>,      // "TR", "DE" gibi ISO 2 harf
    pub country_name: Option<String>, // "Turkey", "Germany"
    /// Run this hop belongs to (echo of the id passed to run_traceroute) - the UI drops stale runs.
    #[serde(default)]
    pub run_id: u64,
}

/// Hop'un GeoIP bilgisi paralel çekildikten sonra gönderilen güncelleme.
#[derive(Debug, Serialize, Clone)]
struct HopGeo {
    hop: u8,
    country: Option<String>,
    country_name: Option<String>,
    run_id: u64,
}

/// Hop'un hostname'i (reverse DNS) hop yayınlandıktan SONRA ayrı gönderilir.
#[derive(Debug, Serialize, Clone)]
struct HopHost {
    hop: u8,
    hostname: String,
    run_id: u64,
}

/// `traceroute:done` payload (was null; now carries the run id).
#[derive(Debug, Serialize, Clone)]
struct TraceDone {
    run_id: u64,
}

/// `traceroute:error` payload (was a bare string; now carries the run id + message).
#[derive(Debug, Serialize, Clone)]
struct TraceError {
    run_id: u64,
    message: String,
}

/// Upper bound for finishing GeoIP + hostname enrichment before `traceroute:done` is emitted.
const GEO_ENRICH_TIMEOUT: Duration = Duration::from_secs(4);

/// Hop ülke sorgusu: NetVeil sunucusu (IP saklanmaz/loglanmaz). Sunucu limiti 30 istek/10 sn;
/// bir trace en fazla MAX_HOPS (30) sorgu üretir ve yalnızca halka açık hop'lar sorgulanır.
const GEO_URL_BASE: &str = "https://api.netveil.net/geo/";

/// Run generation: every start and every stop bumps it. A trace task is "current" only while
/// its own token equals this value; stale tasks stop probing and emit nothing.
static TRACE_GEN: AtomicU64 = AtomicU64::new(0);

fn is_current(token: u64) -> bool {
    TRACE_GEN.load(Ordering::SeqCst) == token
}

/// Tek prob'un sonucu.
enum ProbeOutcome {
    /// Ara router veya hedef yanıt verdi: (kaynak IP, rtt_ms, hedefe_ulaşıldı_mı)
    Reply { ip: IpAddr, rtt_ms: f64, reached: bool },
    /// Bu prob zaman aşımına uğradı.
    Timeout,
}

/// Tek hop tamamlandığında emit edilen event.
const EVENT_HOP: &str = "traceroute:hop";
/// Bir hop'un ülke bilgisi geldiğinde emit edilen event (paralel GeoIP).
const EVENT_HOP_GEO: &str = "traceroute:hop_geo";
/// Bir hop'un hostname'i (reverse DNS) geldiğinde emit edilen event (hop'tan sonra, ayrı).
const EVENT_HOP_HOST: &str = "traceroute:hop_host";
/// Trace tamamlandığında emit edilen event (payload: null).
const EVENT_DONE: &str = "traceroute:done";
/// Hata oluştuğunda emit edilen event (payload: string).
const EVENT_ERROR: &str = "traceroute:error";

#[tauri::command]
pub async fn run_traceroute(
    app: AppHandle,
    host: String,
    ip_version: String, // "4" | "6" | "auto"
    resolve_hostnames: bool,
    run_id: Option<u64>, // UI-chosen id echoed in every event (omitted -> backend token)
) -> Result<(), String> {
    // Starting a run invalidates any previous one (its task exits at the next check).
    let token = TRACE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    let run_id = run_id.unwrap_or(token);
    let app2 = app.clone();
    tokio::task::spawn(async move {
        if let Err(e) = stream_trace(app2, host, ip_version, resolve_hostnames, token, run_id).await {
            if is_current(token) {
                let _ = app.emit(EVENT_ERROR, TraceError { run_id, message: e });
            }
        }
    });
    Ok(())
}

/// Cancels the running trace (if any): its task stops probing and emits no further events.
#[tauri::command]
pub async fn stop_traceroute() -> Result<(), String> {
    TRACE_GEN.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

async fn stream_trace(
    app: AppHandle,
    host: String,
    ip_version: String,
    resolve_hostnames: bool,
    token: u64,
    run_id: u64,
) -> Result<(), String> {
    // Hedefi çöz (IPv4 öncelikli — oyun/ping senaryosu için doğru tercih).
    let target = resolve_target(&host, &ip_version).await?;

    // HTTP client — ülke sorguları için (paralel görevlerde paylaşılır).
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("HTTP client hatası: {e}"))?;

    // Stopped/superseded while resolving the target: do nothing.
    if !is_current(token) {
        return Ok(());
    }

    let mut consecutive_timeouts: u8 = 0;
    // GeoIP lookups run in parallel with probing; `done` waits for them (time-bounded).
    let mut geo_tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    // Reverse DNS is enrichment too: it never blocks hop publication or the next TTL.
    let mut host_tasks: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();
    let ptr_slots = ptr_slots();

    for ttl in 1..=MAX_HOPS {
        if !is_current(token) {
            return Ok(());
        }
        let outcomes = probe_hop(target, ttl, token).await;
        if !is_current(token) {
            return Ok(());
        }
        let mut hop = build_hop(ttl, &outcomes);
        hop.run_id = run_id;

        let reached = outcomes.iter().any(|o| matches!(o, ProbeOutcome::Reply { reached: true, .. }));

        if hop.timed_out {
            consecutive_timeouts += 1;
        } else {
            consecutive_timeouts = 0;
        }

        let _ = app.emit(EVENT_HOP, &hop);

        if !hop.timed_out {
            spawn_geo_lookup(&mut geo_tasks, app.clone(), http.clone(), ttl, hop.ip.clone(), token, run_id);
            if resolve_hostnames {
                spawn_hostname_lookup(
                    &mut host_tasks,
                    app.clone(),
                    ptr_slots.clone(),
                    ttl,
                    hop.ip.clone(),
                    token,
                    run_id,
                );
            }
        }

        if reached {
            break;
        }
        if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
            break;
        }
    }

    // Let the outstanding GeoIP lookups finish BEFORE signalling done (the UI drops its
    // listeners on done, which used to lose late country labels). Bounded: each lookup has its own
    // 3 s HTTP timeout and they run in parallel; whatever is still pending is aborted.
    // Hostname (PTR) lookups share the same bound; aborted ones never publish.
    let _ = tokio::time::timeout(GEO_ENRICH_TIMEOUT, async {
        while geo_tasks.join_next().await.is_some() {}
        while host_tasks.join_next().await.is_some() {}
    })
    .await;
    geo_tasks.abort_all();
    host_tasks.abort_all();

    if is_current(token) {
        let _ = app.emit(EVENT_DONE, TraceDone { run_id });
    }
    Ok(())
}

/// Belirli bir TTL için PROBES_PER_HOP prob gönderir.
/// Her prob bloklayıcı IP Helper çağrısı olduğundan blocking thread'de koşar.
async fn probe_hop(target: IpAddr, ttl: u8, token: u64) -> Vec<ProbeOutcome> {
    let mut outcomes = Vec::with_capacity(PROBES_PER_HOP as usize);
    for _ in 0..PROBES_PER_HOP {
        if !is_current(token) {
            break; // cancelled: caller re-checks and discards the partial result
        }
        let outcome = tokio::task::spawn_blocking(move || icmp_probe(target, ttl))
            .await
            .unwrap_or(ProbeOutcome::Timeout);
        outcomes.push(outcome);
    }
    outcomes
}

/// Tek bir ICMP echo prob'u — verilen TTL ile. Bloklayıcı.
fn icmp_probe(target: IpAddr, ttl: u8) -> ProbeOutcome {
    // Benzersiz payload: eski/yanlış eşleşen bir yanıt bu prob'a ait sayılmasın.
    let payload = make_probe_payload();
    match target {
        IpAddr::V4(v4) => icmp_probe_v4(v4, ttl, &payload),
        IpAddr::V6(v6) => icmp_probe_v6(v6, ttl, &payload),
    }
}

/// Prob'a özgü payload: sabit önek + rastgele u32 + süreç sayacı u32 (26 bayt).
fn make_probe_payload() -> Vec<u8> {
    let mut p = Vec::with_capacity(PROBE_PAYLOAD_PREFIX.len() + 8);
    p.extend_from_slice(PROBE_PAYLOAD_PREFIX);
    p.extend_from_slice(&rand::random::<u32>().to_be_bytes());
    p.extend_from_slice(&PROBE_SEQ.fetch_add(1, Ordering::Relaxed).to_be_bytes());
    p
}

/// Yanıt buffer'ı içindeki echo verisini güvenli dilimler; buffer dışına taşan adres/uzunluk → None.
#[cfg_attr(not(windows), allow(dead_code))]
fn echo_data_slice(buf: &[u8], data_addr: usize, data_len: usize) -> Option<&[u8]> {
    let offset = data_addr.checked_sub(buf.as_ptr() as usize)?;
    buf.get(offset..offset.checked_add(data_len)?)
}

/// IPv4 prob — `IcmpSendEcho` (admin gerektirmez).
#[cfg(windows)]
fn icmp_probe_v4(target: Ipv4Addr, ttl: u8, payload: &[u8]) -> ProbeOutcome {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
    };

    // IcmpSendEcho adresi network byte order u32 bekler:
    // octets [a,b,c,d] → u32 LE = a | b<<8 | c<<16 | d<<24
    let o = target.octets();
    let dest: u32 = (o[0] as u32) | (o[1] as u32) << 8 | (o[2] as u32) << 16 | (o[3] as u32) << 24;

    unsafe {
        let handle: HANDLE = match IcmpCreateFile() {
            Ok(h) if !h.is_invalid() => h,
            _ => return ProbeOutcome::Timeout,
        };

        let opts = IP_OPTION_INFORMATION {
            Ttl: ttl,
            Tos: 0,
            Flags: 0,
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };

        let reply_size = std::mem::size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8;
        // u64 backing store: ICMP_ECHO_REPLY contains pointers, so it needs 8-byte alignment
        // (a Vec<u8> only guarantees 1) before being read through a typed reference.
        let mut reply_buf = vec![0u64; reply_size.div_ceil(8)];
        let reply_bytes = reply_buf.len() * 8;

        let ret = IcmpSendEcho(
            handle,
            dest,
            payload.as_ptr() as *const core::ffi::c_void,
            payload.len() as u16,
            Some(&opts),
            reply_buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_bytes as u32,
            PROBE_TIMEOUT_MS,
        );

        let _ = IcmpCloseHandle(handle);

        // ret = number of replies stored; also never read a reply that cannot fit in our buffer.
        if ret == 0 || reply_bytes < std::mem::size_of::<ICMP_ECHO_REPLY>() {
            return ProbeOutcome::Timeout;
        }

        let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
        let bytes = reply.Address.to_le_bytes();
        let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
        // Yanıt verisi (Data/DataSize) buffer içinde ve bu prob'un payload'una eşit olmalı.
        let buf = std::slice::from_raw_parts(reply_buf.as_ptr() as *const u8, reply_bytes);
        let payload_ok =
            echo_data_slice(buf, reply.Data as usize, reply.DataSize as usize) == Some(payload);
        classify_status(
            reply.Status,
            IpAddr::V4(ip),
            reply.RoundTripTime as f64,
            IpAddr::V4(target),
            payload_ok,
        )
    }
}

/// IPv6 prob — `Icmp6SendEcho2` (admin gerektirmez).
#[cfg(windows)]
fn icmp_probe_v6(target: Ipv6Addr, ttl: u8, payload: &[u8]) -> ProbeOutcome {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{
        Icmp6CreateFile, Icmp6SendEcho2, IcmpCloseHandle, ICMPV6_ECHO_REPLY_LH,
        IP_OPTION_INFORMATION,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6, SOCKADDR_IN6_0,
    };

    // sin6_addr: octet dizisi network byte order (big-endian segment baytları).
    let seg = target.octets();

    let make_sockaddr = |addr_bytes: [u8; 16]| SOCKADDR_IN6 {
        sin6_family: AF_INET6,
        sin6_port: 0,
        sin6_flowinfo: 0,
        sin6_addr: IN6_ADDR {
            u: IN6_ADDR_0 { Byte: addr_bytes },
        },
        Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: 0 },
    };

    // Kaynak adres :: (unspecified) — sistem uygun arayüzü seçer.
    let src = make_sockaddr([0u8; 16]);
    let dst = make_sockaddr(seg);

    unsafe {
        let handle: HANDLE = match Icmp6CreateFile() {
            Ok(h) if !h.is_invalid() => h,
            _ => return ProbeOutcome::Timeout,
        };

        let opts = IP_OPTION_INFORMATION {
            Ttl: ttl, // IPv6'da hop limit olarak kullanılır
            Tos: 0,
            Flags: 0,
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };

        // Icmp6 reply buffer, IPv4'tekinden daha büyük olmalı (LH yapısı + payload + ICMP error).
        let reply_size =
            std::mem::size_of::<ICMPV6_ECHO_REPLY_LH>() + payload.len() + 64;
        // u64 backing store -> 8-byte aligned (see the IPv4 path).
        let mut reply_buf = vec![0u64; reply_size.div_ceil(8)];
        let reply_bytes = reply_buf.len() * 8;

        let ret = Icmp6SendEcho2(
            handle,
            None,
            None,
            None,
            &src,
            &dst,
            payload.as_ptr() as *const core::ffi::c_void,
            payload.len() as u16,
            Some(&opts),
            reply_buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_bytes as u32,
            PROBE_TIMEOUT_MS,
        );

        let _ = IcmpCloseHandle(handle);

        if ret == 0 || reply_bytes < std::mem::size_of::<ICMPV6_ECHO_REPLY_LH>() {
            return ProbeOutcome::Timeout;
        }

        let reply = &*(reply_buf.as_ptr() as *const ICMPV6_ECHO_REPLY_LH);
        // Address.sin6_addr: [u16; 8] network byte order → Ipv6Addr.
        let words = reply.Address.sin6_addr;
        let ip = Ipv6Addr::new(
            u16::from_be(words[0]), u16::from_be(words[1]),
            u16::from_be(words[2]), u16::from_be(words[3]),
            u16::from_be(words[4]), u16::from_be(words[5]),
            u16::from_be(words[6]), u16::from_be(words[7]),
        );
        // ICMPV6_ECHO_REPLY'de DataSize alanı yok; yanıt verisinin yapıdan sonraki konumu
        // belgelenmemiş olduğundan IPv6'da payload doğrulanmaz — yalnızca kaynak adres
        // = hedef kontrolü uygulanır (payload_ok = true).
        classify_status(
            reply.Status,
            IpAddr::V6(ip),
            reply.RoundTripTime as f64,
            IpAddr::V6(target),
            true,
        )
    }
}

/// IP Helper status kodunu ProbeOutcome'a çevirir (v4/v6 ortak).
#[cfg_attr(not(windows), allow(dead_code))]
fn classify_status(
    status: u32,
    ip: IpAddr,
    rtt_ms: f64,
    target: IpAddr,
    payload_ok: bool,
) -> ProbeOutcome {
    match status {
        // Hedefe ulaşıldı sayılması için kaynak = hedef VE yanıt bu prob'un payload'ını taşımalı.
        // Aksi halde (eski/yanlış eşleşmiş yanıt) prob zaman aşımı gibi sayılır.
        IP_SUCCESS if ip == target && payload_ok => ProbeOutcome::Reply { ip, rtt_ms, reached: true },
        IP_SUCCESS => {
            crate::dlog!("[traceroute] IP_SUCCESS reddedildi (kaynak/payload uyuşmaz)");
            ProbeOutcome::Timeout
        }
        IP_TTL_EXPIRED_TRANSIT => ProbeOutcome::Reply { ip, rtt_ms, reached: false },
        // Diğer tüm statüler (timeout, unreachable vb.) → bu prob başarısız.
        _ => ProbeOutcome::Timeout,
    }
}

/// Windows dışı platformlarda IP Helper yok — şimdilik desteklenmiyor.
/// (NetVeil pratikte Windows-only: tüm diğer modüller WlanAPI/PowerShell kullanır.)
#[cfg(not(windows))]
fn icmp_probe_v4(_target: Ipv4Addr, _ttl: u8, _payload: &[u8]) -> ProbeOutcome {
    ProbeOutcome::Timeout
}

#[cfg(not(windows))]
fn icmp_probe_v6(_target: std::net::Ipv6Addr, _ttl: u8, _payload: &[u8]) -> ProbeOutcome {
    ProbeOutcome::Timeout
}

/// Prob sonuçlarından HopResult kurar. Tüm prob'lar timeout ise → hop timeout.
/// RTT olarak yanıt veren prob'ların min'i alınır (rapor önerisi).
fn build_hop(ttl: u8, outcomes: &[ProbeOutcome]) -> HopResult {
    let mut hop_ip: Option<IpAddr> = None;
    let mut rtts: Vec<f64> = Vec::new();

    for o in outcomes {
        if let ProbeOutcome::Reply { ip, rtt_ms, .. } = o {
            hop_ip.get_or_insert(*ip);
            rtts.push(*rtt_ms);
        }
    }

    let Some(ip) = hop_ip else {
        return HopResult {
            hop: ttl,
            ip: "*".into(),
            hostname: None,
            rtt_ms: None,
            min_ms: None,
            max_ms: None,
            timed_out: true,
            country: None,
            country_name: None,
            run_id: 0,
        };
    };

    let min = rtts.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = rtts.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    HopResult {
        hop: ttl,
        ip: ip.to_string(),
        hostname: None,
        rtt_ms: Some(round2(min)), // min RTT — en temsili (rapor önerisi)
        min_ms: Some(round2(min)),
        max_ms: Some(round2(max)),
        timed_out: false,
        country: None,
        country_name: None,
        run_id: 0,
    }
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
/// `permit` bloklayıcı çağrı bitene kadar tutulur: zaman aşımına uğrayıp terk edilen bir sorgu
/// thread'i hâlâ meşgulken yeni sorgu eşzamanlılık sınırını aşamaz.
async fn reverse_dns(ip: &str, permit: tokio::sync::OwnedSemaphorePermit) -> Option<String> {
    let Ok(addr) = ip.parse::<IpAddr>() else { return None };
    let ip_str = ip.to_string();
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        dns_lookup::lookup_addr(&addr).ok()
    })
    .await
    .ok()
    .flatten()
    .filter(|name| name != &ip_str)
}

/// Yayınlanacak hostname güncellemesi: yalnızca çözüm varsa VE koşu hâlâ güncelse.
/// (Durdurulmuş/yenisiyle değiştirilmiş koşunun geç PTR sonucu asla yayınlanmaz.)
fn host_update(current: bool, run_id: u64, hop: u8, hostname: Option<String>) -> Option<HopHost> {
    let hostname = hostname?;
    current.then_some(HopHost { hop, hostname, run_id })
}

/// Reverse DNS'i paralel (sınırlı eşzamanlılık + sınırlı bekleme) başlatır; sonuç gelince
/// `traceroute:hop_host` emit eder. Hop yayını ve sonraki TTL bu sorguyu beklemez.
fn spawn_hostname_lookup(
    tasks: &mut tokio::task::JoinSet<()>,
    app: AppHandle,
    slots: Arc<tokio::sync::Semaphore>,
    hop: u8,
    ip: String,
    token: u64,
    run_id: u64,
) {
    tasks.spawn(async move {
        let Ok(permit) = slots.acquire_owned().await else { return };
        if !is_current(token) {
            return; // terk edilmiş koşu için bloklayıcı sorgu başlatma
        }
        let hostname = tokio::time::timeout(PTR_LOOKUP_TIMEOUT, reverse_dns(&ip, permit))
            .await
            .ok()
            .flatten();
        if let Some(update) = host_update(is_current(token), run_id, hop, hostname) {
            let _ = app.emit(EVENT_HOP_HOST, update);
        }
    });
}

/// GeoIP sorgusunu paralel başlatır; sonuç gelince geo-update event'i emit eder.
/// Task is added to `tasks` so the run can await outstanding lookups before `done`.
fn spawn_geo_lookup(
    tasks: &mut tokio::task::JoinSet<()>,
    app: AppHandle,
    http: reqwest::Client,
    hop: u8,
    ip: String,
    token: u64,
    run_id: u64,
) {
    tasks.spawn(async move {
        let (country, country_name) = fetch_country(&http, &ip, hop).await;
        if (country.is_some() || country_name.is_some()) && is_current(token) {
            let _ = app.emit(
                EVENT_HOP_GEO,
                HopGeo {
                    hop,
                    country,
                    country_name,
                    run_id,
                },
            );
        }
    });
}

/// NetVeil sunucusunun gizlilik-korumalı `GET /geo/{ip}` endpoint'i ile ülke kodu + adı çeker
/// (sunucu sorgulanan IP'yi saklamaz/loglamaz; GeoLite2 araması yalnızca bellekte).
/// Yanıt: 200 `{"country_code","country_name"}`; 400/404/429/503 ve her hata → None.
/// Private/loopback IP'ler için: SADECE hop 1 (ilk hop = yerel ağ geçidi) "LAN" döner.
/// Hop 1 dışında private IP (ISP omurgası / hedef-tarafı NAT) "LAN" DEĞİL — etiketsiz
/// bırakılır (ham IP görünür; yanlış LAN etiketi vermeyiz). Timeout/hata → None.
async fn fetch_country(client: &reqwest::Client, ip: &str, hop: u8) -> (Option<String>, Option<String>) {
    if is_private_ip(ip) {
        if hop == 1 {
            return (Some("LAN".into()), Some("Local Network".into()));
        }
        // Hop >1 private: ISP/uzak NAT olabilir — LAN deme, etiketsiz bırak.
        return (None, None);
    }

    // Yalnızca halka açık, geçerli literal IP sunucuya gönderilir (özel/multicast/belge/CGNAT
    // vb. adresler ve ayrıştırılamayan değerler asla sorgulanmaz; sunucu da bunları 400'ler).
    match ip.parse::<IpAddr>() {
        Ok(addr) if crate::remote_list::is_public_ip(addr) => {}
        _ => return (None, None),
    }
    let url = format!("{GEO_URL_BASE}{ip}");

    #[derive(Deserialize)]
    struct GeoResp {
        #[serde(default)]
        country_code: Option<String>,
        #[serde(default)]
        country_name: Option<String>,
    }

    // Sunucu /geo'yu yalnızca NetVeil istek biçimine (X-NetVeil-Client: x.y.z) yanıtlar;
    // kimlik doğrulama değil, dış sorguları eleyen bir kapı. UA yalnızca bu istekte ayarlanır.
    let version = env!("CARGO_PKG_VERSION");
    let Ok(resp) = client
        .get(&url)
        .header("X-NetVeil-Client", version)
        .header(reqwest::header::USER_AGENT, format!("NetVeil/{version}"))
        .send()
        .await
    else {
        return (None, None);
    };
    if !resp.status().is_success() {
        return (None, None);
    }
    match resp.json::<GeoResp>().await {
        Ok(data) => (
            data.country_code.filter(|s| !s.is_empty()),
            data.country_name.filter(|s| !s.is_empty()),
        ),
        Err(_) => (None, None),
    }
}

/// RFC 1918 / loopback / link-local / CGNAT adresleri kontrol eder.
fn is_private_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else { return false };
    match addr {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64) // CGNAT 100.64/10
        }
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || (seg0 & 0xFE00) == 0xFC00 // ULA fc00::/7
                || (seg0 & 0xFFC0) == 0xFE80 // link-local fe80::/10
        }
    }
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

// ── Testler ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn reply(ip: &str, rtt: f64, reached: bool) -> ProbeOutcome {
        ProbeOutcome::Reply {
            ip: ip.parse().unwrap(),
            rtt_ms: rtt,
            reached,
        }
    }

    #[test]
    fn test_build_hop_all_timeout() {
        let hop = build_hop(3, &[ProbeOutcome::Timeout, ProbeOutcome::Timeout]);
        assert!(hop.timed_out);
        assert_eq!(hop.ip, "*");
        assert_eq!(hop.hop, 3);
        assert!(hop.rtt_ms.is_none());
    }

    #[test]
    fn test_build_hop_uses_min_rtt() {
        let hop = build_hop(
            7,
            &[reply("1.1.1.1", 20.0, false), reply("1.1.1.1", 11.5, false), reply("1.1.1.1", 15.2, false)],
        );
        assert!(!hop.timed_out);
        assert_eq!(hop.ip, "1.1.1.1");
        assert_eq!(hop.rtt_ms, Some(11.5));
        assert_eq!(hop.min_ms, Some(11.5));
        assert_eq!(hop.max_ms, Some(20.0));
    }

    #[test]
    fn test_build_hop_partial_timeout_takes_first_ip() {
        // İlk prob timeout, ikincisi yanıt → hop yine de yanıt veren olarak işaretlenir.
        let hop = build_hop(5, &[ProbeOutcome::Timeout, reply("8.8.8.8", 9.3, false)]);
        assert!(!hop.timed_out);
        assert_eq!(hop.ip, "8.8.8.8");
        assert_eq!(hop.rtt_ms, Some(9.3));
    }

    #[test]
    fn test_build_hop_reached_target() {
        let hop = build_hop(12, &[reply("1.1.1.1", 8.0, true)]);
        assert!(!hop.timed_out);
        assert_eq!(hop.ip, "1.1.1.1");
    }

    #[test]
    fn test_ipv4_dest_byte_order() {
        // IcmpSendEcho dest encoding doğrulaması: 1.2.3.4 → LE u32 = 0x04030201
        let o = Ipv4Addr::new(1, 2, 3, 4).octets();
        let dest: u32 =
            (o[0] as u32) | (o[1] as u32) << 8 | (o[2] as u32) << 16 | (o[3] as u32) << 24;
        assert_eq!(dest, 0x04030201);
    }

    #[test]
    fn test_private_ip_detection() {
        assert!(is_private_ip("192.168.1.1"));
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("172.16.0.1"));
        assert!(is_private_ip("127.0.0.1"));
        assert!(is_private_ip("100.64.0.1")); // CGNAT
        assert!(!is_private_ip("8.8.8.8"));
        assert!(!is_private_ip("1.1.1.1"));
    }

    #[test]
    fn test_stale_trace_token_after_stop_and_restart() {
        let first = TRACE_GEN.fetch_add(1, Ordering::SeqCst) + 1;
        assert!(is_current(first));
        TRACE_GEN.fetch_add(1, Ordering::SeqCst); // stop_traceroute
        assert!(!is_current(first), "stopped trace must be cancelled");
        let second = TRACE_GEN.fetch_add(1, Ordering::SeqCst) + 1; // restart
        assert!(!is_current(first), "stopped trace must not revive");
        assert!(is_current(second));
    }

    #[test]
    fn test_events_carry_run_id() {
        let mut hop = build_hop(1, &[reply("1.1.1.1", 5.0, false)]);
        hop.run_id = 42;
        let v = serde_json::to_value(&hop).unwrap();
        assert_eq!(v["run_id"], 42);
        let geo = serde_json::to_value(HopGeo { hop: 1, country: None, country_name: None, run_id: 7 }).unwrap();
        assert_eq!(geo["run_id"], 7);
        assert_eq!(serde_json::to_value(TraceDone { run_id: 9 }).unwrap()["run_id"], 9);
        let e = serde_json::to_value(TraceError { run_id: 3, message: "x".into() }).unwrap();
        assert_eq!((e["run_id"].clone(), e["message"].clone()), (serde_json::json!(3), serde_json::json!("x")));
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn is_reached(o: &ProbeOutcome) -> bool {
        matches!(o, ProbeOutcome::Reply { reached: true, .. })
    }

    #[test]
    fn test_success_from_target_with_matching_payload_is_reached() {
        let o = classify_status(IP_SUCCESS, ip("1.1.1.1"), 5.0, ip("1.1.1.1"), true);
        assert!(is_reached(&o));
    }

    #[test]
    fn test_success_from_wrong_source_is_rejected() {
        // Target 1.1.1.1 but IP_SUCCESS from another address: timeout-equivalent, address not recorded.
        let o = classify_status(IP_SUCCESS, ip("192.168.1.1"), 1.0, ip("1.1.1.1"), true);
        assert!(matches!(o, ProbeOutcome::Timeout));
        let hop = build_hop(1, &[o]);
        assert!(hop.timed_out);
        assert_eq!(hop.ip, "*");
    }

    #[test]
    fn test_success_with_payload_mismatch_is_rejected() {
        // Same target, but the reply belongs to another (stale) probe.
        let o = classify_status(IP_SUCCESS, ip("1.1.1.1"), 1.0, ip("1.1.1.1"), false);
        assert!(matches!(o, ProbeOutcome::Timeout));
    }

    #[test]
    fn test_time_exceeded_is_never_reached() {
        let o = classify_status(IP_TTL_EXPIRED_TRANSIT, ip("192.168.1.1"), 2.0, ip("1.1.1.1"), false);
        assert!(matches!(o, ProbeOutcome::Reply { reached: false, .. }));
        // Even a Time-Exceeded carrying the target address is not "reached".
        let o = classify_status(IP_TTL_EXPIRED_TRANSIT, ip("1.1.1.1"), 2.0, ip("1.1.1.1"), true);
        assert!(matches!(o, ProbeOutcome::Reply { reached: false, .. }));
    }

    #[test]
    fn test_other_statuses_are_timeouts() {
        let o = classify_status(11010, ip("1.1.1.1"), 1.0, ip("1.1.1.1"), true);
        assert!(matches!(o, ProbeOutcome::Timeout));
    }

    #[test]
    fn test_mixed_probes_terminate_only_on_valid_success() {
        let target = ip("1.1.1.1");
        // Rejected stale success + a valid Time-Exceeded: hop shows only the valid address, no termination.
        let stale = classify_status(IP_SUCCESS, ip("203.0.113.9"), 1.0, target, true);
        let te = classify_status(IP_TTL_EXPIRED_TRANSIT, ip("192.168.1.1"), 2.0, target, false);
        let outcomes = [stale, te];
        assert!(!outcomes.iter().any(is_reached), "stale success must not end the trace");
        let hop = build_hop(1, &outcomes);
        assert_eq!(hop.ip, "192.168.1.1");

        // A valid success from the target does terminate.
        let good = classify_status(IP_SUCCESS, target, 9.0, target, true);
        let outcomes = [reply("10.0.0.1", 3.0, false), good];
        assert!(outcomes.iter().any(is_reached));
        assert_eq!(build_hop(2, &outcomes).ip, "10.0.0.1");
    }

    #[test]
    fn test_probe_payload_is_unique_and_prefixed() {
        let a = make_probe_payload();
        let b = make_probe_payload();
        assert!(a.starts_with(PROBE_PAYLOAD_PREFIX));
        assert_eq!(a.len(), PROBE_PAYLOAD_PREFIX.len() + 8);
        assert_ne!(a, b);
    }

    #[test]
    fn test_echo_data_slice_bounds() {
        let buf = [0u8, 1, 2, 3, 4, 5, 6, 7];
        let base = buf.as_ptr() as usize;
        assert_eq!(echo_data_slice(&buf, base + 2, 3), Some(&buf[2..5]));
        assert_eq!(echo_data_slice(&buf, base + 6, 2), Some(&buf[6..8]));
        assert_eq!(echo_data_slice(&buf, base + 6, 3), None); // past the end
        assert_eq!(echo_data_slice(&buf, base.wrapping_sub(1), 2), None); // before the start
        assert_eq!(echo_data_slice(&buf, 0, 2), None); // outside the buffer (null / foreign pointer)
        assert_eq!(echo_data_slice(&buf, base + 1, usize::MAX), None); // length overflow
    }

    #[test]
    fn test_host_update_stale_generation_cannot_publish() {
        let upd = host_update(true, 11, 3, Some("router.example".into())).expect("current run publishes");
        assert_eq!((upd.hop, upd.run_id, upd.hostname.as_str()), (3, 11, "router.example"));
        assert!(host_update(true, 11, 3, None).is_none(), "no hostname -> nothing to publish");
        // Stopped/superseded run (is_current == false): a late PTR result must not publish.
        assert!(host_update(false, 11, 3, Some("late.example".into())).is_none());
    }

    #[test]
    fn test_hop_host_event_payload() {
        let v = serde_json::to_value(HopHost { hop: 2, hostname: "h".into(), run_id: 5 }).unwrap();
        assert_eq!((v["hop"].clone(), v["hostname"].clone(), v["run_id"].clone()), (serde_json::json!(2), serde_json::json!("h"), serde_json::json!(5)));
    }

    #[test]
    fn test_ptr_slots_shared_across_generations() {
        // Two simulated runs (generations) must see the SAME process-wide limiter.
        let (gen_a, gen_b) = (ptr_slots(), ptr_slots());
        assert!(Arc::ptr_eq(&gen_a, &gen_b));
        let mut held = Vec::new();
        while let Ok(p) = gen_a.clone().try_acquire_owned() {
            held.push(p);
            assert!(held.len() <= PTR_CONCURRENCY, "outstanding permits exceeded the limit");
        }
        assert_eq!(held.len(), PTR_CONCURRENCY);
        // A "restarted" run gets no fresh permits while old blocking lookups still hold theirs.
        assert!(gen_b.clone().try_acquire_owned().is_err());
        held.pop();
        assert!(gen_b.try_acquire_owned().is_ok());
    }

    #[test]
    fn test_round2() {
        assert_eq!(round2(11.555), 11.56);
        assert_eq!(round2(20.0), 20.0);
    }
}

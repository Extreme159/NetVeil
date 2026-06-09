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
/// ICMP payload — sabit küçük bir imza.
const PROBE_PAYLOAD: &[u8] = b"netveil-traceroute";

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
}

/// Hop'un GeoIP bilgisi paralel çekildikten sonra gönderilen güncelleme.
#[derive(Debug, Serialize, Clone)]
struct HopGeo {
    hop: u8,
    country: Option<String>,
    country_name: Option<String>,
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
) -> Result<(), String> {
    let app2 = app.clone();
    tokio::task::spawn(async move {
        if let Err(e) = stream_trace(app2, host, ip_version, resolve_hostnames).await {
            let _ = app.emit(EVENT_ERROR, e);
        }
    });
    Ok(())
}

async fn stream_trace(
    app: AppHandle,
    host: String,
    ip_version: String,
    resolve_hostnames: bool,
) -> Result<(), String> {
    // Hedefi çöz (IPv4 öncelikli — oyun/ping senaryosu için doğru tercih).
    let target = resolve_target(&host, &ip_version).await?;

    // HTTP client — ülke sorguları için (paralel görevlerde paylaşılır).
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .map_err(|e| format!("HTTP client hatası: {e}"))?;

    let mut consecutive_timeouts: u8 = 0;

    for ttl in 1..=MAX_HOPS {
        let outcomes = probe_hop(target, ttl).await;
        let mut hop = build_hop(ttl, &outcomes);

        let reached = outcomes.iter().any(|o| matches!(o, ProbeOutcome::Reply { reached: true, .. }));

        if hop.timed_out {
            consecutive_timeouts += 1;
        } else {
            consecutive_timeouts = 0;
            if resolve_hostnames {
                hop.hostname = reverse_dns(&hop.ip).await;
            }
        }

        let _ = app.emit(EVENT_HOP, &hop);

        if !hop.timed_out {
            spawn_geo_lookup(app.clone(), http.clone(), ttl, hop.ip.clone());
        }

        if reached {
            break;
        }
        if consecutive_timeouts >= MAX_CONSECUTIVE_TIMEOUTS {
            break;
        }
    }

    let _ = app.emit(EVENT_DONE, ());
    Ok(())
}

/// Belirli bir TTL için PROBES_PER_HOP prob gönderir.
/// Her prob bloklayıcı IP Helper çağrısı olduğundan blocking thread'de koşar.
async fn probe_hop(target: IpAddr, ttl: u8) -> Vec<ProbeOutcome> {
    let mut outcomes = Vec::with_capacity(PROBES_PER_HOP as usize);
    for _ in 0..PROBES_PER_HOP {
        let outcome = tokio::task::spawn_blocking(move || icmp_probe(target, ttl))
            .await
            .unwrap_or(ProbeOutcome::Timeout);
        outcomes.push(outcome);
    }
    outcomes
}

/// Tek bir ICMP echo prob'u — verilen TTL ile. Bloklayıcı.
fn icmp_probe(target: IpAddr, ttl: u8) -> ProbeOutcome {
    match target {
        IpAddr::V4(v4) => icmp_probe_v4(v4, ttl),
        IpAddr::V6(v6) => icmp_probe_v6(v6, ttl),
    }
}

/// IPv4 prob — `IcmpSendEcho` (admin gerektirmez).
#[cfg(windows)]
fn icmp_probe_v4(target: Ipv4Addr, ttl: u8) -> ProbeOutcome {
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

        let reply_size = std::mem::size_of::<ICMP_ECHO_REPLY>() + PROBE_PAYLOAD.len() + 8;
        let mut reply_buf = vec![0u8; reply_size];

        let ret = IcmpSendEcho(
            handle,
            dest,
            PROBE_PAYLOAD.as_ptr() as *const core::ffi::c_void,
            PROBE_PAYLOAD.len() as u16,
            Some(&opts),
            reply_buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_size as u32,
            PROBE_TIMEOUT_MS,
        );

        let _ = IcmpCloseHandle(handle);

        if ret == 0 {
            return ProbeOutcome::Timeout;
        }

        let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
        let bytes = reply.Address.to_le_bytes();
        let ip = Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]);
        classify_status(reply.Status, IpAddr::V4(ip), reply.RoundTripTime as f64)
    }
}

/// IPv6 prob — `Icmp6SendEcho2` (admin gerektirmez).
#[cfg(windows)]
fn icmp_probe_v6(target: Ipv6Addr, ttl: u8) -> ProbeOutcome {
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
            std::mem::size_of::<ICMPV6_ECHO_REPLY_LH>() + PROBE_PAYLOAD.len() + 64;
        let mut reply_buf = vec![0u8; reply_size];

        let ret = Icmp6SendEcho2(
            handle,
            None,
            None,
            None,
            &src,
            &dst,
            PROBE_PAYLOAD.as_ptr() as *const core::ffi::c_void,
            PROBE_PAYLOAD.len() as u16,
            Some(&opts),
            reply_buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_size as u32,
            PROBE_TIMEOUT_MS,
        );

        let _ = IcmpCloseHandle(handle);

        if ret == 0 {
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
        classify_status(reply.Status, IpAddr::V6(ip), reply.RoundTripTime as f64)
    }
}

/// IP Helper status kodunu ProbeOutcome'a çevirir (v4/v6 ortak).
#[cfg(windows)]
fn classify_status(status: u32, ip: IpAddr, rtt_ms: f64) -> ProbeOutcome {
    match status {
        IP_SUCCESS => ProbeOutcome::Reply { ip, rtt_ms, reached: true },
        IP_TTL_EXPIRED_TRANSIT => ProbeOutcome::Reply { ip, rtt_ms, reached: false },
        // Diğer tüm statüler (timeout, unreachable vb.) → bu prob başarısız.
        _ => ProbeOutcome::Timeout,
    }
}

/// Windows dışı platformlarda IP Helper yok — şimdilik desteklenmiyor.
/// (NetVeil pratikte Windows-only: tüm diğer modüller WlanAPI/PowerShell kullanır.)
#[cfg(not(windows))]
fn icmp_probe_v4(_target: Ipv4Addr, _ttl: u8) -> ProbeOutcome {
    ProbeOutcome::Timeout
}

#[cfg(not(windows))]
fn icmp_probe_v6(_target: std::net::Ipv6Addr, _ttl: u8) -> ProbeOutcome {
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
async fn reverse_dns(ip: &str) -> Option<String> {
    let Ok(addr) = ip.parse::<IpAddr>() else { return None };
    let ip_str = ip.to_string();
    tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&addr).ok())
        .await
        .ok()
        .flatten()
        .filter(|name| name != &ip_str)
}

/// GeoIP sorgusunu paralel başlatır; sonuç gelince geo-update event'i emit eder.
fn spawn_geo_lookup(app: AppHandle, http: reqwest::Client, hop: u8, ip: String) {
    tokio::task::spawn(async move {
        let (country, country_name) = fetch_country(&http, &ip, hop).await;
        if country.is_some() || country_name.is_some() {
            let _ = app.emit(
                EVENT_HOP_GEO,
                HopGeo {
                    hop,
                    country,
                    country_name,
                },
            );
        }
    });
}

/// ip-api.com ücretsiz JSON endpoint ile ülke kodu + adı çeker.
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

    // ip-api.com HTTPS'i ücretli plan gerektiriyor; ücretsiz endpoint yalnızca HTTP.
    // İstek uygulama kökenli ve hassas veri taşımıyor.
    let url = format!("http://ip-api.com/json/{ip}?fields=status,country,countryCode");

    #[derive(Deserialize)]
    struct IpApiResp {
        status: String,
        #[serde(default)]
        country: String,
        #[serde(rename = "countryCode", default)]
        country_code: String,
    }

    match client.get(&url).send().await {
        Ok(resp) => match resp.json::<IpApiResp>().await {
            Ok(data) if data.status == "success" => {
                let code = if data.country_code.is_empty() { None } else { Some(data.country_code) };
                let name = if data.country.is_empty() { None } else { Some(data.country) };
                (code, name)
            }
            _ => (None, None),
        },
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
    fn test_round2() {
        assert_eq!(round2(11.555), 11.56);
        assert_eq!(round2(20.0), 20.0);
    }
}

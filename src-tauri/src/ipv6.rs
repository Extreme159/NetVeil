// IPv6 Diagnostics — bağlantı tespiti ve kalite skoru.

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv6Addr};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// ─── Structs ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ipv6AddressInfo {
    pub address: String,
    pub prefix_len: u8,
    pub scope: String,        // "Global" | "LinkLocal" | "UniqueLocal" | "Loopback"
    pub address_type: String, // "Native" | "Teredo" | "6to4" | "ISATAP" | "LinkLocal" | "Unknown"
    pub is_temporary: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ipv6ConnectivityInfo {
    pub has_ipv6: bool,
    pub has_native_ipv6: bool,
    pub has_tunnel_only: bool,
    pub addresses: Vec<Ipv6AddressInfo>,
    pub gateway_v6: Option<String>,
    pub wan_ipv6: Option<String>,
    pub isp_ipv6: Option<String>,
    pub dns_ipv6_works: bool,
    /// En az bir IPv6 TCP tasima probu (literal global adres, DNS'siz) basarili oldu mu.
    /// Eski sonuclarda yok -> serde default (false).
    #[serde(default)]
    pub ipv6_transport_works: bool,
    pub connectivity_grade: String, // "Full" | "Partial" | "Tunnel" | "None"
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ipv6QualityScore {
    pub score: u8,
    pub grade: String,          // "Excellent" | "Good" | "Partial" | "Poor" | "None"
    pub has_connectivity: bool,
    pub has_native: bool,
    pub dns_works: bool,
    pub isp_assessment: String,
    pub recommendations: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Ipv6TestResult {
    pub connectivity: Ipv6ConnectivityInfo,
    pub quality: Ipv6QualityScore,
}

#[derive(Serialize, Clone, Debug)]
pub struct Ipv6Progress {
    pub phase: String,   // "connectivity" | "scoring"
    pub status: String,  // "running" | "done" | "error"
    pub message: String,
}

// ─── Yardımcı ─────────────────────────────────────────────────────────────────

fn emit_progress(app: &AppHandle, phase: &str, status: &str, message: &str) {
    let _ = app.emit("ipv6:progress", Ipv6Progress {
        phase: phase.to_string(),
        status: status.to_string(),
        message: message.to_string(),
    });
}

// ─── Adres tipi / scope tespit ────────────────────────────────────────────────

/// Adresi (scope, address_type) olarak sınıflandırır.
///
/// Önceki sürüm string-prefix tabanlıydı ve hatalı sonuçlar üretebiliyordu:
///   - HE Tunnel Broker (2001:470::/32) "Native" sayılıyordu (rapor maddesi),
///   - ISATAP için `:fe:` substring kontrolü yanlış pozitif üretebiliyordu,
///   - `2001:0:` prefix'i Teredo dışı 2001 bloklarıyla karışabiliyordu.
///
/// Bu sürüm adresi `Ipv6Addr` olarak parse edip RFC prefix'lerine göre ayırır.
/// Parse başarısızsa string-fallback'e döner (PowerShell çıktısı genelde geçerlidir).
fn classify_address(addr: &str) -> (String, String) {
    // %zone_id (örn. fe80::1%12) varsa ayıkla.
    let clean = addr.split('%').next().unwrap_or(addr).trim();

    let Ok(v6) = clean.parse::<Ipv6Addr>() else {
        // Parse edilemezse en azından loopback/link-local'i yakala.
        let lower = clean.to_lowercase();
        if lower == "::1" {
            return ("Loopback".into(), "Loopback".into());
        }
        if lower.starts_with("fe80") {
            return ("LinkLocal".into(), "LinkLocal".into());
        }
        return ("Unknown".into(), "Unknown".into());
    };

    let s = v6.segments();

    if v6.is_loopback() {
        ("Loopback".into(), "Loopback".into())
    } else if (s[0] & 0xffc0) == 0xfe80 {
        ("LinkLocal".into(), "LinkLocal".into())
    } else if (s[0] & 0xfe00) == 0xfc00 {
        // ULA fc00::/7 — yalnızca yerel; internete yönlendirilmez, "Native" SAYILMAZ.
        ("UniqueLocal".into(), "UniqueLocal".into())
    } else if s[0] == 0x2001 && s[1] == 0x0000 {
        ("Global".into(), "Teredo".into()) // 2001:0000::/32 (RFC 4380)
    } else if s[0] == 0x2001 && s[1] == 0x0470 {
        ("Global".into(), "Tunnel".into()) // 2001:470::/32 Hurricane Electric 6in4
    } else if s[0] == 0x2002 {
        ("Global".into(), "6to4".into()) // 2002::/16 (RFC 3056)
    } else if is_isatap(&s) {
        ("Global".into(), "ISATAP".into())
    } else if (s[0] & 0xe000) == 0x2000 {
        // 2000::/3 global unicast — yukarıdaki tünel türleri dışındakiler native.
        ("Global".into(), "Native".into())
    } else {
        ("Unknown".into(), "Unknown".into())
    }
}

/// ISATAP interface-id imzası: son 64 bit ::0:5efe:x.x.x.x veya ::200:5efe:x.x.x.x.
/// Yani 5. segment 0x5efe ve 4. segment 0x0000/0x0200 (RFC 5214).
fn is_isatap(s: &[u16; 8]) -> bool {
    s[5] == 0x5efe && (s[4] == 0x0000 || s[4] == 0x0200)
}

/// Bir address_type'ın "global ulaşılabilir" sayılıp sayılmadığı.
/// Native + tünel türleri dahil; link-local/loopback hariç.
fn is_globally_routable_type(addr_type: &str) -> bool {
    matches!(addr_type, "Native" | "6to4" | "Teredo" | "Tunnel" | "ISATAP")
}

// ─── Windows PowerShell helpers ───────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn ps(cmd: &str) -> String {
    let Ok(out) = std::process::Command::new(crate::powershell_exe())
        .args(["-NoProfile", "-NonInteractive", "-Command", cmd])
        .creation_flags(CREATE_NO_WINDOW)
        .output() else { return String::new(); };
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[cfg(not(target_os = "windows"))]
fn ps(_cmd: &str) -> String { String::new() }

fn get_ipv6_addresses() -> Vec<Ipv6AddressInfo> {
    #[cfg(target_os = "windows")]
    {
        // Sadece bağlı (Up) ve fiziksel/sanal ethernet/wi-fi arayüzlerini al
        // Loopback, tunnel-sadece (isatap, teredo pseudo), devre dışı NIC'leri hariç tut
        let raw = ps(
            "$upIfaces = (Get-NetAdapter | Where-Object {$_.Status -eq 'Up'}).InterfaceIndex; \
             Get-NetIPAddress -AddressFamily IPv6 \
             | Where-Object { \
                 $upIfaces -contains $_.InterfaceIndex -and \
                 $_.InterfaceAlias -notlike '*Loopback*' -and \
                 $_.InterfaceAlias -notlike '*isatap*' \
             } \
             | Select-Object IPAddress,PrefixLength,SuffixOrigin \
             | ConvertTo-Csv -NoTypeInformation"
        );
        let mut result = Vec::new();
        for line in raw.lines().skip(1) {
            let cols: Vec<&str> = line.splitn(3, ',').collect();
            if cols.len() < 2 { continue; }
            let addr    = cols[0].trim().trim_matches('"');
            let plen    = cols[1].trim().trim_matches('"').parse::<u8>().unwrap_or(64);
            let suffix  = cols.get(2).unwrap_or(&"").trim().trim_matches('"');
            let is_temp = suffix.eq_ignore_ascii_case("random");
            let (scope, addr_type) = classify_address(addr);
            if scope == "Loopback" { continue; }
            result.push(Ipv6AddressInfo {
                address: addr.to_string(),
                prefix_len: plen,
                scope,
                address_type: addr_type,
                is_temporary: is_temp,
            });
        }
        result
    }
    #[cfg(not(target_os = "windows"))]
    { vec![] }
}

fn get_ipv6_gateway() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        let raw = ps(
            "(Get-NetRoute -DestinationPrefix '::/0' -ErrorAction SilentlyContinue \
             | Sort-Object RouteMetric | Select-Object -First 1).NextHop"
        );
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() || trimmed == "::" { None } else { Some(trimmed) }
    }
    #[cfg(not(target_os = "windows"))]
    { None }
}

// ─── WAN IPv6 / ISP bilgisi ───────────────────────────────────────────────────

/// WAN IPv6 için IPv6-only endpoint'ler. ipinfo.io tek başına IPv4 fallback
/// üzerinden çözülürse IPv4 döndürebiliyordu (rapor maddesi); bu yüzden gerçek
/// WAN IPv6 yalnızca IPv6 üzerinden cevap veren kaynaklardan alınır ve doğrulanır.
const WAN_IPV6_ENDPOINTS: [&str; 3] = [
    "https://v6.ident.me",
    "https://api6.ipify.org",
    "https://v6.ipinfo.io/ip",
];

async fn fetch_wan_ipv6() -> (Option<String>, Option<String>) {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build() else { return (None, None); };

    // 1) Gerçek WAN IPv6'yı IPv6-only kaynaklardan al; dönen değer GERÇEKTEN
    //    global-routable IPv6 mı diye doğrula (IPv4/link-local reddet).
    let mut wan: Option<String> = None;
    for url in WAN_IPV6_ENDPOINTS {
        let Ok(resp) = client.get(url).send().await else { continue };
        let Ok(text) = resp.text().await else { continue };
        let candidate = text.trim();
        if let Ok(v6) = candidate.parse::<Ipv6Addr>() {
            let (_, addr_type) = classify_address(candidate);
            if is_globally_routable_type(&addr_type) {
                wan = Some(v6.to_string());
                break;
            }
        }
    }

    // 2) ISP/org bilgisini ipinfo'dan al (bu kısım IPv4 üzerinden de geçerli).
    #[derive(Deserialize)]
    struct Info {
        #[serde(default)] org: String,
    }
    let isp = match client.get("https://ipinfo.io/json").send().await {
        Ok(resp) => match resp.json::<Info>().await {
            Ok(info) => info
                .org.split_once(' ').map(|x| x.1)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            Err(_) => None,
        },
        Err(_) => None,
    };

    (wan, isp)
}

// ─── DNS IPv6 kontrolü ────────────────────────────────────────────────────────

/// IPv6 tasima probu: DNS'e BAGIMLI OLMADAN literal global IPv6 adreslerine TCP/443 baglantisi.
/// Cloudflare (2606:4700:4700::1111) ve Google (2001:4860:4860::8888); biri basarili olursa true.
async fn check_ipv6_transport() -> bool {
    use tokio::net::TcpStream;
    let probe = |addr: &'static str| async move {
        matches!(
            tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr)).await,
            Ok(Ok(_))
        )
    };
    let (a, b) = tokio::join!(
        probe("[2606:4700:4700::1111]:443"),
        probe("[2001:4860:4860::8888]:443"),
    );
    a || b
}

async fn check_dns_ipv6() -> bool {
    let result = tokio::task::spawn_blocking(|| {
        use std::net::ToSocketAddrs;
        // Dönen adresin yalnızca IPv6 olması yetmez — gerçekten global-routable
        // bir IPv6 olduğunu doğrula (loopback/link-local AAAA'ları sayma).
        ("ipv6.google.com", 80u16)
            .to_socket_addrs()
            .map(|it| {
                it.filter_map(|a| match a.ip() {
                    IpAddr::V6(v6) => Some(v6),
                    IpAddr::V4(_) => None,
                })
                .any(|v6| {
                    let (_, addr_type) = classify_address(&v6.to_string());
                    is_globally_routable_type(&addr_type)
                })
            })
            .unwrap_or(false)
    })
    .await;
    result.unwrap_or(false)
}

// ─── Bağlantı bilgisi topla ───────────────────────────────────────────────────

async fn get_connectivity_info() -> Ipv6ConnectivityInfo {
    // PowerShell çağrıları blocking — spawn_blocking ile Tokio thread'ini bloklamaktan kaçın.
    let (addresses, gateway_v6) = tokio::task::spawn_blocking(|| {
        (get_ipv6_addresses(), get_ipv6_gateway())
    }).await.unwrap_or_default();

    // Yalnizca global unicast (2000::/3) "IPv6 internet" sayilir; ULA (fc00::/7) sayilmaz.
    let has_ipv6 = addresses.iter().any(|a| a.scope == "Global");
    let has_native_ipv6 = addresses.iter().any(|a| a.address_type == "Native");
    let has_tunnel = addresses.iter().any(|a|
        matches!(a.address_type.as_str(), "Teredo" | "6to4" | "ISATAP" | "Tunnel")
    );
    let has_tunnel_only = has_tunnel && !has_native_ipv6;

    let dns_ipv6_works = if has_ipv6 { check_dns_ipv6().await } else { false };
    let ipv6_transport_works = if has_ipv6 { check_ipv6_transport().await } else { false };
    // IPv6 yoksa WAN sorgusunu atla
    let (mut wan_ipv6, isp_ipv6) = if has_ipv6 { fetch_wan_ipv6().await } else { (None, None) };

    // ipinfo.io IPv4 ile de çalışabilir — döndüğü IP IPv4 ise WAN IPv6 yok demek.
    // Fallback: local adresler arasındaki Global Native (geici olmayan) adresi kullan.
    if wan_ipv6.is_none() && has_native_ipv6 {
        wan_ipv6 = addresses.iter()
            .filter(|a| a.address_type == "Native" && a.scope == "Global" && !a.is_temporary)
            .map(|a| a.address.clone())
            .next();
    }

    // "Full": native + AAAA cozumleme + en az bir basarili IPv6 tasima probu.
    let connectivity_grade = if has_native_ipv6 && dns_ipv6_works && ipv6_transport_works {
        "Full"
    } else if has_tunnel_only {
        "Tunnel"
    } else if has_ipv6 {
        "Partial"
    } else {
        "None"
    }.to_string();

    crate::dlog!("[ipv6] grade={} native={} tunnel={} dns={}", connectivity_grade, has_native_ipv6, has_tunnel_only, dns_ipv6_works);

    Ipv6ConnectivityInfo {
        has_ipv6, has_native_ipv6, has_tunnel_only,
        addresses, gateway_v6, wan_ipv6, isp_ipv6,
        dns_ipv6_works, ipv6_transport_works, connectivity_grade,
    }
}

// ─── Kalite skoru hesapla ─────────────────────────────────────────────────────

fn calculate_quality(conn: &Ipv6ConnectivityInfo) -> Ipv6QualityScore {
    // Seçenek C: score connectivity_grade'den türetilir.
    // Full=100, Partial=50, Tunnel=25, None=0
    let score: u8 = match conn.connectivity_grade.as_str() {
        "Full"    => 100,
        "Partial" => 50,
        "Tunnel"  => 25,
        _         => 0,
    };

    // Grade eşikleri: Full native+DNS = Excellent asla Partial göstermez.
    let grade = match conn.connectivity_grade.as_str() {
        "Full"    => "Excellent",
        "Partial" => "Good",
        "Tunnel"  => "Poor",
        _         => "None",
    }.to_string();

    let isp_assessment = if !conn.has_ipv6 {
        "ipv6_assess_none"
    } else if conn.has_tunnel_only {
        "ipv6_assess_tunnel"
    } else if !conn.dns_ipv6_works || !conn.ipv6_transport_works {
        "ipv6_assess_partial"
    } else {
        "ipv6_assess_good"
    }.to_string();

    let mut recs: Vec<String> = Vec::new();

    if !conn.has_ipv6 {
        recs.push("ipv6_rec_no_ipv6".into());
    } else if conn.has_tunnel_only {
        recs.push("ipv6_rec_tunnel_only".into());
    }
    if conn.has_ipv6 && !conn.dns_ipv6_works {
        recs.push("ipv6_rec_dns_broken".into());
    }
    if conn.has_ipv6 && !conn.has_tunnel_only && !conn.ipv6_transport_works {
        recs.push("ipv6_rec_no_transport".into());
    }
    if conn.has_native_ipv6 && conn.dns_ipv6_works && conn.ipv6_transport_works {
        recs.push("ipv6_rec_good".into());
    }

    Ipv6QualityScore {
        score, grade,
        has_connectivity: conn.has_ipv6,
        has_native: conn.has_native_ipv6,
        dns_works: conn.dns_ipv6_works,
        isp_assessment,
        recommendations: recs,
    }
}

// ─── Ana Tauri Komutu ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_ipv6_test(app: AppHandle) -> Result<Ipv6TestResult, String> {
    crate::dlog!("[ipv6] test başlıyor");

    emit_progress(&app, "connectivity", "running", "IPv6 adresleri ve gateway tespit ediliyor…");
    let connectivity = get_connectivity_info().await;
    emit_progress(&app, "connectivity", "done", &connectivity.connectivity_grade);

    emit_progress(&app, "scoring", "running", "Kalite skoru hesaplanıyor…");
    let quality = calculate_quality(&connectivity);
    emit_progress(&app, "scoring", "done", &format!("Skor: {}/100 — {}", quality.score, quality.grade));

    crate::dlog!("[ipv6] tamamlandı — skor={} grade={}", quality.score, quality.grade);

    Ok(Ipv6TestResult { connectivity, quality })
}

// ── Testler ──────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn t(addr: &str) -> (String, String) {
        classify_address(addr)
    }

    #[test]
    fn test_loopback() {
        assert_eq!(t("::1").1, "Loopback");
    }

    #[test]
    fn test_link_local() {
        assert_eq!(t("fe80::1").0, "LinkLocal");
        assert_eq!(t("fe80::abcd:1234%12").0, "LinkLocal"); // zone-id ayıklanır
    }

    #[test]
    fn test_unique_local() {
        // ULA "Native" DEGIL; globally-routable da sayilmaz.
        assert_eq!(t("fd00::1"), ("UniqueLocal".into(), "UniqueLocal".into()));
        assert_eq!(t("fc00::1").0, "UniqueLocal");
        assert_eq!(t("fdff:ffff::1").1, "UniqueLocal");
        assert!(!is_globally_routable_type(&t("fd12:3456::1").1));
    }

    #[test]
    fn test_global_unicast_siniri() {
        // 2000::/3 sinirlari: 2000::/3 icinde Native; 4000::/ ve 1fff:: global unicast DEGIL
        assert_eq!(t("2000::1").1, "Native");
        assert_eq!(t("3fff:ffff::1").1, "Native");
        assert_eq!(t("4000::1").1, "Unknown");
        assert_eq!(t("1fff::1").1, "Unknown");
        // multicast da native degil
        assert_eq!(t("ff02::1").1, "Unknown");
    }

    #[test]
    fn test_teredo() {
        assert_eq!(t("2001:0:4136:e378:8000:63bf:3fff:fdd2").1, "Teredo");
    }

    #[test]
    fn test_he_tunnel_not_native() {
        // Kritik regresyon: HE 2001:470::/32 "Native" DEĞİL, "Tunnel" olmalı.
        assert_eq!(t("2001:470:0:76::2").1, "Tunnel");
        assert_ne!(t("2001:470:1f0e::2").1, "Native");
    }

    #[test]
    fn test_6to4() {
        assert_eq!(t("2002:c0a8:0101::1").1, "6to4");
    }

    #[test]
    fn test_isatap() {
        // ::0:5efe:x ve ::200:5efe:x ISATAP imzaları
        assert_eq!(t("fe80::5efe:192.168.1.1").0, "LinkLocal"); // link-local önce yakalanır
        assert_eq!(t("2001:db8::5efe:c0a8:101").1, "ISATAP");
        assert_eq!(t("2001:db8::200:5efe:c0a8:101").1, "ISATAP");
    }

    #[test]
    fn test_native_global() {
        assert_eq!(t("2606:4700:4700::1111").1, "Native");
        assert_eq!(t("2001:4860:4860::8888").1, "Native");
    }

    #[test]
    fn test_globally_routable() {
        assert!(is_globally_routable_type("Native"));
        assert!(is_globally_routable_type("Tunnel"));
        assert!(is_globally_routable_type("6to4"));
        assert!(!is_globally_routable_type("LinkLocal"));
        assert!(!is_globally_routable_type("Loopback"));
        assert!(!is_globally_routable_type("Unknown"));
    }
}

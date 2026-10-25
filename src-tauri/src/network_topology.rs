// Ağ topolojisi tespit yardımcıları:
//   - Default gateway (router) IP'si
//   - Local IP
//   - WAN IP + ISP adı (ipinfo.io)
//   - Gateway live ping (sürekli)

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::process::Command;

trait NoWindow {
    fn no_window(&mut self) -> &mut Self;
}

#[cfg(target_os = "windows")]
impl NoWindow for Command {
    fn no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x08000000);
        self
    }
}

#[cfg(not(target_os = "windows"))]
impl NoWindow for Command {
    fn no_window(&mut self) -> &mut Self { self }
}
use std::time::{Duration, Instant};
use surge_ping::{Client, Config, IcmpPacket, PingIdentifier, PingSequence, ICMP};
use tokio::time::timeout;

/// Sistemin default gateway IP'sini döndürür (0.0.0.0/0 route'unun NextHop'u).
/// Windows'ta `Get-NetRoute`, Unix'te `ip route` kullanılır.
pub fn detect_default_gateway() -> Option<IpAddr> {
    #[cfg(windows)]
    {
        detect_gateway_windows()
    }
    #[cfg(not(windows))]
    {
        detect_gateway_unix()
    }
}

#[cfg(windows)]
fn detect_gateway_windows() -> Option<IpAddr> {
    // PowerShell — DestinationPrefix 0.0.0.0/0 olan, en düşük RouteMetric'li route'un NextHop'u
    let output = Command::new(crate::powershell_exe())
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | \
             Sort-Object RouteMetric | Select-Object -First 1).NextHop",
        ])
        .no_window()
        .output()
        .ok()?;

    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if s.is_empty() || s == "0.0.0.0" {
        // Fallback — ipconfig'i parse et
        return detect_gateway_ipconfig();
    }
    s.parse().ok()
}

#[cfg(windows)]
fn detect_gateway_ipconfig() -> Option<IpAddr> {
    let output = Command::new(crate::win_system32("ipconfig.exe")).no_window().output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let lower = line.to_lowercase();
        if lower.contains("default gateway") || lower.contains("varsayılan ağ geçidi") {
            // "   Default Gateway . . . . . . . . . : 192.168.1.1"
            if let Some(ip_str) = line.split(':').nth(1) {
                let trimmed = ip_str.trim();
                if !trimmed.is_empty() {
                    if let Ok(ip) = trimmed.parse::<IpAddr>() {
                        return Some(ip);
                    }
                }
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn detect_gateway_unix() -> Option<IpAddr> {
    let output = Command::new("ip").args(["route", "show", "default"]).no_window().output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    // "default via 192.168.1.1 dev eth0 ..."
    for token in text.split_whitespace() {
        if let Ok(ip) = token.parse::<IpAddr>() {
            return Some(ip);
        }
    }
    None
}

// ── Public structs / commands ───────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopologyInfo {
    /// Birincil aktif NIC'in local IPv4 adresi.
    pub local_ip: String,
    /// Default gateway (router) IP'si.
    pub gateway_ip: String,
    /// Public (WAN) IP adresi.
    pub wan_ip: String,
    /// ISP / operatör adı (ipinfo.io'dan).
    pub isp: String,
    /// İki harfli ülke kodu (TR, DE, ...).
    pub country: String,
    /// Şehir adı.
    pub city: String,
}

/// Gateway'e tek bir ICMP ping atar, RTT'yi ms cinsinden döndürür.
/// ICMP izin yoksa TCP 80 ile fallback yapar.
/// Erişilemezse None döner.
#[tauri::command]
pub async fn ping_host(host: String, timeout_ms: Option<u64>) -> Result<Option<f64>, String> {
    let ip: IpAddr = host.parse().map_err(|_| "Invalid IP".to_string())?;
    let ms = icmp_single_ping_timeout(ip, timeout_ms.unwrap_or(500)).await;
    Ok(ms)
}

async fn icmp_single_ping_timeout(ip: IpAddr, timeout_ms: u64) -> Option<f64> {
    let config = Config::builder()
        .kind(match ip { IpAddr::V4(_) => ICMP::V4, IpAddr::V6(_) => ICMP::V6 })
        .build();
    let client = Client::new(&config).ok()?;
    let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
    pinger.timeout(Duration::from_millis(timeout_ms));
    let payload = [0u8; 16];
    let start = Instant::now();
    match timeout(
        Duration::from_millis(timeout_ms),
        pinger.ping(PingSequence(0), &payload),
    ).await {
        Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) => {
            Some(start.elapsed().as_secs_f64() * 1000.0)
        }
        _ => None,
    }
}

#[tauri::command]
pub async fn ping_gateway() -> Result<Option<f64>, String> {
    let gw = match detect_default_gateway() {
        Some(ip) => ip,
        None => return Ok(None),
    };

    let ms = icmp_single_ping(gw)
        .await
        .or_else(|| tcp_single_ping_gw(gw));

    Ok(ms)
}

/// Tek ICMP ping → RTT ms. Başarısız veya timeout → None.
async fn icmp_single_ping(ip: IpAddr) -> Option<f64> {
    let config = Config::builder()
        .kind(match ip { IpAddr::V4(_) => ICMP::V4, IpAddr::V6(_) => ICMP::V6 })
        .build();

    let client = Client::new(&config).ok()?;
    let mut pinger = client.pinger(ip, PingIdentifier(crate::rand_id())).await;
    pinger.timeout(Duration::from_millis(500));

    let payload = [0u8; 16];
    let start = Instant::now();
    match timeout(
        Duration::from_millis(500),
        pinger.ping(PingSequence(0), &payload),
    )
    .await
    {
        Ok(Ok((IcmpPacket::V4(_), _))) | Ok(Ok((IcmpPacket::V6(_), _))) => {
            Some(start.elapsed().as_secs_f64() * 1000.0)
        }
        _ => None,
    }
}

/// TCP 80 fallback — gateway ICMP'yi blokluyorsa.
fn tcp_single_ping_gw(ip: IpAddr) -> Option<f64> {
    use std::net::{TcpStream, SocketAddr};
    let addr = SocketAddr::new(ip, 80);
    let start = Instant::now();
    match TcpStream::connect_timeout(&addr, Duration::from_millis(500)) {
        Ok(_) => Some(start.elapsed().as_secs_f64() * 1000.0),
        Err(_) => None,
    }
}

/// Tüm topoloji bilgisini toplar ve tek seferde döndürür.
/// WAN/ISP için ipinfo.io'ya HTTP isteği atılır — internet bağlantısı gerektirir.
#[tauri::command]
pub async fn get_topology() -> Result<TopologyInfo, String> {
    let gateway_ip = detect_default_gateway()
        .map(|ip| ip.to_string())
        .unwrap_or_else(|| "—".to_string());

    let local_ip = detect_local_ip().unwrap_or_else(|| "—".to_string());

    // ipinfo.io/json → WAN IP + ISP + ülke + şehir
    let (wan_ip, isp, country, city) = fetch_wan_info().await;

    Ok(TopologyInfo { local_ip, gateway_ip, wan_ip, isp, country, city })
}

/// Sistemin birincil aktif NIC'inin local IPv4'ünü döndürür.
/// Loopback (127.x) ve APIPA (169.254.x) adreslerini atlar.
fn detect_local_ip() -> Option<String> {
    use std::net::UdpSocket;
    // UDP socket aç, gateway'e "bağlan" (paket gönderme yok) —
    // bu yöntem OS'un seçeceği kaynak IP'yi ortaya çıkarır.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

/// ipinfo.io/json'dan WAN IP, ISP, ülke ve şehir bilgisini çeker.
/// Hata durumunda alanlar "—" ile döner.
async fn fetch_wan_info() -> (String, String, String, String) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(6))
        .build()
    {
        Ok(c) => c,
        Err(_) => return ("—".into(), "—".into(), "—".into(), "—".into()),
    };

    let resp = match client
        .get("https://ipinfo.io/json")
        .header("Accept", "application/json")
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => return ("—".into(), "—".into(), "—".into(), "—".into()),
    };

    #[derive(Deserialize)]
    struct IpInfoResp {
        #[serde(default)] ip:      String,
        #[serde(default)] org:     String,   // "AS12345 Turk Telekom" formatı
        #[serde(default)] country: String,
        #[serde(default)] city:    String,
    }

    match resp.json::<IpInfoResp>().await {
        Ok(info) => {
            // org: "AS9121 Turk Telekomunikasyon" → sadece ISP adı al
            let isp = info.org.split_once(' ').map(|x| x.1)
                .unwrap_or(&info.org)
                .to_string();
            let isp = if isp.is_empty() { "—".to_string() } else { isp };
            (info.ip, isp, info.country, info.city)
        }
        Err(_) => ("—".into(), "—".into(), "—".into(), "—".into()),
    }
}

// ── Auto-update check ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub has_update: bool,
    pub download_url: String,
    pub release_notes: String,
}

#[derive(Deserialize)]
struct VersionResponse {
    version: String,
    url: Option<String>,
    notes: Option<String>,
}

fn version_parts(v: &str) -> Vec<u32> {
    v.split('.').filter_map(|s| s.parse().ok()).collect()
}

#[tauri::command]
pub async fn check_for_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let fallback = UpdateInfo {
        current_version: current.clone(),
        latest_version: current.clone(),
        has_update: false,
        download_url: "https://github.com/Extreme159/NetVeil/releases/latest".into(),
        release_notes: String::new(),
    };

    let Ok(client) = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()
        else { return Ok(fallback); };

    let Ok(resp) = client.get("https://api.netveil.net/version").send().await
        else { return Ok(fallback); };

    let Ok(vr) = resp.json::<VersionResponse>().await
        else { return Ok(fallback); };

    let has_update = version_parts(&vr.version) > version_parts(&current);

    crate::dlog!("[update] current={} latest={} has_update={}", current, vr.version, has_update);

    Ok(UpdateInfo {
        current_version: current,
        latest_version: vr.version,
        has_update,
        download_url: validated_update_url(vr.url.as_deref()),
        release_notes: vr.notes.unwrap_or_default(),
    })
}

const DEFAULT_UPDATE_URL: &str = "https://github.com/Extreme159/NetVeil/releases/latest";

/// `v0.9.19`, `v1.0.0-beta1` biçimli sürüm etiketi (nokta-dizisi yalnızca rakam; `..` geçmez).
fn is_release_version(s: &str) -> bool {
    let Some(rest) = s.strip_prefix('v') else { return false };
    let mut parts = rest.split('-');
    let core = parts.next().unwrap_or("");
    !core.is_empty()
        && core.split('.').all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        && parts.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Yayın dosyası adı: nokta-ayrık, boş olmayan `[A-Za-z0-9_-]` parçaları (`.`/`..` geçmez).
fn is_release_asset(s: &str) -> bool {
    s.split('.').all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

/// `/Extreme159/NetVeil/releases` + (`` | `/latest` | `/tag/vX` | `/download/vX/asset`) + isteğe bağlı `/`.
/// tauri.conf.json `plugins.shell.open` düzenli ifadesindeki github şekliyle aynı olmalı.
fn is_release_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("/Extreme159/NetVeil/releases") else { return false };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.is_empty() || rest == "/latest" {
        return true;
    }
    let segs: Vec<&str> = rest.split('/').collect();
    match segs.as_slice() {
        ["", "tag", v] => is_release_version(v),
        ["", "download", v, asset] => is_release_version(v) && is_release_asset(asset),
        _ => false,
    }
}

/// Sunucudan gelen güncelleme bağlantısını doğrular: yalnızca GitHub sürüm yolları
/// (`/releases`, `/releases/latest`, `/releases/tag/vX`, `/releases/download/vX/dosya`).
/// Url::parse yolu normalleştirir (`..`/`%2e%2e` segmentleri çözülür); host/şema/kimlik bilgisi/port/sorgu/parça kontrol edilir.
/// Geçersiz ya da yoksa sabit varsayılan sürüm sayfasına düşer (sunucu keyfi URL enjekte edemez).
fn validated_update_url(raw: Option<&str>) -> String {
    let Some(raw) = raw else { return DEFAULT_UPDATE_URL.into() };
    let Ok(u) = reqwest::Url::parse(raw.trim()) else { return DEFAULT_UPDATE_URL.into() };
    let path_ok = is_release_path(u.path());
    let ok = u.scheme() == "https"
        && u.query().is_none()
        && u.fragment().is_none()
        && u.host_str() == Some("github.com")
        && u.port().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && path_ok;
    if ok { u.into() } else { DEFAULT_UPDATE_URL.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_url_github_releases_kabul() {
        for u in [
            "https://github.com/Extreme159/NetVeil/releases/latest",
            "https://github.com/Extreme159/NetVeil/releases",
            "https://github.com/Extreme159/NetVeil/releases/tag/v0.9.19",
            "https://github.com/Extreme159/NetVeil/releases/tag/v1.0.0-beta1",
            "https://github.com/Extreme159/NetVeil/releases/download/v0.9.19/NetVeil-v0.9.19-portable.exe",
            "https://github.com/Extreme159/NetVeil/releases/",
        ] {
            assert_eq!(validated_update_url(Some(u)), u);
        }
    }

    #[test]
    fn update_url_gecersiz_varsayilana_duser() {
        for u in [
            "http://github.com/Extreme159/NetVeil/releases/latest",
            "https://evil.example/Extreme159/NetVeil/releases/latest",
            "https://github.com.evil.example/Extreme159/NetVeil/releases/latest",
            "https://github.com@evil.example/Extreme159/NetVeil/releases/latest",
            "https://user:pw@github.com/Extreme159/NetVeil/releases/latest",
            "https://github.com:8443/Extreme159/NetVeil/releases/latest",
            "https://github.com/Extreme159/NetVeil/releases-evil",
            "https://github.com/Extreme159/NetVeil/releases/../../evil/x",
            "https://github.com/Extreme159/NetVeil/releases/../../Other/Repo",
            "https://github.com/Extreme159/NetVeil/releases/%2e%2e/%2e%2e/Other/Repo",
            "https://github.com/Extreme159/NetVeil/releases/%2E%2E/Other",
            "https://github.com/Extreme159/NetVeil/releases/tag/v..",
            "https://github.com/Extreme159/NetVeil/releases/tag/v0.9.19/extra",
            "https://github.com/Extreme159/NetVeil/releases/tag/v0.9.19?x=1",
            "https://github.com/Extreme159/NetVeil/releases/latest#frag",
            "https://github.com/Extreme159/NetVeil/releases/download/v0.9.19/..",
            "https://github.com/Extreme159/NetVeil/releases/download/v0.9.19/a%2fb",
            "https://github.com/Extreme159/NetVeil/releases/download/v0.9.19/a..b",
            "https://github.com/Extreme159/NetVeil/releases/expanded_assets/v0.9.19",
            "https://github.com/Extreme159/NetVeil/releases//latest",
            "https://github.com/Other/NetVeil/releases/latest",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "ftp://github.com/Extreme159/NetVeil/releases",
            "not a url",
            "",
        ] {
            assert_eq!(validated_update_url(Some(u)), DEFAULT_UPDATE_URL, "{u}");
        }
        assert_eq!(validated_update_url(None), DEFAULT_UPDATE_URL);
    }
}

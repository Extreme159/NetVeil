use serde::{Deserialize, Serialize};
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WifiInfo {
    pub ssid: String,
    pub bssid: String,
    pub signal_percent: u32,
    pub signal_dbm: i32,
    pub quality: String,
    pub channel: u32,
    pub band: String,
    pub phy_type: String,
    pub tx_rate: String,
    pub rx_rate: String,
    pub auth: String,
    pub cipher: String,
    pub connected: bool,
}

#[tauri::command]
pub async fn get_wifi_info() -> Result<WifiInfo, String> {
    // netsh wlan show interfaces ile Wi-Fi bilgilerini al
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .no_window()
        .output()
        .map_err(|e| format!("netsh çalıştırılamadı: {}", e))?;

    let text = String::from_utf8_lossy(&output.stdout).to_string();

    if text.contains("There is no wireless interface")
        || text.contains("kablosuz arabirim yok")
        || text.trim().is_empty()
    {
        return Ok(WifiInfo {
            ssid: "Wi-Fi adaptörü bulunamadı".to_string(),
            bssid: "".to_string(),
            signal_percent: 0,
            signal_dbm: 0,
            quality: "N/A".to_string(),
            channel: 0,
            band: "N/A".to_string(),
            phy_type: "N/A".to_string(),
            tx_rate: "N/A".to_string(),
            rx_rate: "N/A".to_string(),
            auth: "N/A".to_string(),
            cipher: "N/A".to_string(),
            connected: false,
        });
    }

    let ssid = extract_netsh_field(&text, &["SSID", "SSID"]).unwrap_or_default();
    let bssid = extract_netsh_field(&text, &["BSSID"]).unwrap_or_default();
    let signal_str = extract_netsh_field(&text, &["Signal", "Señal", "Sinyal"]).unwrap_or_default();
    let channel_str = extract_netsh_field(&text, &["Channel", "Canal", "Kanal"]).unwrap_or_default();
    let phy_raw = extract_netsh_field(&text, &["Radio type", "Tipo de radio", "Radyo türü"]).unwrap_or_default();
    let tx_raw = extract_netsh_field_contains(&text, &["Transmit rate", "Velocidad de transmisión", "İletim hızı"]).unwrap_or_default();
    let rx_raw = extract_netsh_field_contains(&text, &["Receive rate", "Velocidad de recepción", "Alma hızı"]).unwrap_or_default();
    // Mbps birimi ekle / normalize et
    let fmt_rate = |s: &str| -> String {
        if s.is_empty() { return String::new(); }
        if s.contains("Mbps") || s.contains("Gbps") { s.to_string() }
        else { format!("{} Mbps", s) }
    };
    let tx_rate = fmt_rate(&tx_raw);
    let rx_rate = fmt_rate(&rx_raw);
    let auth = extract_netsh_field(&text, &["Authentication", "Autenticación", "Kimlik doğrulama"]).unwrap_or_default();
    let cipher = extract_netsh_field(&text, &["Cipher", "Cifrado", "Şifre"]).unwrap_or_default();

    // Sinyal % → dBm dönüşümü (Windows 0-100 arası verir)
    let signal_percent = signal_str
        .trim_end_matches('%')
        .trim()
        .parse::<u32>()
        .unwrap_or(0);

    // RSSI tahmini: Windows signal% = 2 * (dBm + 100) formülü
    let signal_dbm = ((signal_percent as f32 / 2.0) - 100.0) as i32;

    let channel = channel_str.trim().parse::<u32>().unwrap_or(0);

    let band = if channel > 14 {
        "5 GHz".to_string()
    } else if channel > 0 {
        "2.4 GHz".to_string()
    } else {
        "Bilinmiyor".to_string()
    };

    let phy_type = parse_phy_type(&phy_raw);
    let quality = signal_quality(signal_percent);
    let connected = !ssid.is_empty() && ssid != "Wi-Fi adaptörü bulunamadı";

    Ok(WifiInfo {
        ssid,
        bssid,
        signal_percent,
        signal_dbm,
        quality,
        channel,
        band,
        phy_type,
        tx_rate,
        rx_rate,
        auth,
        cipher,
        connected,
    })
}

/// Satır label'i key ile BITER mi diye bakar (tam eşleşme)
fn extract_netsh_field(text: &str, keys: &[&str]) -> Option<String> {
    for line in text.lines() {
        for key in keys {
            if line.contains(key) && line.contains(':') {
                // "SSID                   : MyNetwork" → "MyNetwork"
                // Sadece "SSID" eşleşsin, "BSSID" değil
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let label = parts[0].trim();
                    // Tam eşleşme kontrolü (SSID vs BSSID sorunu)
                    if label == *key || label.ends_with(&format!(" {}", key)) {
                        return Some(parts[1].trim().to_string());
                    }
                }
            }
        }
    }
    None
}

/// Label içinde key GEÇİYOR mu diye bakar — TX/RX gibi suffix'li alanlar için
fn extract_netsh_field_contains(text: &str, keys: &[&str]) -> Option<String> {
    for line in text.lines() {
        for key in keys {
            if line.contains(':') {
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    let label = parts[0].trim();
                    if label.to_lowercase().contains(&key.to_lowercase()) {
                        let val = parts[1].trim().to_string();
                        if !val.is_empty() { return Some(val); }
                    }
                }
            }
        }
    }
    None
}

fn parse_phy_type(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("802.11be") || lower.contains("wi-fi 7") {
        "Wi-Fi 7 (802.11be)".to_string()
    } else if lower.contains("802.11ax") || lower.contains("wi-fi 6") {
        "Wi-Fi 6/6E (802.11ax)".to_string()
    } else if lower.contains("802.11ac") || lower.contains("wi-fi 5") {
        "Wi-Fi 5 (802.11ac)".to_string()
    } else if lower.contains("802.11n") || lower.contains("wi-fi 4") {
        "Wi-Fi 4 (802.11n)".to_string()
    } else if lower.contains("802.11g") {
        "Wi-Fi 3 (802.11g)".to_string()
    } else if raw.is_empty() {
        "Bilinmiyor".to_string()
    } else {
        raw.to_string()
    }
}

fn signal_quality(percent: u32) -> String {
    match percent {
        80..=100 => "Mükemmel".to_string(),
        60..=79 => "İyi".to_string(),
        40..=59 => "Orta".to_string(),
        20..=39 => "Zayıf".to_string(),
        _ => "Çok Zayıf".to_string(),
    }
}

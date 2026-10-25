use serde::{Deserialize, Serialize};
use sysinfo::Networks;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NicInfo {
    pub name: String,
    pub mac: String,
    pub ip_addresses: Vec<String>,
    pub adapter_type: String,
    pub is_active: bool,
    pub received_bytes: u64,
    pub transmitted_bytes: u64,
    pub link_speed: Option<String>,
}

#[cfg(target_os = "windows")]
fn fetch_link_speeds() -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(o) = std::process::Command::new(crate::powershell_exe())
        .args(["-NoProfile", "-Command",
            "Get-NetAdapter | Select-Object Name,LinkSpeed | ConvertTo-Csv -NoTypeInformation"
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output() else { return map; };

    let text = String::from_utf8_lossy(&o.stdout);
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.splitn(2, ',').collect();
        if cols.len() == 2 {
            let name  = cols[0].trim().trim_matches('"').to_string();
            let speed = cols[1].trim().trim_matches('"').to_string();
            if !speed.is_empty() && speed != "0 bps" {
                map.insert(name.to_lowercase(), speed.clone());
                map.insert(name, speed);
            }
        }
    }
    map
}

#[cfg(not(target_os = "windows"))]
fn fetch_link_speeds() -> HashMap<String, String> {
    HashMap::new()
}

#[tauri::command]
pub async fn get_nic_info() -> Result<Vec<NicInfo>, String> {
    let networks = Networks::new_with_refreshed_list();
    let link_speeds = fetch_link_speeds();
    let mut adapters: Vec<NicInfo> = Vec::new();

    for (name, data) in &networks {
        let mac = data.mac_address().to_string();
        let ip_addresses: Vec<String> = data
            .ip_networks()
            .iter()
            .map(|ip| ip.addr.to_string())
            .collect();

        let adapter_type = detect_adapter_type(name);
        let is_active = data.received() > 0 || data.transmitted() > 0;

        // MAC 00:00:00 olanları atla (loopback vb.)
        if mac == "00:00:00:00:00:00" && name.to_lowercase().contains("loopback") {
            continue;
        }

        // Önce tam eşleşme, sonra lowercase eşleşme
        let link_speed = link_speeds.get(name)
            .or_else(|| link_speeds.get(&name.to_lowercase()))
            .cloned();
        adapters.push(NicInfo {
            name: name.clone(),
            mac,
            ip_addresses,
            adapter_type,
            is_active,
            received_bytes: data.received(),
            transmitted_bytes: data.transmitted(),
            link_speed,
        });
    }

    // Aktif olanlar önce
    adapters.sort_by(|a, b| b.is_active.cmp(&a.is_active));

    Ok(adapters)
}

fn detect_adapter_type(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower.contains("wi-fi")
        || lower.contains("wifi")
        || lower.contains("wireless")
        || lower.contains("wlan")
        || lower.contains("802.11")
    {
        "Wi-Fi".to_string()
    } else if lower.contains("vpn")
        || lower.contains("tun")
        || lower.contains("tap")
        || lower.contains("nordvpn")
        || lower.contains("expressvpn")
        || lower.contains("proton")
    {
        "VPN".to_string()
    } else if lower.contains("loopback") || lower.contains("lo") {
        "Loopback".to_string()
    } else if lower.contains("vmware")
        || lower.contains("virtualbox")
        || lower.contains("hyper-v")
        || lower.contains("vethernet")
    {
        "Virtual".to_string()
    } else if lower.contains("bluetooth") {
        "Bluetooth".to_string()
    } else {
        "Ethernet".to_string()
    }
}

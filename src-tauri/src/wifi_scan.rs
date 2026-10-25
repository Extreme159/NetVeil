use serde::{Deserialize, Serialize};
use std::process::Command;

#[cfg(target_os = "windows")]
use windows::Win32::{
    Foundation::HANDLE,
    NetworkManagement::WiFi::{
        dot11_BSS_type_any, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
        WlanGetNetworkBssList, WlanOpenHandle, WlanRegisterNotification, WlanScan,
        WLAN_BSS_LIST, WLAN_INTERFACE_INFO_LIST, WLAN_NOTIFICATION_SOURCE_ACM,
        WLAN_NOTIFICATION_SOURCE_NONE,
        wlan_notification_acm_scan_complete, wlan_notification_acm_scan_fail,
        L2_NOTIFICATION_DATA,
    },
};

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
    fn no_window(&mut self) -> &mut Self {
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AccessPoint {
    pub ssid: String,
    pub bssid: String,
    pub rssi: i32,
    pub signal_quality: u32,
    pub channel: u32,
    pub center_freq_mhz: u32,
    pub band: String,
    pub channel_width: String,
    pub phy_type: String,
    pub is_connected: bool,
    /// Rust tarafı tek doğruluk kaynağı: bağlı AP ile aynı fiziksel cihaz / aynı SSID (is_own_ap).
    pub is_own: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ChannelRecommendation {
    pub band: String,
    pub current_channel: u32,
    pub recommended_channel: u32,
    pub reason: String,
    pub congestion_score: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WifiScanResult {
    pub access_points: Vec<AccessPoint>,
    pub current_ssid: String,
    pub current_bssid: String,
    pub current_channel: u32,
    pub current_band: String,
    pub recommendations: Vec<ChannelRecommendation>,
    pub interface_found: bool,
}

// ── Frekans → Kanal + Band ──────────────────────────────────────────────────

fn freq_to_channel_band(freq_khz: u32) -> (u32, String) {
    let freq_mhz = freq_khz / 1000;
    match freq_mhz {
        2412..=2484 => {
            let ch = if freq_mhz == 2484 {
                14
            } else {
                (freq_mhz.saturating_sub(2407)) / 5
            };
            (ch.clamp(1, 14), "2.4 GHz".to_string())
        }
        5160..=5885 => {
            let ch = (freq_mhz.saturating_sub(5000)) / 5;
            (ch, "5 GHz".to_string())
        }
        5925..=7125 => {
            let ch = if freq_mhz == 5935 {
                2
            } else {
                (freq_mhz.saturating_sub(5950)) / 5
            };
            (ch, "6 GHz".to_string())
        }
        _ => (0, "Unknown".to_string()),
    }
}

// ── PHY Type ────────────────────────────────────────────────────────────────

fn phy_type_str(phy: u32) -> String {
    match phy {
        11 => "Wi-Fi 7 (802.11be)".to_string(),
        10 => "Wi-Fi 6/6E (802.11ax)".to_string(),
        8  => "Wi-Fi 5 (802.11ac)".to_string(),
        7  => "Wi-Fi 4 (802.11n)".to_string(),
        6  => "Wi-Fi 3 (802.11g)".to_string(),
        5  => "Wi-Fi 2 (802.11a)".to_string(),
        4  => "Wi-Fi 1 (802.11b)".to_string(),
        n  => format!("Unknown (type {})", n),
    }
}

// ── IE Blob → gerçek kanal genişliği ────────────────────────────────────────
// IEEE 802.11 Information Elements:
//   IE 61 (0x3D) — HT Operation: secondary_channel_offset bits[1:0] of data[1]
//                  0=none(20MHz)  1=above(HT40+)  3=below(HT40-)
//   IE 192 (0xC0) — VHT Operation: data[0] chan_width
//                   0=20/40  1=80MHz  2=160MHz(legacy)  3=80+80MHz
//   IE 255 ext36 — HE Operation: 6GHz control byte bits[1:0]
//                  0=20  1=40  2=80  3=160
//   IE 255 ext106 — EHT Operation (Wi-Fi 7): 320 MHz

fn parse_channel_width(ie_data: &[u8]) -> String {
    let mut ht_sec_offset: Option<u8> = None;
    let mut vht_width: Option<u8> = None;
    let mut he_6ghz_width: Option<u8> = None;
    let mut eht_width: Option<u8> = None;

    let mut i = 0usize;
    while i + 1 < ie_data.len() {
        let id = ie_data[i];
        let len = ie_data[i + 1] as usize;
        i += 2;
        if i + len > ie_data.len() {
            break;
        }
        let data = &ie_data[i..i + len];
        i += len;

        match id {
            // HT Operation (802.11n)
            61 if len >= 3 => {
                ht_sec_offset = Some(data[1] & 0x03);
            }
            // VHT Operation (802.11ac)
            192 if len >= 3 => {
                vht_width = Some(data[0]);
            }
            // Extended IEs
            255 if len >= 1 => {
                let ext_id = data[0];
                match ext_id {
                    // HE Operation (802.11ax)
                    36 if len >= 7 => {
                        let op_params = (data[1] as u32)
                            | ((data[2] as u32) << 8)
                            | ((data[3] as u32) << 16);
                        let vht_present   = (op_params & (1 << 14)) != 0;
                        let cohosted      = (op_params & (1 << 15)) != 0;
                        let sixghz_present = (op_params & (1 << 17)) != 0;
                        let mut off = 7usize; // ext_id(1)+op_params(3)+bss_color(1)+basic_mcs(2)
                        if vht_present { off += 3; }
                        if cohosted    { off += 1; }
                        if sixghz_present && len >= off + 5 {
                            // 6 GHz Operation Info: primary(1) + control(1) + seg0(1) + seg1(1) + minrate(1)
                            let control = data[off + 1];
                            he_6ghz_width = Some(control & 0x03);
                        }
                    }
                    // EHT Operation (802.11be / Wi-Fi 7)
                    // data[1] = EHT Operation Parameters (bit0 = EHT Operation Information
                    // Present), data[2..6] = Basic EHT-MCS And Nss Set (4 bayt), ardindan
                    // EHT Operation Information: Control(1) CCFS0(1) CCFS1(1).
                    // Control bits[2:0]: 0=20, 1=40, 2=80, 3=160, 4=320 MHz (5-7 reserved).
                    106 if len >= 9 && (data[1] & 0x01) != 0 => {
                        let w = data[6] & 0x07;
                        if w <= 4 {
                            eht_width = Some(w);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    // Öncelik: EHT > HE 6GHz > VHT > HT
    if let Some(w) = eht_width {
        return match w {
            4 => "320 MHz".to_string(),
            3 => "160 MHz".to_string(),
            2 => "80 MHz".to_string(),
            1 => "40 MHz".to_string(),
            _ => "20 MHz".to_string(),
        };
    }
    if let Some(w) = he_6ghz_width {
        return match w {
            3 => "160 MHz".to_string(),
            2 => "80 MHz".to_string(),
            1 => "40 MHz".to_string(),
            _ => "20 MHz".to_string(),
        };
    }
    if let Some(w) = vht_width {
        return match w {
            1 => "80 MHz".to_string(),
            2 | 3 => "160 MHz".to_string(),
            _ => {
                // chan_width=0 → 20 veya 40 → HT Op'a bak
                if ht_sec_offset.unwrap_or(0) != 0 {
                    "40 MHz".to_string()
                } else {
                    "20 MHz".to_string()
                }
            }
        };
    }
    // Sadece HT Op var
    if let Some(sec) = ht_sec_offset {
        if sec != 0 {
            return "40 MHz".to_string();
        }
    }
    "20 MHz".to_string()
}

// ── Mevcut bağlantı (SSID + BSSID) netsh ile ────────────────────────────────

struct CurrentConnection {
    ssid: String,
    bssid: String,
}

fn get_current_connection() -> CurrentConnection {
    let Ok(output) = Command::new(crate::win_system32("netsh.exe"))
        .args(["wlan", "show", "interfaces"])
        .no_window()
        .output()
    else {
        return CurrentConnection { ssid: String::new(), bssid: String::new() };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut ssid = String::new();
    let mut bssid = String::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if ssid.is_empty() {
            if let Some(rest) = trimmed.strip_prefix("SSID") {
                if !rest.trim_start().starts_with('I') {
                    if let Some(val) = rest.trim_start().strip_prefix(':') {
                        ssid = val.trim().to_string();
                    }
                }
            }
        }
        if bssid.is_empty() {
            if let Some(rest) = trimmed.strip_prefix("BSSID") {
                if let Some(val) = rest.trim_start().strip_prefix(':') {
                    let raw = val.trim().to_uppercase();
                    if raw.len() == 17 {
                        bssid = raw;
                    }
                }
            }
        }
        if !ssid.is_empty() && !bssid.is_empty() { break; }
    }
    CurrentConnection { ssid, bssid }
}

// ── Aynı fiziksel cihaz tespiti ─────────────────────────────────────────────
// IEEE 802.11-2020 §11.1.4: Virtual BSS / MBSSID — aynı radyo zinciri.
// Üretici (OUI) eşitliği sahiplik değildir: komşunun aynı marka router'ı
// tıkanıklıktan düşmemeli. Bu yüzden OUI yerine dar bir sanal-BSSID kuralı:
//   - BSSID'nin orta 4 baytı (b1..b4) aynı olmalı;
//   - ilk bayt aynı VEYA yalnızca bit 1-3'te farklı ve en az birinde locally-administered
//     biti (0x02) set (multicast biti ikisinde de temiz);
//     (hostapd/OpenWrt sanal BSS'leri ilk baytı 0x02 | (i<<2) ile türetir);
//   - son bayt serbest (aynı radyonun farklı BSS / band'ları).
// Ek kural: bağlı SSID ile aynı ismi taşıyan AP'ler hariç (mesh node / sanal BSS).
pub(crate) fn parse_bssid(s: &str) -> Option<[u8; 6]> {
    let mut out = [0u8; 6];
    let mut n = 0usize;
    for part in s.trim().split(|c| c == ':' || c == '-') {
        if n >= 6 || part.len() != 2 {
            return None;
        }
        out[n] = u8::from_str_radix(part, 16).ok()?;
        n += 1;
    }
    if n == 6 { Some(out) } else { None }
}

/// İki BSSID aynı fiziksel AP'ye (aynı radyo/sanal BSS) ait mi? Bağlı BSSID'nin
/// kendisi de true döner. Ayrıştırılamayan BSSID → false (hariç tutma yapılmaz).
pub(crate) fn is_same_device_bssid(a: &str, b: &str) -> bool {
    let (Some(x), Some(y)) = (parse_bssid(a), parse_bssid(b)) else {
        return false;
    };
    if x[1..5] != y[1..5] {
        return false;
    }
    // Multicast biti ikisinde de temiz; ilk baytlar yalnızca bit 1-3'te (LA biti +
    // sanal-BSS indeksi) farklılaşabilir ve farklıysa en az birinde LA biti set olmalı.
    if (x[0] | y[0]) & 0x01 != 0 || (x[0] ^ y[0]) & !0x0E != 0 {
        return false;
    }
    x[0] == y[0] || ((x[0] | y[0]) & 0x02) != 0
}

fn is_own_ap(ap: &AccessPoint, own_ssid: &str, own_bssid: &str) -> bool {
    is_same_device_bssid(&ap.bssid, own_bssid)
        || (!own_ssid.is_empty() && ap.ssid == own_ssid)
}

// ── Congestion Score ──────────────────────────────────────────────────────────
fn congestion_24(aps: &[AccessPoint], ch: u32, own_ssid: &str, own_bssid: &str) -> u32 {
    aps.iter()
        .filter(|ap| ap.band == "2.4 GHz")
        .filter(|ap| !is_own_ap(ap, own_ssid, own_bssid))
        .filter(|ap| (ap.channel as i32 - ch as i32).abs() <= 4)
        .map(|ap| {
            let base = (ap.rssi + 100).max(0) as u32;
            let diff = (ap.channel as i32 - ch as i32).abs();
            // Co-channel (diff==0) EN ZARARLI: CSMA/CA aynı kanalı paylaşır (tam çakışma).
            // ACI (adjacent, diff>0) daha az zararlı ve mesafeyle azalır (sinyal sızıntısı).
            if diff == 0 { base } else { base / 2 }  // ACI yarı ağırlık
        })
        .sum()
}

fn congestion_5_6(aps: &[AccessPoint], band: &str, ch: u32, own_ssid: &str, own_bssid: &str) -> u32 {
    aps.iter()
        .filter(|ap| ap.band == band && ap.channel == ch)
        .filter(|ap| !is_own_ap(ap, own_ssid, own_bssid))
        .map(|ap| (ap.rssi + 100).max(0) as u32)
        .sum()
}

// 5 GHz 20 MHz öneri skoru: AP'nin bonding grubunu da hesaba katar.
// CH40'ı değerlendirirken 80 MHz AP [36,40,44,48]'in CH40'ı işgal ettiğini görür.
fn congestion_5_20(aps: &[AccessPoint], ch: u32, own_ssid: &str, own_bssid: &str) -> u32 {
    aps.iter()
        .filter(|ap| ap.band == "5 GHz")
        .filter(|ap| !is_own_ap(ap, own_ssid, own_bssid))
        .filter(|ap| {
            let width: u32 = if ap.channel_width.starts_with("160") { 160 }
                else if ap.channel_width.starts_with("80") { 80 }
                else if ap.channel_width.starts_with("40") { 40 }
                else { 20 };
            if width == 20 {
                ap.channel == ch
            } else {
                bonding_group_5(ap.channel, width).contains(&ch)
            }
        })
        .map(|ap| (ap.rssi + 100).max(0) as u32)
        .sum()
}


// ── Channel Bonding Grupları (5 GHz) ─────────────────────────────────────────
// IEEE 802.11-2020 Annex E Table E-1

fn bonding_group_5(primary: u32, width_mhz: u32) -> Vec<u32> {
    let groups_40: &[&[u32]] = &[
        &[36,40], &[44,48], &[52,56], &[60,64],
        &[100,104], &[108,112], &[116,120], &[124,128],
        &[132,136], &[140,144], &[149,153], &[157,161],
    ];
    let groups_80: &[&[u32]] = &[
        &[36,40,44,48], &[52,56,60,64],
        &[100,104,108,112], &[116,120,124,128],
        &[132,136,140,144], &[149,153,157,161],
    ];
    let groups_160: &[&[u32]] = &[
        &[36,40,44,48,52,56,60,64],
        &[100,104,108,112,116,120,124,128],
    ];
    let groups: &[&[u32]] = match width_mhz {
        160 => groups_160,
        80  => groups_80,
        _   => groups_40,
    };
    for g in groups {
        if g.contains(&primary) { return g.to_vec(); }
    }
    vec![primary]
}

fn congestion_group(aps: &[AccessPoint], band: &str, group: &[u32], own_ssid: &str, own_bssid: &str) -> u32 {
    group.iter().map(|&ch| congestion_5_6(aps, band, ch, own_ssid, own_bssid)).sum()
}

// Bonding "STAY" karar
// Mevcut kanalin GRUBU temizse (skor 0) veya en iyi alternatif HYSTERESIS kadar
// daha iyi degilse mevcut grubun primary'si korunur; aksi halde en iyi alternatif.
fn bonded_recommendation(
    aps: &[AccessPoint],
    ch_now: u32,
    width_mhz: u32,
    primaries: &[u32],
    best: (u32, u32),
    own_ssid: &str,
    own_bssid: &str,
) -> u32 {
    if ch_now == 0 {
        return best.0;
    }
    let Some(cur_primary) = primaries
        .iter()
        .copied()
        .find(|&p| bonding_group_5(p, width_mhz).contains(&ch_now))
    else {
        return best.0;
    };
    let cur_score = congestion_group(aps, "5 GHz", &bonding_group_5(cur_primary, width_mhz), own_ssid, own_bssid);
    if cur_score == 0 || best.1.saturating_add(HYSTERESIS) > cur_score {
        cur_primary
    } else {
        best.0
    }
}


// Don't-churn hysteresis: mevcut skor 0 (temiz) veya aday bu kadar daha iyi
// değilse STAY öner. Kaynak: PulseGeek, Cisco RRM, Aruba — docs/research/wifi-channel-standards.md §5.
const HYSTERESIS: u32 = 15;

// ── Öneri Algoritması — tüm aktif bandlar ────────────────────────────────────
// Bağlı SSID olsa da olmasa da her aktif band için öneri üretir.
// current_bssid: aynı cihaz (sanal BSSID) hariç tutması için kullanılır; boşsa BSSID bazlı hariç tutma yapılmaz.

fn build_recommendations(
    aps: &[AccessPoint],
    current_ch: u32,
    current_band: &str,
    current_bssid: &str,
    current_ssid: &str,
) -> Vec<ChannelRecommendation> {
    let own_bssid = current_bssid;
    let own_ssid = current_ssid;
    let mut recs = Vec::new();

    let has_24 = aps.iter().any(|ap| ap.band == "2.4 GHz");
    let has_5  = aps.iter().any(|ap| ap.band == "5 GHz");
    let has_6  = aps.iter().any(|ap| ap.band == "6 GHz");

    // 2.4 GHz ──────────────────────────────────────────────────────────────────
    if has_24 {
        let ch_now = if current_band == "2.4 GHz" { current_ch } else { 0 };
        let candidates = [1u32, 6, 11];
        let best = candidates.iter()
            .map(|&ch| (ch, congestion_24(aps, ch, own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((1, 0));
        let current_score = if ch_now > 0 { congestion_24(aps, ch_now, own_ssid, own_bssid) } else { 0 };
        // Don't-churn: temiz (skor=0) veya aday yeterince daha iyi değilse STAY
        let rec_24 = if ch_now > 0 && (current_score == 0 || best.1.saturating_add(HYSTERESIS) > current_score) {
            ch_now
        } else {
            best.0
        };
        recs.push(ChannelRecommendation {
            band: "2.4 GHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_24,
            reason: "IEEE 802.11 non-overlapping (1/6/11). Same-device virtual BSSIDs excluded.".to_string(),
            congestion_score: current_score,
        });
    }

    // 5 GHz ────────────────────────────────────────────────────────────────────
    if has_5 {
        let ch_now = if current_band == "5 GHz" { current_ch } else { 0 };
        let non_dfs: &[u32] = &[36, 40, 44, 48, 149, 153, 157, 161, 165];
        let all_5: &[u32] = &[
            36, 40, 44, 48, 52, 56, 60, 64,
            100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
            149, 153, 157, 161, 165,
        ];
        let primaries_40: &[u32] = &[36, 44, 52, 60, 100, 108, 116, 124, 132, 140, 149, 157];
        let primaries_80: &[u32] = &[36, 52, 100, 116, 132, 149];
        let primaries_160: &[u32] = &[36, 100];

        let best_20_nondfs = non_dfs.iter()
            .map(|&ch| (ch, congestion_5_20(aps, ch, own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((36, 0));
        let best_20_any = all_5.iter()
            .map(|&ch| (ch, congestion_5_20(aps, ch, own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((36, 0));
        // Non-DFS tie-break: DFS ancak >20 puan avantajlıysa seçilir
        let best_20 = if best_20_any.1 < best_20_nondfs.1.saturating_sub(20) { best_20_any } else { best_20_nondfs };
        let current_score_20 = if ch_now > 0 { congestion_5_20(aps, ch_now, own_ssid, own_bssid) } else { 0 };

        let best_40 = primaries_40.iter()
            .map(|&p| (p, congestion_group(aps, "5 GHz", &bonding_group_5(p, 40), own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((36, 0));
        let best_80 = primaries_80.iter()
            .map(|&p| (p, congestion_group(aps, "5 GHz", &bonding_group_5(p, 80), own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((36, 0));
        let best_160 = primaries_160.iter()
            .map(|&p| (p, congestion_group(aps, "5 GHz", &bonding_group_5(p, 160), own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((36, 0));

        // Don't-churn 20 MHz: temiz veya aday yeterince daha iyi değilse STAY
        let rec_5_20 = if ch_now > 0 && (current_score_20 == 0 || best_20.1.saturating_add(HYSTERESIS) > current_score_20) {
            ch_now
        } else {
            best_20.0
        };
        // Don't-churn bonding kartları: MEVCUT kanalın grubu temizse veya en iyi alternatif
        // yeterince daha iyi değilse STAY (alternatifin temiz olması karar vermez).
        let rec_5_40 = bonded_recommendation(aps, ch_now, 40, primaries_40, best_40, own_ssid, own_bssid);
        let rec_5_80 = bonded_recommendation(aps, ch_now, 80, primaries_80, best_80, own_ssid, own_bssid);
        let rec_5_160 = bonded_recommendation(aps, ch_now, 160, primaries_160, best_160, own_ssid, own_bssid);
        recs.push(ChannelRecommendation {
            band: "5 GHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_5_20,
            reason: format!("20 MHz | Non-DFS preferred (UNII-1/3). Score: {}", best_20.1),
            congestion_score: current_score_20,
        });
        recs.push(ChannelRecommendation {
            band: "5 GHz 40MHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_5_40,
            reason: format!("40 MHz | Primary CH {} group {:?}. Score: {}", best_40.0, bonding_group_5(best_40.0, 40), best_40.1),
            congestion_score: best_40.1,
        });
        recs.push(ChannelRecommendation {
            band: "5 GHz 80MHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_5_80,
            reason: format!("80 MHz | Primary CH {} group {:?}. Score: {}", best_80.0, bonding_group_5(best_80.0, 80), best_80.1),
            congestion_score: best_80.1,
        });
        recs.push(ChannelRecommendation {
            band: "5 GHz 160MHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_5_160,
            reason: format!("160 MHz | Primary CH {} group {:?}. Score: {}", best_160.0, bonding_group_5(best_160.0, 160), best_160.1),
            congestion_score: best_160.1,
        });
    }

    // 6 GHz ────────────────────────────────────────────────────────────────────
    if has_6 {
        let ch_now = if current_band == "6 GHz" { current_ch } else { 0 };
        let psc: &[u32] = &[5, 21, 37, 53, 69, 85, 101, 117, 133, 149, 165, 181, 197, 213, 229];
        let psc_160: &[u32] = &[5, 37, 69, 101, 133, 165, 197];

        let best_6_80 = psc.iter()
            .map(|&ch| (ch, congestion_5_6(aps, "6 GHz", ch, own_ssid, own_bssid)))
            .min_by_key(|s| s.1).unwrap_or((5, 0));
        // 6 GHz 160 MHz = 4 ardışık PSC kanalı: p, p+16, p+32, p+48
        let best_6_160 = psc_160.iter()
            .map(|&p| {
                let score = [p, p + 16, p + 32, p + 48].iter()
                    .map(|&c| congestion_5_6(aps, "6 GHz", c, own_ssid, own_bssid))
                    .sum::<u32>();
                (p, score)
            })
            .min_by_key(|s| s.1).unwrap_or((5, 0));

        let current_score = if ch_now > 0 { congestion_5_6(aps, "6 GHz", ch_now, own_ssid, own_bssid) } else { 0 };
        // Don't-churn 6 GHz 80: temiz veya aday yeterince daha iyi değilse STAY
        let rec_6_80 = if ch_now > 0 && (current_score == 0 || best_6_80.1.saturating_add(HYSTERESIS) > current_score) {
            ch_now
        } else {
            best_6_80.0
        };
        // Don't-churn 6 GHz 160: current_score 0 veya aday grubun skoru yeterli avantaj sağlamıyorsa STAY
        let current_score_6_160 = if ch_now > 0 {
            [ch_now, ch_now.saturating_add(16), ch_now.saturating_add(32), ch_now.saturating_add(48)]
                .iter().map(|&c| congestion_5_6(aps, "6 GHz", c, own_ssid, own_bssid)).sum::<u32>()
        } else { 0 };
        let rec_6_160 = if ch_now > 0 && (current_score_6_160 == 0 || best_6_160.1.saturating_add(HYSTERESIS) > current_score_6_160) {
            psc_160.iter().copied().find(|&p| {
                [p, p + 16, p + 32, p + 48].contains(&ch_now)
            }).unwrap_or(best_6_160.0)
        } else {
            best_6_160.0
        };
        recs.push(ChannelRecommendation {
            band: "6 GHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_6_80,
            reason: "80 MHz | IEEE 802.11ax PSC. Same-device virtual BSSIDs excluded.".to_string(),
            congestion_score: current_score,
        });
        recs.push(ChannelRecommendation {
            band: "6 GHz 160MHz".to_string(),
            current_channel: ch_now,
            recommended_channel: rec_6_160,
            reason: format!("160 MHz | PSC primary CH {}. Score: {}", best_6_160.0, best_6_160.1),
            congestion_score: best_6_160.1,
        });
    }

    recs
}


// ── Ana Tauri Komutu ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_wifi_scan() -> Result<WifiScanResult, String> {
    tokio::task::spawn_blocking(run_wifi_scan_sync)
        .await
        .map_err(|e| format!("Thread error: {}", e))?
}

// ── Windows Implementasyonu ──────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn run_wifi_scan_sync() -> Result<WifiScanResult, String> {
    let conn = get_current_connection();
    let current_ssid = conn.ssid;
    let current_bssid = conn.bssid;

    let mut handle: HANDLE = HANDLE::default();
    let mut negotiated: u32 = 0;
    let ret = unsafe { WlanOpenHandle(2, None, &mut negotiated, &mut handle) };
    if ret != 0 {
        return Ok(WifiScanResult {
            access_points: vec![],
            current_ssid: String::new(),
            current_bssid: String::new(),
            current_channel: 0,
            current_band: String::new(),
            recommendations: vec![],
            interface_found: false,
        });
    }

    let mut iface_list: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
    let ret = unsafe { WlanEnumInterfaces(handle, None, &mut iface_list) };
    if ret != 0 || iface_list.is_null() {
        unsafe { WlanCloseHandle(handle, None) };
        return Ok(WifiScanResult {
            access_points: vec![],
            current_ssid: String::new(),
            current_bssid: String::new(),
            current_channel: 0,
            current_band: String::new(),
            recommendations: vec![],
            interface_found: false,
        });
    }

    let iface_count = unsafe { (*iface_list).dwNumberOfItems };
    if iface_count == 0 {
        unsafe {
            WlanFreeMemory(iface_list as *mut _);
            WlanCloseHandle(handle, None);
        }
        return Ok(WifiScanResult {
            access_points: vec![],
            current_ssid: String::new(),
            current_bssid: String::new(),
            current_channel: 0,
            current_band: String::new(),
            recommendations: vec![],
            interface_found: false,
        });
    }

    let interfaces = unsafe {
        std::slice::from_raw_parts((*iface_list).InterfaceInfo.as_ptr(), iface_count as usize)
    };

    let mut access_points: Vec<AccessPoint> = Vec::new();
    let mut current_channel = 0u32;
    let mut current_band = String::new();

    for iface in interfaces {
        let guid = &iface.InterfaceGuid;

        // ── Scan tamamlanana kadar notification ile bekle ──────────────────
        // WlanRegisterNotification + event flag — max 4 saniye (driver logo spec)
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let scan_done = Arc::new(AtomicBool::new(false));
        let scan_done_cb = scan_done.clone();

        // Callback: scan_complete veya scan_fail gelince flag'i set et
        // WLAN_NOTIFICATION_CALLBACK imzası: fn(*mut L2_NOTIFICATION_DATA, *mut c_void)
        unsafe extern "system" fn scan_callback(
            notif_data: *mut L2_NOTIFICATION_DATA,
            ctx: *mut core::ffi::c_void,
        ) {
            if notif_data.is_null() || ctx.is_null() {
                return;
            }
            let code = (*notif_data).NotificationCode;
            let done = &*(ctx as *const AtomicBool);
            if code == wlan_notification_acm_scan_complete.0 as u32
                || code == wlan_notification_acm_scan_fail.0 as u32
            {
                done.store(true, Ordering::Release);
            }
        }

        let ctx_ptr = Arc::as_ptr(&scan_done_cb) as *const core::ffi::c_void;
        let _ = unsafe {
            WlanRegisterNotification(
                handle,
                WLAN_NOTIFICATION_SOURCE_ACM,
                false,
                Some(scan_callback),
                Some(ctx_ptr),
                None,
                None,
            )
        };

        // Taze tarama başlat
        let _ = unsafe { WlanScan(handle, guid, None, None, None) };

        // scan_complete veya 4 saniye timeout bekle
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(4000);
        while !scan_done.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        // Notification kaydını kaldır
        let _ = unsafe {
            WlanRegisterNotification(handle, WLAN_NOTIFICATION_SOURCE_NONE, false, None, None, None, None)
        };

        // BSS listesini al
        let mut bss_list: *mut WLAN_BSS_LIST = std::ptr::null_mut();
        let ret = unsafe {
            WlanGetNetworkBssList(handle, guid, None, dot11_BSS_type_any, false, None, &mut bss_list)
        };
        if ret != 0 || bss_list.is_null() {
            continue;
        }

        let bss_count = unsafe { (*bss_list).dwNumberOfItems } as usize;
        let entries = unsafe {
            std::slice::from_raw_parts((*bss_list).wlanBssEntries.as_ptr(), bss_count)
        };

        for entry in entries {
            let ssid_len = entry.dot11Ssid.uSSIDLength as usize;
            let ssid_bytes = &entry.dot11Ssid.ucSSID[..ssid_len.min(32)];
            let ssid = String::from_utf8_lossy(ssid_bytes).into_owned();

            if ssid.trim().is_empty() {
                continue;
            }

            let b = &entry.dot11Bssid;
            let bssid = format!(
                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                b[0], b[1], b[2], b[3], b[4], b[5]
            );

            let rssi = entry.lRssi;
            let signal_quality = entry.uLinkQuality;
            let freq_khz = entry.ulChCenterFrequency;
            let (channel, band) = freq_to_channel_band(freq_khz);
            let center_freq_mhz = freq_khz / 1000;

            // IE blob'dan gerçek kanal genişliğini parse et
            let channel_width = if entry.ulIeSize > 0 {
                let ie_ptr = unsafe {
                    (entry as *const _ as *const u8).add(entry.ulIeOffset as usize)
                };
                let ie_slice = unsafe {
                    std::slice::from_raw_parts(ie_ptr, entry.ulIeSize as usize)
                };
                parse_channel_width(ie_slice)
            } else {
                "20 MHz".to_string()
            };

            let phy_raw = entry.dot11BssPhyType.0 as u32;
            let phy_type = phy_type_str(phy_raw);

            // BSSID ile kontrol: aynı SSID'ye sahip farklı AP'lerde sadece
            // gerçekten bağlı olduğumuz BSSID'ye "connected" tag atarız.
            // Bağlı BSSID bilinmiyorsa SSID ile fallback.
            let is_connected = if !current_bssid.is_empty() {
                bssid == current_bssid
            } else {
                !current_ssid.is_empty() && ssid == current_ssid
            };
            if is_connected {
                current_channel = channel;
                current_band = band.clone();
            }

            access_points.push(AccessPoint {
                ssid,
                bssid,
                rssi,
                signal_quality,
                channel,
                center_freq_mhz,
                band,
                channel_width,
                phy_type,
                is_connected,
                is_own: false,
            });
        }

        unsafe { WlanFreeMemory(bss_list as *mut _) };
    }

    unsafe {
        WlanFreeMemory(iface_list as *mut _);
        WlanCloseHandle(handle, None);
    }

    for ap in access_points.iter_mut() {
        ap.is_own = is_own_ap(ap, &current_ssid, &current_bssid);
    }

    let recommendations = build_recommendations(&access_points, current_channel, &current_band, &current_bssid, &current_ssid);

    let (final_channel, final_band) = if current_band.is_empty() {
        access_points
            .iter()
            .find(|ap| ap.is_connected)
            .map(|ap| (ap.channel, ap.band.clone()))
            .unwrap_or((0, String::new()))
    } else {
        (current_channel, current_band)
    };

    Ok(WifiScanResult {
        access_points,
        current_ssid,
        current_bssid,
        current_channel: final_channel,
        current_band: final_band,
        recommendations,
        interface_found: true,
    })
}

#[cfg(not(target_os = "windows"))]
fn run_wifi_scan_sync() -> Result<WifiScanResult, String> {
    Ok(WifiScanResult {
        access_points: vec![],
        current_ssid: String::new(),
        current_bssid: String::new(),
        current_channel: 0,
        current_band: String::new(),
        recommendations: vec![],
        interface_found: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(ssid: &str, bssid: &str, band: &str, channel: u32, rssi: i32) -> AccessPoint {
        AccessPoint {
            ssid: ssid.to_string(),
            bssid: bssid.to_string(),
            rssi,
            signal_quality: 0,
            channel,
            center_freq_mhz: 0,
            band: band.to_string(),
            channel_width: "20 MHz".to_string(),
            phy_type: String::new(),
            is_connected: false,
            is_own: false,
        }
    }

    #[test]
    fn ayni_marka_komsu_tikaniklikta_sayilir() {
        let own = "18:E8:29:10:20:30";
        let aps = vec![
            ap("Home", own, "2.4 GHz", 6, -50),
            ap("Neighbour", "18:E8:29:AA:BB:CC", "2.4 GHz", 6, -60),
        ];
        assert!(!is_same_device_bssid("18:E8:29:AA:BB:CC", own));
        assert_eq!(congestion_24(&aps, 6, "Home", own), 40);
    }

    #[test]
    fn kendi_sanal_bssid_ve_coklu_ssid_haric() {
        let own = "18:E8:29:10:20:30";
        // Ayni SSID (mesh/sanal BSS) haric
        assert!(is_own_ap(&ap("Home", "00:11:22:33:44:55", "2.4 GHz", 6, -50), "Home", own));
        // Farkli SSID, son bayt farkli (ayni radyo, multi-SSID) haric
        assert!(is_own_ap(&ap("Guest", "18:E8:29:10:20:31", "2.4 GHz", 6, -50), "Home", own));
        // Bagli BSSID'nin kendisi haric (SSID bos olsa da)
        assert!(is_own_ap(&ap("X", own, "2.4 GHz", 6, -50), "", own));
        // Bagli BSSID bilinmiyorsa BSSID bazli haric tutma yok
        assert!(!is_own_ap(&ap("Guest", "18:E8:29:10:20:31", "2.4 GHz", 6, -50), "Home", ""));
    }

    #[test]
    fn locally_administered_sanal_bssid_ayni_cihaz() {
        // 1E:E8:29 vs 18:E8:29 (LA biti + hostapd bit'i), orta baytlar ayni
        assert!(is_same_device_bssid("1E:E8:29:10:20:30", "18:E8:29:10:20:30"));
        assert!(is_same_device_bssid("1e:e8:29:10:20:3a", "18:E8:29:10:20:30"));
        // LA biti yok, ilk bayt farkli -> farkli cihaz
        assert!(!is_same_device_bssid("1C:E8:29:10:20:30", "18:E8:29:10:20:30"));
        // Sanal-BSS indeksi + LA biti: 18 vs 1E, son bayt farkli -> ayni cihaz
        assert!(is_same_device_bssid("18:E8:29:10:20:30", "1E:E8:29:10:20:31"));
        // LA biti set ama ilk bayt bit 1-3 disinda farkli (EA vs 18) -> farkli cihaz
        assert!(!is_same_device_bssid("18:E8:29:10:20:30", "EA:E8:29:10:20:99"));
        // Multicast biti set -> sahiplik yok
        assert!(!is_same_device_bssid("19:E8:29:10:20:30", "19:E8:29:10:20:30"));
        // Orta baytlar farkli -> farkli cihaz
        assert!(!is_same_device_bssid("1E:E8:29:11:20:30", "18:E8:29:10:20:30"));
        // Ayristirilamayan girdi
        assert!(!is_same_device_bssid("", "18:E8:29:10:20:30"));
        assert!(!is_same_device_bssid("zz", "zz"));
    }

    fn eht_ie(params: u8, ctrl: u8) -> Vec<u8> {
        // id=255, len=9: ext(106) params mcs[4] control ccfs0 ccfs1
        vec![255, 9, 106, params, 0, 0, 0, 0, ctrl, 0, 0]
    }

    #[test]
    fn eht_op_genislik() {
        assert_eq!(parse_channel_width(&eht_ie(0x01, 0)), "20 MHz");
        assert_eq!(parse_channel_width(&eht_ie(0x01, 2)), "80 MHz");
        assert_eq!(parse_channel_width(&eht_ie(0x01, 3)), "160 MHz");
        assert_eq!(parse_channel_width(&eht_ie(0x01, 4)), "320 MHz");
        // Diger Parameters bitleri (bit1..) kontrol baytini etkilemez
        assert_eq!(parse_channel_width(&eht_ie(0x0F, 4)), "320 MHz");
    }

    #[test]
    fn eht_op_bilgi_yok_veya_kesik_ise_vht_fallback() {
        let vht = [192u8, 3, 0x01, 0x00, 0x00];
        // Operation Information Present biti yok: eski kod data[1]&7=4 -> 320 derdi
        let mut ie = eht_ie(0x04, 4);
        ie.extend_from_slice(&vht);
        assert_eq!(parse_channel_width(&ie), "80 MHz");
        // Kesik (len=8, EHT Operation Information tam degil: CCFS1 yok) -> guvenilmez, fallback
        let mut cut = vec![255u8, 8, 106, 0x01, 0, 0, 0, 0, 4, 0];
        cut.extend_from_slice(&vht);
        assert_eq!(parse_channel_width(&cut), "80 MHz");
        // Reserved genislik degeri yok sayilir
        let mut rsv = eht_ie(0x01, 7);
        rsv.extend_from_slice(&vht);
        assert_eq!(parse_channel_width(&rsv), "80 MHz");
    }

    #[test]
    fn eht_op_kesik_bloblar_panik_etmez() {
        let full = eht_ie(0x01, 4);
        // Her kesme noktasi: IE basligindaki len dogru kalsa da govde kesik
        for cut in 0..=full.len() {
            let _ = parse_channel_width(&full[..cut]);
        }
        // Her len degeri icin (govde sabit): panik yok
        for len in 0u8..=12 {
            let mut ie = vec![255u8, len, 106, 0x01, 0, 0, 0, 0, 4, 0, 0];
            ie.truncate(2 + len as usize);
            let _ = parse_channel_width(&ie);
        }
        assert_eq!(parse_channel_width(&[255, 1, 106]), "20 MHz");
    }

    fn rec<'a>(recs: &'a [ChannelRecommendation], band: &str) -> &'a ChannelRecommendation {
        recs.iter().find(|r| r.band == band).unwrap()
    }

    #[test]
    fn bonding_stay_mevcut_grup_kirliyse_temiz_alternatife_gec() {
        let own = "AA:BB:CC:00:00:01";
        let aps = vec![
            ap("Home", own, "5 GHz", 36, -50),
            ap("N1", "10:00:00:00:00:01", "5 GHz", 36, -40),
            ap("N2", "20:00:00:00:00:02", "5 GHz", 40, -40),
            ap("N3", "30:00:00:00:00:03", "5 GHz", 44, -40),
            ap("N4", "40:00:00:00:00:04", "5 GHz", 48, -40),
            ap("N5", "50:00:00:00:00:05", "5 GHz", 149, -90),
        ];
        let recs = build_recommendations(&aps, 36, "5 GHz", own, "Home");
        // Mevcut 40/80/160 gruplari kirli, temiz alternatif var -> gec
        assert_eq!(rec(&recs, "5 GHz 40MHz").recommended_channel, 52);
        assert_eq!(rec(&recs, "5 GHz 80MHz").recommended_channel, 52);
        assert_eq!(rec(&recs, "5 GHz 160MHz").recommended_channel, 100);
    }

    #[test]
    fn bonding_stay_mevcut_grup_temizse_veya_histerezis_icindeyse_kal() {
        let own = "AA:BB:CC:00:00:01";
        // Mevcut grup temiz, baska yerler kirli
        let clean = vec![
            ap("Home", own, "5 GHz", 36, -50),
            ap("N1", "10:00:00:00:00:01", "5 GHz", 52, -40),
            ap("N2", "20:00:00:00:00:02", "5 GHz", 100, -40),
            ap("N3", "30:00:00:00:00:03", "5 GHz", 149, -40),
        ];
        let recs = build_recommendations(&clean, 36, "5 GHz", own, "Home");
        assert_eq!(rec(&recs, "5 GHz 40MHz").recommended_channel, 36);
        assert_eq!(rec(&recs, "5 GHz 80MHz").recommended_channel, 36);
        assert_eq!(rec(&recs, "5 GHz 160MHz").recommended_channel, 36);
        // Mevcut grupta kucuk gurultu (skor 10 < HYSTERESIS), alternatif temiz -> kal
        let mild = vec![
            ap("Home", own, "5 GHz", 44, -50),
            ap("N1", "10:00:00:00:00:01", "5 GHz", 48, -90),
        ];
        let recs = build_recommendations(&mild, 44, "5 GHz", own, "Home");
        assert_eq!(rec(&recs, "5 GHz 40MHz").recommended_channel, 44);
    }

    #[test]
    fn bos_ve_bozuk_ie_default_20mhz() {
        // Bos IE -> default 20 MHz, PANIK YOK
        assert_eq!(parse_channel_width(&[]), "20 MHz");
        // Tek byte (eksik len) -> dongu girmez
        assert_eq!(parse_channel_width(&[61]), "20 MHz");
        // len tasmasi: id=61, len=10 ama veri yok -> break, panik yok
        assert_eq!(parse_channel_width(&[61, 10, 0x00]), "20 MHz");
    }

    #[test]
    fn ht_op_40mhz() {
        // HT Operation (id=61, len=3): data[1] & 0x03 = sec_offset
        // sec_offset != 0 -> 40 MHz
        let ie = [61u8, 3, 0x00, 0x01, 0x00];
        assert_eq!(parse_channel_width(&ie), "40 MHz");
        // sec_offset = 0 -> 20 MHz
        let ie0 = [61u8, 3, 0x00, 0x00, 0x00];
        assert_eq!(parse_channel_width(&ie0), "20 MHz");
    }

    #[test]
    fn vht_op_80mhz() {
        // VHT Operation (id=192, len=3): data[0] = width; 1 -> 80 MHz
        let ie = [192u8, 3, 0x01, 0x00, 0x00];
        assert_eq!(parse_channel_width(&ie), "80 MHz");
        // width=2 -> 160 MHz
        let ie160 = [192u8, 3, 0x02, 0x00, 0x00];
        assert_eq!(parse_channel_width(&ie160), "160 MHz");
    }

    #[test]
    fn vht_oncelik_ht_uzerinde() {
        // Hem HT(40) hem VHT(80) varsa VHT kazanir (oncelik)
        let ie = [61u8, 3, 0x00, 0x01, 0x00, 192u8, 3, 0x01, 0x00, 0x00];
        assert_eq!(parse_channel_width(&ie), "80 MHz");
    }
}

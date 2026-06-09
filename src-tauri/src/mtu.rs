//! MTU / MSS Keşfi (Lite araç)
//!
//! Path MTU Discovery (RFC 1191 mantığı): DF (Don't Fragment) bitli ICMP echo'yu
//! ARTAN payload boyutlarıyla gönderir. Yol MTU'sundan büyük paket → router
//! "fragmentation needed" döndürür (Windows: IcmpSendEcho IP_REQ_MTU_EXCEEDED).
//! İkili arama (binary search) ile geçen en büyük payload bulunur → Path MTU.
//!
//! Yöntem: traceroute ile aynı `IcmpSendEcho` (admin GEREKMEZ) + IP_OPTION_INFORMATION
//! Flags = IP_FLAG_DF (0x02). Sunucu GEREKMEZ — hedefe doğrudan DF-ping.
//!
//! Toplam paket boyutu = IP header (20) + ICMP header (8) + payload.
//! Yani: Path MTU = en_buyuk_gecen_payload + 28.
//!
//! Hedefler: 1.1.1.1 (internet path MTU) + gateway (yerel link MTU). ICMP bloklu
//! yolda "belirlenemedi" döner (uydurma YOK).

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

/// IP + ICMP header toplamı (payload dışı). Path MTU = payload + bu.
const IP_ICMP_HEADER: u32 = 28;
/// İkili arama alt/üst sınır (payload byte). 1500 MTU → 1472 payload; jumbo'ya kadar dene.
const MIN_PAYLOAD: u32 = 0; // 28 byte paket (sadece header)
const MAX_PAYLOAD: u32 = 9000 - IP_ICMP_HEADER; // jumbo frame üst sınırı
/// Her boyut için ICMP timeout.
const PROBE_TIMEOUT_MS: u32 = 1500;
/// İnternet path MTU için varsayılan hedef.
const INTERNET_TARGET: Ipv4Addr = Ipv4Addr::new(1, 1, 1, 1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtuTarget {
    pub label: String,
    pub host: String,
    /// Keşfedilen Path MTU (byte). None = ICMP bloklu/ulaşılamadı.
    pub mtu: Option<u32>,
    /// MSS = MTU - 40 (TCP/IP header). None = MTU yok.
    pub mss: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtuResult {
    pub targets: Vec<MtuTarget>,
    /// Değerlendirme için kullanılan birincil (internet) MTU.
    pub primary_mtu: Option<u32>,
    /// i18n açıklama anahtarı (MTU değerine göre).
    pub detail_key: String,
}

/// DF-bitli ICMP echo gönderir; `true` = geçti (fragmentasyon gerekmedi),
/// `false` = çok büyük (MTU exceeded) VEYA ulaşılamadı. `None` = hedef hiç yanıt vermiyor
/// (ICMP tamamen bloklu — MTU keşfi yapılamaz).
///
/// Ayrım için: payload=0 ile bir kez dene; o bile yanıt vermezse hedef ICMP'yi blokluyor.
#[cfg(windows)]
fn df_ping_v4(target: Ipv4Addr, payload_len: u32) -> ProbeResult {
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
    };

    const IP_FLAG_DF: u8 = 0x02; // Don't Fragment
    const IP_SUCCESS: u32 = 0;
    // Paket çok büyük + DF set → fragmentasyon gerekti ama yapılamadı.
    const IP_REQ_MTU_EXCEEDED: u32 = 11050;
    const IP_PACKET_TOO_BIG: u32 = 11009;

    let o = target.octets();
    let dest: u32 = (o[0] as u32) | (o[1] as u32) << 8 | (o[2] as u32) << 16 | (o[3] as u32) << 24;

    // Payload tamponu (içerik önemsiz).
    let payload = vec![0x61u8; payload_len as usize];

    unsafe {
        let handle: HANDLE = match IcmpCreateFile() {
            Ok(h) if !h.is_invalid() => h,
            _ => return ProbeResult::Unreachable,
        };

        let opts = IP_OPTION_INFORMATION {
            Ttl: 64,
            Tos: 0,
            Flags: IP_FLAG_DF, // ← DF set: yol fragmentasyon isterse hata döner
            OptionsSize: 0,
            OptionsData: std::ptr::null_mut(),
        };

        let reply_size = std::mem::size_of::<ICMP_ECHO_REPLY>() + payload_len as usize + 8;
        let mut reply_buf = vec![0u8; reply_size];

        let ret = IcmpSendEcho(
            handle,
            dest,
            payload.as_ptr() as *const core::ffi::c_void,
            payload_len as u16,
            Some(&opts),
            reply_buf.as_mut_ptr() as *mut core::ffi::c_void,
            reply_size as u32,
            PROBE_TIMEOUT_MS,
        );

        let status = if ret != 0 {
            let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
            reply.Status
        } else {
            // ret==0: IcmpSendEcho başarısız. GetLastError ile MTU-exceeded ayırt edilebilir
            // ama pratikte ret==0 + büyük payload çoğunlukla "too big" demek; yine de
            // güvenli tarafta "TooBig" varsaymak yanlış olabilir → Unreachable/timeout say.
            return ProbeResult::Unreachable;
        };

        let _ = IcmpCloseHandle(handle);

        match status {
            IP_SUCCESS => ProbeResult::Ok,
            IP_REQ_MTU_EXCEEDED | IP_PACKET_TOO_BIG => ProbeResult::TooBig,
            _ => ProbeResult::Unreachable,
        }
    }
}

#[cfg(not(windows))]
fn df_ping_v4(_target: Ipv4Addr, _payload_len: u32) -> ProbeResult {
    ProbeResult::Unreachable
}

#[derive(Debug, PartialEq)]
enum ProbeResult {
    Ok,          // bu boyut geçti
    TooBig,      // DF set + MTU aşıldı → daralt
    Unreachable, // yanıt yok (ICMP bloklu olabilir)
}

/// Bir hedef için Path MTU'yu ikili aramayla keşfeder.
/// None = hedef ICMP'ye hiç yanıt vermiyor (keşif yapılamaz).
fn discover_mtu(target: Ipv4Addr) -> Option<u32> {
    // 1. Önce küçük bir DF-ping ile hedefin ICMP'ye yanıt verdiğini doğrula.
    //    payload=0 bile yanıt vermezse → ICMP bloklu, keşif imkânsız.
    match df_ping_v4(target, 0) {
        ProbeResult::Ok => {}
        // payload=0 TooBig olamaz; Unreachable → bloklu.
        _ => return None,
    }

    // 2. İkili arama: en büyük "Ok" dönen payload'u bul.
    let mut lo = MIN_PAYLOAD; // bilinen geçen (0 geçti)
    let mut hi = MAX_PAYLOAD; // bilinen üst sınır (geçmeyebilir)
    let mut best = 0u32;

    // Üst sınır gerçekten geçmiyor mu? Geçiyorsa jumbo — direkt onu kullan.
    if df_ping_v4(target, hi) == ProbeResult::Ok {
        return Some(hi + IP_ICMP_HEADER);
    }

    // lo geçer, hi geçmez → aradaki en büyük geçeni bul.
    while lo + 1 < hi {
        let mid = lo + (hi - lo) / 2;
        match df_ping_v4(target, mid) {
            ProbeResult::Ok => {
                best = mid;
                lo = mid;
            }
            ProbeResult::TooBig => {
                hi = mid;
            }
            ProbeResult::Unreachable => {
                // Geçici kayıp olabilir; bir kez daha dene, yine olmazsa daralt
                // (yanlış büyük MTU raporlamaktansa muhafazakâr ol).
                if df_ping_v4(target, mid) == ProbeResult::Ok {
                    best = mid;
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
        }
    }

    Some(best + IP_ICMP_HEADER)
}

/// MTU değerine göre i18n açıklama anahtarı (Smart ile tutarlı eşikler).
fn mtu_detail_key(mtu: Option<u32>) -> &'static str {
    match mtu {
        None => "mtu_detail_blocked",
        Some(m) if m >= 1500 => "mtu_detail_optimal",   // Ethernet standart / jumbo
        Some(m) if m >= 1492 => "mtu_detail_pppoe",     // PPPoE (çok yaygın, normal)
        Some(m) if m >= 1400 => "mtu_detail_tunnel",    // VPN/tünel overhead (kabul edilebilir)
        Some(m) if m >= 1280 => "mtu_detail_low",       // düşük — fragmentasyon riski
        Some(_) => "mtu_detail_critical",               // <1280 IPv6 min altı, ciddi
    }
}

/// Ana komut: internet (1.1.1.1) + gateway için MTU keşfi.
#[tauri::command]
pub async fn run_mtu_test() -> Result<MtuResult, String> {
    crate::dlog!("[mtu] başlıyor");

    // ICMP probe'lar bloklayıcı (IcmpSendEcho senkron) → spawn_blocking.
    let internet = tokio::task::spawn_blocking(|| discover_mtu(INTERNET_TARGET))
        .await
        .unwrap_or(None);

    // Gateway hedefi (yerel link MTU). network_topology'den çöz (yalnızca IPv4).
    let gateway_ip = match crate::network_topology::detect_default_gateway() {
        Some(std::net::IpAddr::V4(v4)) => Some(v4),
        _ => None,
    };
    let gateway_mtu = if let Some(gw) = gateway_ip {
        tokio::task::spawn_blocking(move || discover_mtu(gw)).await.unwrap_or(None)
    } else {
        None
    };

    crate::dlog!("[mtu] internet={:?} gateway={:?}", internet, gateway_mtu);

    let mut targets = vec![MtuTarget {
        label: "Internet (1.1.1.1)".to_string(),
        host: "1.1.1.1".to_string(),
        mtu: internet,
        mss: internet.map(|m| m.saturating_sub(40)),
    }];
    if let Some(gw) = gateway_ip {
        targets.push(MtuTarget {
            label: "Gateway".to_string(),
            host: gw.to_string(),
            mtu: gateway_mtu,
            mss: gateway_mtu.map(|m| m.saturating_sub(40)),
        });
    }

    let detail_key = mtu_detail_key(internet).to_string();

    Ok(MtuResult {
        targets,
        primary_mtu: internet,
        detail_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mtu_detail_esikleri() {
        assert_eq!(mtu_detail_key(None), "mtu_detail_blocked");
        assert_eq!(mtu_detail_key(Some(1500)), "mtu_detail_optimal");
        assert_eq!(mtu_detail_key(Some(1600)), "mtu_detail_optimal");
        assert_eq!(mtu_detail_key(Some(1492)), "mtu_detail_pppoe");
        assert_eq!(mtu_detail_key(Some(1450)), "mtu_detail_tunnel");
        assert_eq!(mtu_detail_key(Some(1300)), "mtu_detail_low");
        assert_eq!(mtu_detail_key(Some(1200)), "mtu_detail_critical");
    }

    // Canli ag testi: cargo test --lib mtu_canli -- --ignored --nocapture
    #[test]
    #[ignore]
    fn mtu_canli() {
        let internet = discover_mtu(INTERNET_TARGET);
        println!("\n=== CANLI MTU ===");
        println!("1.1.1.1 Path MTU: {:?}  (MSS: {:?})", internet, internet.map(|m| m - 40));
        println!("detail_key: {}\n", mtu_detail_key(internet));
    }

    #[test]
    fn mss_hesabi() {
        // MSS = MTU - 40 (20 IP + 20 TCP header)
        let mtu = 1500u32;
        assert_eq!(mtu.saturating_sub(40), 1460); // klasik Ethernet MSS
    }
}

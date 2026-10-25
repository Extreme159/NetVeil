// Sunucudan (api.netveil.net) çekilen hedef listelerinin (ping-targets, dns-providers, ...)
// istemci tarafı doğrulaması. Sunucu ele geçirilse / MITM olsa bile istemci; yerel ağ,
// loopback veya link-local adreslere bağlanmaya yönlendirilemesin diye girdiler kullanılmadan
// önce sözdizimi + adres aralığı açısından süzülür. Geçersiz girdi DÜŞER (liste yine çalışır;
// hiç geçerli girdi kalmazsa çağıran gömülü fallback listeye döner).
//
// Sınır: yalnızca sözdizimi/literal IP kontrol edilir; bir DNS adının sonradan özel bir IP'ye
// çözülmesi (DNS rebinding) burada engellenmez.

use serde::de::DeserializeOwned;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Ham JSON girdilerini tek tek `T`'ye çevirir; çözülemeyen veya `valid` reddeden girdileri atar.
/// (Tek bozuk girdi tüm listeyi çöpe atmasın diye `Vec<T>` yerine girdi bazlı ayrıştırma.)
pub fn filter_valid<T: DeserializeOwned>(raw: Vec<serde_json::Value>, valid: impl Fn(&T) -> bool) -> Vec<T> {
    raw.into_iter()
        .filter_map(|v| serde_json::from_value::<T>(v).ok())
        .filter(|t| valid(t))
        .collect()
}

// Tek politika: IANA IPv4/IPv6 Special-Purpose Address Registry. Aynı tablo sunucuda
// (server/src/main.rs) ve UI'da (ui/src/lib/remotePresets.ts) birebir tekrarlanır; değiştirirken
// üçünü birlikte güncelle.
fn is_public_ipv4(v4: Ipv4Addr) -> bool {
    let o = v4.octets();
    !(o[0] == 0                                  // 0.0.0.0/8 (unspecified dahil)
        || o[0] == 10                            // 10/8
        || (o[0] == 100 && (o[1] & 0xC0) == 64)  // CGNAT 100.64/10
        || o[0] == 127                           // loopback 127/8
        || (o[0] == 169 && o[1] == 254)          // link-local 169.254/16
        || (o[0] == 172 && (o[1] & 0xF0) == 16)  // 172.16/12
        // 192.0.0.0/24 IETF protokol atamaları; küresel erişilebilir 192.0.0.9 ve 192.0.0.10 hariç
        || (o[0] == 192 && o[1] == 0 && o[2] == 0 && o[3] != 9 && o[3] != 10)
        || (o[0] == 192 && o[1] == 0 && o[2] == 2)   // TEST-NET-1 192.0.2/24
        || (o[0] == 192 && o[1] == 88 && o[2] == 99) // 6to4 relay (deprecated) 192.88.99/24
        || (o[0] == 192 && o[1] == 168)          // 192.168/16
        || (o[0] == 198 && (o[1] & 0xFE) == 18)  // benchmarking 198.18/15
        || (o[0] == 198 && o[1] == 51 && o[2] == 100) // TEST-NET-2 198.51.100/24
        || (o[0] == 203 && o[1] == 0 && o[2] == 113)  // TEST-NET-3 203.0.113/24
        || o[0] >= 224)                          // multicast 224/4 + reserved 240/4 + broadcast
}

fn is_public_ipv6(v6: Ipv6Addr) -> bool {
    let s = v6.segments();
    // IPv4-mapped ::ffff:0:0/96 → gömülü IPv4 kurallarıyla.
    if let Some(v4) = v6.to_ipv4_mapped() {
        return is_public_ipv4(v4);
    }
    let embedded = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    // NAT64 64:ff9b::/96 → gömülü IPv4 (::/8 kuralından önce: 0064:: da ::/8 içindedir).
    if s[0] == 0x0064 && s[1] == 0xff9b && s[2..6].iter().all(|&x| x == 0) {
        return is_public_ipv4(embedded(s[6], s[7]));
    }
    // ::/8 IETF rezerve: ::/128 unspecified, ::1 loopback, IPv4-compatible ::a.b.c.d,
    // 64:ff9b:1::/48 yerel NAT64 dahil.
    if (s[0] & 0xFF00) == 0 {
        return false;
    }
    // 6to4 2002::/16 → gömülü IPv4.
    if s[0] == 0x2002 {
        return is_public_ipv4(embedded(s[1], s[2]));
    }
    // 2001::/23 IETF protokol atamaları; küresel erişilebilir alt aralıklar hariç:
    // 2001:1::1/128, 2001:1::2/128, 2001:3::/32, 2001:4:112::/48, 2001:20::/28, 2001:30::/28.
    let in_2001_23 = s[0] == 0x2001 && (s[1] & 0xFE00) == 0;
    let in_2001_global = (s[1] == 0x0001 && s[2..7].iter().all(|&x| x == 0) && (s[7] == 1 || s[7] == 2))
        || s[1] == 0x0003
        || (s[1] == 0x0004 && s[2] == 0x0112)
        || (s[1] & 0xFFF0) == 0x0020
        || (s[1] & 0xFFF0) == 0x0030;
    !((in_2001_23 && !in_2001_global)
        || (s[0] == 0x0100 && s[1] == 0 && s[2] == 0 && s[3] <= 1)          // discard-only 100::/64 + dummy 100:0:0:1::/64 (RFC 9780)
        || (s[0] == 0x2001 && s[1] == 0x0db8)                              // documentation 2001:db8::/32
        || (s[0] == 0x3fff && (s[1] & 0xF000) == 0)                        // documentation 3fff::/20
        || s[0] == 0x5f00                                                  // SRv6 SIDs 5f00::/16
        || (s[0] & 0xFE00) == 0xFC00                                       // ULA fc00::/7
        || (s[0] & 0xFFC0) == 0xFE80                                       // link-local fe80::/10
        || (s[0] & 0xFFC0) == 0xFEC0                                       // site-local fec0::/10 (deprecated)
        || (s[0] & 0xFF00) == 0xFF00)                                      // multicast ff00::/8
}

/// Halka açık yönlendirilebilir adres mi? (özel/loopback/link-local/multicast/unspecified/... → false)
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_ipv4(v4),
        IpAddr::V6(v6) => is_public_ipv6(v6),
    }
}

/// RFC 1123 sözdizimli, en az iki etiketli DNS adı. Yerel/özel kullanım son ekleri ve
/// son etiketi tamamen rakam olan biçimler (`127.1`, `0x7f.1` gibi kısaltmalar) reddedilir.
pub fn is_valid_hostname(h: &str) -> bool {
    if h.is_empty() || h.len() > 253 {
        return false;
    }
    let lower = h.to_ascii_lowercase();
    const LOCAL_SUFFIXES: [&str; 7] = [".localhost", ".local", ".localdomain", ".internal", ".lan", ".intranet", ".home.arpa"];
    if LOCAL_SUFFIXES.iter().any(|s| lower.ends_with(s)) {
        return false;
    }
    let labels: Vec<&str> = lower.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    };
    if !labels.iter().all(label_ok) {
        return false;
    }
    // WHATWG URL "ends in a number": son etiket tamamen rakam veya 0x-onekli hex ise URL ayrıştırıcıları
    // (ve inet_aton/getaddrinfo) host'u IPv4 olarak yorumlar (`0x7f.0x0.0x0.0x1`, `127.1`,
    // `2130706433`, `0177.0.0.1`). Bunlar DNS adı değildir → reddedilir.
    let last = labels[labels.len() - 1];
    let hex_tail = last.strip_prefix("0x").is_some_and(|r| r.bytes().all(|b| b.is_ascii_hexdigit()));
    if hex_tail || last.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    // Savunma derinliği: gerçek URL ayrıştırıcısının normalize ettiği host hâlâ aynı alan adı olmalı
    // (IP'ye dönüşen veya yeniden yazılan her biçim reddedilir).
    reqwest::Url::parse(&format!("https://{lower}/")).is_ok_and(|u| u.host_str() == Some(lower.as_str()))
}

/// Ağ hedefi olarak kullanılabilir host: halka açık literal IP veya geçerli DNS adı.
/// Port/şema/yol/köşeli parantez içeren değerler host sayılmaz.
pub fn is_valid_target_host(h: &str) -> bool {
    if let Ok(ip) = h.parse::<IpAddr>() {
        return is_public_ip(ip);
    }
    is_valid_hostname(h)
}

/// DoH istek yolu: `/` ile başlar, makul uzunlukta, yalnızca URL-yol güvenli karakterler;
/// sorgu (`?`), parça (`#`), ters eğik çizgi, `..` ve boşluk/kontrol karakteri yok.
pub fn is_valid_url_path(p: &str) -> bool {
    p.starts_with('/')
        && p.len() <= 128
        && !p.contains("..")
        && !p.contains("//")
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'_' | b'.' | b'~' | b'%'))
}

/// TCP/UDP hedef portu: 1..=65535 (u16 olduğundan yalnızca 0 geçersiz).
pub fn is_valid_port(p: u16) -> bool {
    p != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn halka_acik_ip_kabul() {
        for s in ["1.1.1.1", "8.8.8.8", "151.101.1.1", "185.228.168.168", "2606:4700:4700::1111", "2001:4860:4860::8888"] {
            assert!(is_public_ip(ip(s)), "{s}");
        }
    }

    #[test]
    fn ozel_loopback_linklocal_multicast_unspecified_ipv4_red() {
        for s in [
            "0.0.0.0", "0.1.2.3", "127.0.0.1", "127.255.255.254", "10.0.0.1", "172.16.0.1", "172.31.255.255",
            "192.168.1.1", "169.254.169.254", "100.64.0.1", "100.127.255.255", "224.0.0.1", "239.255.255.250",
            "240.0.0.1", "255.255.255.255", "192.0.0.1", "198.18.0.1", "192.0.2.1", "198.51.100.1", "203.0.113.9",
        ] {
            assert!(!is_public_ip(ip(s)), "{s}");
        }
    }

    #[test]
    fn ozel_loopback_linklocal_multicast_unspecified_ipv6_red() {
        for s in [
            "::", "::1", "fe80::1", "fc00::1", "fd12:3456::1", "ff02::1", "fec0::1", "2001:db8::1",
            "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:192.168.1.1", "::ffff:169.254.1.1", "::ffff:0.0.0.0",
            "::127.0.0.1", "64:ff9b::7f00:1", "64:ff9b::a00:1", "2002:7f00:1::1", "2002:c0a8:101::1",
        ] {
            assert!(!is_public_ip(ip(s)), "{s}");
        }
    }

    #[test]
    fn ipv4_mapped_halka_acik_kabul() {
        assert!(is_public_ip(ip("::ffff:8.8.8.8")));
    }

    #[test]
    fn hostname_gecerli_kabul() {
        for h in ["cloudflare.com", "dns.google", "cache2-vie1.steamcontent.com", "xn--bcher-kva.example", "a.b", "UPPER.Example.COM"] {
            assert!(is_valid_target_host(h), "{h}");
        }
    }

    #[test]
    fn hostname_gecersiz_red() {
        for h in [
            "", "localhost", "foo.localhost", "printer.local", "nas.lan", "svc.internal", "router.home.arpa", "intranet",
            "127.1", "0x7f.0.0.1", "1.2.3", "-a.com", "a-.com", "a..com", ".com", "a.com.", "exa mple.com",
            "http://evil.com", "evil.com/path", "evil.com:8080", "user@evil.com", "[::1]", "a_b.com", "ex\u{e4}mple.com",
        ] {
            assert!(!is_valid_target_host(h), "{h}");
        }
        assert!(!is_valid_target_host(&format!("{}.com", "a".repeat(64))));
        assert!(!is_valid_target_host(&format!("{}.com", vec!["a".repeat(60); 5].join("."))));
    }

    #[test]
    fn literal_ip_host_aralik_kontrolu() {
        assert!(is_valid_target_host("1.1.1.1"));
        assert!(is_valid_target_host("2606:4700:4700::1111"));
        for h in ["127.0.0.1", "10.1.1.1", "169.254.169.254", "::1", "fe80::1", "::ffff:10.0.0.1", "0.0.0.0"] {
            assert!(!is_valid_target_host(h), "{h}");
        }
    }

    #[test]
    fn sayisal_alternatif_ipv4_bicimleri_hostname_degil() {
        for h in [
            "0x7f.0x0.0x0.0x1", "0x7f.1", "0x7f.0.0.0x1", "127.1", "2130706433", "0177.0.0.1", "0177.1",
            "0x7f000001", "0X7F.0X1", "1.2.0x", "0x8.0x8.0x8.0x8", "8.8.8.08", "a.0x1f", "example.0x7f",
            "4294967295", "1.1.1", "10.1",
        ] {
            assert!(!is_valid_hostname(h), "{h}");
            assert!(!is_valid_target_host(h), "{h}");
        }
    }

    #[test]
    fn normal_dns_adlari_kabul() {
        for h in [
            "dns.google", "cloudflare-dns.com", "1dot1dot1dot1.cloudflare-dns.com", "dns.quad9.net",
            "0x7f.example.com", "127.0.0.1.nip.io", "x0x1.com", "a.0xg", "1e100.net", "a.b2",
        ] {
            assert!(is_valid_hostname(h), "{h}");
            assert!(is_valid_target_host(h), "{h}");
        }
    }

    #[test]
    fn iana_ozel_amacli_ipv4_tablosu() {
        let reject = [
            "0.0.0.0", "0.255.255.255", "10.0.0.0", "10.255.255.255", "100.64.0.0", "100.127.255.255",
            "127.0.0.0", "127.255.255.255", "169.254.0.0", "169.254.255.255", "172.16.0.0", "172.31.255.255",
            "192.0.0.0", "192.0.0.8", "192.0.0.11", "192.0.0.255", "192.0.2.0", "192.0.2.255", "192.88.99.0",
            "192.88.99.255", "192.168.0.0", "192.168.255.255", "198.18.0.0", "198.19.255.255", "198.51.100.0",
            "198.51.100.255", "203.0.113.0", "203.0.113.255", "224.0.0.0", "239.255.255.255", "240.0.0.0",
            "255.255.255.254", "255.255.255.255",
        ];
        let accept = [
            "1.0.0.0", "9.255.255.255", "11.0.0.0", "100.63.255.255", "100.128.0.0", "126.255.255.255",
            "128.0.0.0", "169.253.255.255", "169.255.0.0", "172.15.255.255", "172.32.0.0", "192.0.0.9",
            "192.0.0.10", "192.0.1.0", "192.0.3.0", "192.88.98.255", "192.88.100.0", "192.167.255.255",
            "192.169.0.0", "198.17.255.255", "198.20.0.0", "198.51.99.255", "198.51.101.0", "203.0.112.255",
            "203.0.114.0", "223.255.255.255", "8.8.8.8", "1.1.1.1",
        ];
        for s in reject {
            assert!(!is_public_ip(ip(s)), "{s} reddedilmeli");
        }
        for s in accept {
            assert!(is_public_ip(ip(s)), "{s} kabul edilmeli");
        }
    }

    #[test]
    fn iana_ozel_amacli_ipv6_tablosu() {
        let reject = [
            "::", "::1", "::2", "::127.0.0.1", "::ffff:0.0.0.0", "::ffff:127.0.0.1", "::ffff:10.1.1.1",
            "::ffff:192.0.0.8", "64:ff9b::7f00:1", "64:ff9b::c0a8:1", "64:ff9b:1::1", "64:ff9b:1:ffff::1",
            "100::", "100::1", "100:0:0:0:ffff::1", "100:0:0:1::", "100:0:0:1:ffff:ffff:ffff:ffff", "2001::", "2001:0:4136:e378::1", "2001:1::", "2001:1::3",
            "2001:2::1", "2001:10::1", "2001:1ff:ffff::1", "2001:db8::1", "2001:db8:ffff::1", "2002:7f00:1::1",
            "2002:c0a8:101::1", "2002:0a00:1::1", "3fff::", "3fff:fff:ffff::1", "5f00::", "5f00:ffff::1", "fc00::",
            "fd12:3456::1", "fdff:ffff::1", "fe80::1", "febf::1", "fec0::1", "feff::1", "ff00::", "ff02::1",
            "ffff::1",
        ];
        let accept = [
            "2001:1::1", "2001:1::2", "2001:3::", "2001:3:ffff::1", "2001:4:112::", "2001:4:112:ffff::1",
            "2001:20::1", "2001:2f:ffff::1", "2001:30::1", "2001:3f:ffff::1", "2001:200::1", "2001:4860:4860::8888",
            "2606:4700:4700::1111", "2a00:1450::1", "2002:808:808::1", "64:ff9b::808:808", "::ffff:8.8.8.8",
            "3ffe::1", "4000::1", "5e00::1", "5f01::1", "2fff::1", "3fff:1000::1", "fbff::1", "fe7f::1",
        ];
        for s in reject {
            assert!(!is_public_ip(ip(s)), "{s} reddedilmeli");
        }
        for s in accept {
            assert!(is_public_ip(ip(s)), "{s} kabul edilmeli");
        }
    }

    #[test]
    fn port_araligi() {
        assert!(!is_valid_port(0));
        assert!(is_valid_port(1));
        assert!(is_valid_port(443));
        assert!(is_valid_port(65535));
    }

    #[test]
    fn url_yolu() {
        for p in ["/dns-query", "/p0", "/doh/security-filter/", "/a/b_c-d.e~f"] {
            assert!(is_valid_url_path(p), "{p}");
        }
        for p in ["", "dns-query", "/a/../b", "//evil", "/a?x=1", "/a#f", "/a b", "/a\\b", "/a\n", "/\u{e4}"] {
            assert!(!is_valid_url_path(p), "{p:?}");
        }
        assert!(!is_valid_url_path(&format!("/{}", "a".repeat(200))));
    }

    #[test]
    fn filter_valid_gecersizleri_dusurur() {
        #[derive(serde::Deserialize)]
        struct T {
            host: String,
            port: Option<u16>,
        }
        let raw: Vec<serde_json::Value> = serde_json::from_str(
            r#"[
                {"host":"example.com","port":443},
                {"host":"127.0.0.1","port":80},
                {"host":"ok.example.org","port":70000},
                {"host":"zero.example.org","port":0},
                {"port":1},
                "bozuk",
                {"host":"fine.example.net"}
            ]"#,
        )
        .unwrap();
        let out = filter_valid::<T>(raw, |t| is_valid_target_host(&t.host) && t.port.is_none_or(is_valid_port));
        let hosts: Vec<&str> = out.iter().map(|t| t.host.as_str()).collect();
        assert_eq!(hosts, ["example.com", "fine.example.net"]);
    }
}

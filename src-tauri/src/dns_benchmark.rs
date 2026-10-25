// DNS Benchmark — UDP/53 · TCP-TLS/853 (DoT) · HTTPS/443 (DoH)
//
// Her provider için:
//   - 5 adet "A google.com" sorgusu → median RTT, packet loss
//   - UDP port 53  (klasik DNS, RFC 1035)
//   - DoT  port 853 (DNS over TLS, RFC 7858)
//   - DoH  HTTPS   (DNS over HTTPS, RFC 8484) — POST /dns-query
//
// Grading: <20ms=A+, <50ms=A, <100ms=B, <150ms=C, <200ms=D, >=200/timeout=F

use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::time::timeout;

const DNS_PROVIDERS_URL: &str = "https://api.netveil.net/dns-providers";
static DNS_PROVIDERS: OnceLock<Vec<DnsProviderDef>> = OnceLock::new();

/// Uygulama açılışında çağrılır — sunucudan provider listesini çek.
pub async fn init_dns_providers() {
    if DNS_PROVIDERS.get().is_some() { return; }
    let list = fetch_dns_providers_remote().await;
    let _ = DNS_PROVIDERS.set(list);
}

fn get_dns_providers() -> Vec<DnsProviderDef> {
    DNS_PROVIDERS.get().cloned().unwrap_or_else(dns_providers_fallback)
}

async fn fetch_dns_providers_remote() -> Vec<DnsProviderDef> {
    let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build()
        else { return dns_providers_fallback(); };
    let Ok(resp) = client.get(DNS_PROVIDERS_URL).send().await
        else { return dns_providers_fallback(); };
    let Ok(raw) = resp.json::<Vec<serde_json::Value>>().await
        else { return dns_providers_fallback(); };
    let list = crate::remote_list::filter_valid(raw, valid_dns_provider);
    if list.is_empty() { dns_providers_fallback() } else { list }
}

/// Sunucu girişi: ip halka açık literal IP olmalı; DoH host'u boş (DoH yok) ya da geçerli host,
/// doh_path geçerli URL yolu olmalı.
fn valid_dns_provider(p: &DnsProviderDef) -> bool {
    use crate::remote_list::{is_public_ip, is_valid_target_host, is_valid_url_path};
    p.ip.parse::<IpAddr>().is_ok_and(is_public_ip)
        && (p.doh_host.is_empty() || (is_valid_target_host(&p.doh_host) && is_valid_url_path(&p.doh_path)))
}

fn dns_providers_fallback() -> Vec<DnsProviderDef> {
    vec![
        DnsProviderDef { name: "Cloudflare".into(),    ip: "1.1.1.1".into(),         doh_host: "cloudflare-dns.com".into(),    doh_path: "/dns-query".into() },
        DnsProviderDef { name: "Google".into(),        ip: "8.8.8.8".into(),         doh_host: "dns.google".into(),            doh_path: "/dns-query".into() },
        DnsProviderDef { name: "Quad9".into(),         ip: "9.9.9.9".into(),         doh_host: "dns.quad9.net".into(),         doh_path: "/dns-query".into() },
        DnsProviderDef { name: "NextDNS".into(),       ip: "45.90.28.0".into(),      doh_host: "dns.nextdns.io".into(),        doh_path: "/dns-query".into() },
        DnsProviderDef { name: "AdGuard".into(),       ip: "94.140.14.14".into(),    doh_host: "dns.adguard-dns.com".into(),   doh_path: "/dns-query".into() },
        DnsProviderDef { name: "Control D".into(),     ip: "76.76.2.0".into(),       doh_host: "freedns.controld.com".into(),  doh_path: "/p0".into() },
        DnsProviderDef { name: "CleanBrowsing".into(), ip: "185.228.168.168".into(), doh_host: "doh.cleanbrowsing.org".into(), doh_path: "/doh/security-filter/".into() },
    ]
}

// Her sorgu için rastgele unique QNAME üret — cache bypass için kritik
// Router ve DNS sunucu cache'i, daha önce görmediği domain'i forward etmek zorunda
fn random_qname() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    // örn: "x3f7a2c.netveil-bench.invalid" — .invalid TLD asla cache'de olmaz
    format!("x{:08x}", ns)
}

// DNS wire format sorgusu oluştur: <label>.netveil-bench.invalid A IN
// Her çağrıda farklı subdomain → cache hit imkânsız
fn build_query(tx_id: u16) -> Vec<u8> {
    let label = random_qname();
    let label_bytes = label.as_bytes();
    let mut q = Vec::with_capacity(64);
    // Transaction ID
    q.push((tx_id >> 8) as u8);
    q.push((tx_id & 0xFF) as u8);
    // Flags: standard query, RD=1
    q.extend_from_slice(&[0x01, 0x00]);
    // QDCOUNT=1, ANCOUNT=0, NSCOUNT=0, ARCOUNT=0
    q.extend_from_slice(&[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    // QNAME: <label>.netveil-bench.invalid
    q.push(label_bytes.len() as u8);
    q.extend_from_slice(label_bytes);
    q.extend_from_slice(&[13, b'n',b'e',b't',b'v',b'e',b'i',b'l',b'-',b'b',b'e',b'n',b'c',b'h']); // "netveil-bench"
    q.extend_from_slice(&[7, b'i',b'n',b'v',b'a',b'l',b'i',b'd']); // "invalid"
    q.push(0x00); // root
    // QTYPE=A, QCLASS=IN
    q.extend_from_slice(&[0x00, 0x01, 0x00, 0x01]);
    q
}

// Rastgele tx_id üret — crate::rand_id (PID^nanos^atomik sayaç, proje standardı).
// Eski nanosecond-tabanlı yöntem zayıf rastgelelikti (off-path tahmin riski).
fn next_tx_id() -> u16 {
    crate::rand_id()
}

/// DNS yanit header'ini + question echo'sunu dogrular. Saf fonksiyon - test edilebilir.
/// Gecerli = QR biti set, id eslesiyor, soru bolumu bizim sorgunun aynisi (buyuk/kucuk harf
/// duyarsiz) ve RCODE NOERROR(0) veya NXDOMAIN(3). Rastgele *.netveil-bench.invalid adlari icin
/// BEKLENEN cevap NXDOMAIN'dir; SERVFAIL/REFUSED/bozuk/kisa yanit basarisizdir.
fn is_valid_dns_response(query: &[u8], resp: &[u8], expected_id: u16) -> bool {
    if resp.len() < 12 || query.len() < 12 { return false; }
    if u16::from_be_bytes([resp[0], resp[1]]) != expected_id { return false; }
    if resp[2] & 0x80 == 0 { return false; }                 // QR=1 (yanit)
    let rcode = resp[3] & 0x0F;
    if rcode != 0 && rcode != 3 { return false; }            // NOERROR | NXDOMAIN
    if u16::from_be_bytes([resp[4], resp[5]]) < 1 { return false; } // QDCOUNT >= 1
    let question = &query[12..];
    match resp.get(12..12 + question.len()) {
        Some(echo) => echo.eq_ignore_ascii_case(question),
        None => false,
    }
}

// ─── Sabitler ────────────────────────────────────────────────────────────────

// 20: p90/medyan için istatistiksel olarak anlamlı örnek (5 az güvenilirdi).
// DoT zaten tek TLS session üzerinden multiplexed (aşağı); UDP/DoH paralel provider.
const QUERY_COUNT: usize = 20;
const UDP_TIMEOUT:  Duration = Duration::from_millis(2000);
const DOT_TIMEOUT:  Duration = Duration::from_millis(3000);
const DOH_TIMEOUT:  Duration = Duration::from_millis(4000);
// DoT için tek TLS bağlantısı üzerinden tüm query'ler gönderilir
const DOT_CONNECT_TIMEOUT: Duration = Duration::from_millis(3000);

// DNS_QUERY sabiti artık kullanılmıyor — her sorgu build_query() ile üretilir
// cache bypass için rastgele subdomain kullanılır

// ─── Tipler ──────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DnsProbeResult {
    pub min_ms:      f64,
    pub avg_ms:      f64,
    pub max_ms:      f64,
    pub median_ms:   f64,
    pub jitter_ms:   f64,
    pub packet_loss: f64,  // 0.0–100.0
    pub grade:       String,
    pub reachable:   bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DnsProviderResult {
    pub name:        String,
    pub ip:          String,        // primary IPv4
    pub udp:         DnsProbeResult,
    pub dot:         DnsProbeResult,
    pub doh:         DnsProbeResult,
    /// En iyi protokol (UDP/DoT/DoH) için median
    pub best_ms:     f64,
    pub best_proto:  String,
    pub grade:       String,
    pub rank:        u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DnsBenchmarkResult {
    pub providers:   Vec<DnsProviderResult>,
    pub recommended: String,   // en iyi provider adı
    pub summary:     String,
}

#[derive(Serialize, Clone, Debug)]
pub struct DnsBenchmarkProgress {
    pub provider:  String,
    pub protocol:  String,   // "udp" | "dot" | "doh"
    pub status:    String,   // "running" | "done"
    pub median_ms: f64,
}

// ─── Provider tanımı (sunucudan çekilir) ──────────────────────────────────────

#[derive(Deserialize, Clone)]
pub struct DnsProviderDef {
    pub name:     String,
    pub ip:       String,
    pub doh_host: String,
    pub doh_path: String,
}

// ─── UDP/53 probe ─────────────────────────────────────────────────────────────

async fn probe_udp(ip: &str) -> DnsProbeResult {
    let addr: SocketAddr = format!("{}:53", ip).parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 53)));
    let mut samples: Vec<f64> = Vec::with_capacity(QUERY_COUNT);
    let mut lost = 0usize;

    for _ in 0..QUERY_COUNT {
        let tx_id = next_tx_id();
        let query = build_query(tx_id); // her sorgu farklı random subdomain → cache bypass

        let Ok(sock) = UdpSocket::bind("0.0.0.0:0").await else { lost += 1; continue; };

        // connect() — sadece bu IP:port'tan gelen paketleri al, diğerlerini kernel düşür
        // Bu hem src IP filtrelemeyi hem de port filtrelemeyi kernel seviyesinde yapar
        if sock.connect(addr).await.is_err() { lost += 1; continue; }

        let t = Instant::now();
        if timeout(UDP_TIMEOUT, sock.send(&query)).await.is_err() { lost += 1; continue; }

        let mut buf = [0u8; 512];
        match timeout(UDP_TIMEOUT, sock.recv(&mut buf)).await {
            // Header (QR/id/RCODE) + question echo doğrulaması; NXDOMAIN beklenen/başarılı cevap.
            Ok(Ok(n)) if is_valid_dns_response(&query, &buf[..n], tx_id) => {
                samples.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            _ => lost += 1,
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    build_result(samples, lost)
}

// ─── DoT/853 probe (TCP + TLS) ───────────────────────────────────────────────

async fn probe_dot(ip: &str) -> DnsProbeResult {
    let addr: SocketAddr = format!("{}:853", ip).parse()
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 853)));
    let ip_parsed: IpAddr = ip.parse().unwrap_or(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED));
    let mut samples: Vec<f64> = Vec::with_capacity(QUERY_COUNT);
    let mut lost = 0usize;

    let root_store = rustls_platform_verifier();
    let tls_config = rustls_client_config(root_store, ip);

    let domain = rustls::pki_types::ServerName::IpAddress(ip_parsed.into());

    let tcp = match timeout(DOT_CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
        Ok(Ok(s)) => s,
        _ => return build_result(vec![], QUERY_COUNT),
    };
    let connector = tokio_rustls::TlsConnector::from(tls_config);
    let mut tls   = match timeout(DOT_CONNECT_TIMEOUT, connector.connect(domain, tcp)).await {
        Ok(Ok(s)) => s,
        _ => return build_result(vec![], QUERY_COUNT),
    };

    // RFC 7858: 2-byte uzunluk prefix + DNS mesajı — her query rastgele subdomain
    for _ in 0..QUERY_COUNT {
        let tx_id = next_tx_id();
        let query = build_query(tx_id); // cache bypass
        let msg_len = query.len() as u16;
        let mut wire = Vec::with_capacity(2 + query.len());
        wire.push((msg_len >> 8) as u8);
        wire.push((msg_len & 0xFF) as u8);
        wire.extend_from_slice(&query);

        let t = Instant::now();
        if timeout(DOT_TIMEOUT, tls.write_all(&wire)).await.is_err() { lost += 1; continue; }

        let mut hdr = [0u8; 2];
        if timeout(DOT_TIMEOUT, tls.read_exact(&mut hdr)).await.is_err() { lost += 1; continue; }
        let resp_len = u16::from_be_bytes(hdr) as usize;
        if !(2..=4096).contains(&resp_len) { lost += 1; continue; }
        let mut resp = vec![0u8; resp_len];
        match timeout(DOT_TIMEOUT, tls.read_exact(&mut resp)).await {
            Ok(Ok(_)) => {
                // Header (QR/id/RCODE) + question echo doğrulaması; NXDOMAIN beklenen/başarılı cevap.
                if is_valid_dns_response(&query, &resp, tx_id) {
                    samples.push(t.elapsed().as_secs_f64() * 1000.0);
                } else {
                    lost += 1;
                }
            }
            _ => lost += 1,
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    build_result(samples, lost)
}

// ─── DoH/443 probe (HTTPS POST, RFC 8484) ────────────────────────────────────

async fn probe_doh(doh_host: &str, doh_path: &str) -> DnsProbeResult {
    if doh_host.is_empty() {
        // Provider DoH desteklemiyor
        return DnsProbeResult {
            min_ms: 0.0, avg_ms: 0.0, max_ms: 0.0,
            median_ms: 0.0, jitter_ms: 0.0,
            packet_loss: 100.0, grade: "N/A".into(), reachable: false,
        };
    }

    // Tek client — HTTP/2 connection reuse, ALPN h2 negotiate
    // Cache-Control headerı gönderilmiyor — bazı providerlar (Mullvad) reddediyor
    let client = match reqwest::Client::builder()
        .timeout(DOH_TIMEOUT)
        .use_rustls_tls()          // Windows schannel yerine rustls — ALPN h2 negotiate eder
        .http2_adaptive_window(true)
        .build()
    {
        Ok(c) => c,
        Err(_) => return build_result(vec![], QUERY_COUNT),
    };

    let url = format!("https://{}{}", doh_host, doh_path);
    let mut samples: Vec<f64> = Vec::with_capacity(QUERY_COUNT);
    let mut lost = 0usize;

    for _ in 0..QUERY_COUNT {
        // RFC 8484 §4.1: DoH istemcisi DNS ID=0 kullanmalı (cache dostu); cevap da aynı ID'yi taşır.
        let tx_id: u16 = 0;
        let query = build_query(tx_id); // her sorgu farklı random subdomain → cache bypass
        let t = Instant::now();
        let res = client.post(&url)
            .header("Content-Type", "application/dns-message")
            .header("Accept", "application/dns-message")
            .body(query.clone())
            .send()
            .await;

        match res {
            Ok(r) if r.status().is_success() && is_dns_message_content_type(&r) => {
                // Gövde gerçek bir DNS cevabı olmalı (HTTP 200 + rastgele gövde başarı sayılmaz).
                match r.bytes().await {
                    Ok(body) if is_valid_dns_response(&query, &body, tx_id) => {
                        samples.push(t.elapsed().as_secs_f64() * 1000.0);
                    }
                    _ => lost += 1,
                }
            }
            _ => lost += 1,
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    build_result(samples, lost)
}

/// DoH cevabının Content-Type'ı application/dns-message mı (parametreler/büyük-küçük harf yok sayılır).
fn is_dns_message_content_type(r: &reqwest::Response) -> bool {
    r.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/dns-message"))
}

// ─── İstatistik yardımcıları ──────────────────────────────────────────────────

fn build_result(mut samples: Vec<f64>, lost: usize) -> DnsProbeResult {
    let total = samples.len() + lost;
    let loss  = if total == 0 { 100.0 } else { (lost as f64 / total as f64) * 100.0 };

    if samples.is_empty() {
        return DnsProbeResult {
            min_ms: 0.0, avg_ms: 0.0, max_ms: 0.0,
            median_ms: 0.0, jitter_ms: 0.0,
            packet_loss: loss, grade: "F".into(), reachable: false,
        };
    }

    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min    = samples[0];
    let max    = *samples.last().unwrap_or(&0.0);
    let avg    = samples.iter().sum::<f64>() / samples.len() as f64;
    let median = if samples.len().is_multiple_of(2) {
        (samples[samples.len() / 2 - 1] + samples[samples.len() / 2]) / 2.0
    } else {
        samples[samples.len() / 2]
    };
    // Jitter = mean absolute deviation from median
    let jitter = samples.iter().map(|s| (s - median).abs()).sum::<f64>() / samples.len() as f64;

    let grade = latency_grade(median, loss);
    DnsProbeResult { min_ms: min, avg_ms: avg, max_ms: max, median_ms: median, jitter_ms: jitter, packet_loss: loss, grade, reachable: true }
}

fn latency_grade(median_ms: f64, loss: f64) -> String {
    if loss >= 60.0             { return "F".into(); }
    if median_ms <  20.0        { "A+".into() }
    else if median_ms <  50.0   { "A".into()  }
    else if median_ms < 100.0   { "B".into()  }
    else if median_ms < 150.0   { "C".into()  }
    else if median_ms < 200.0   { "D".into()  }
    else                        { "F".into()  }
}

fn overall_grade(best_ms: f64, loss: f64) -> String {
    latency_grade(best_ms, loss)
}

// ─── TLS yardımcıları ─────────────────────────────────────────────────────────

fn rustls_platform_verifier() -> std::sync::Arc<rustls::RootCertStore> {
    let mut store = rustls::RootCertStore::empty();
    // rustls-native-certs 0.8: load_native_certs() returns CertificateResult (not Result)
    let certs_result = rustls_native_certs::load_native_certs();
    for cert in certs_result.certs {
        let _ = store.add(cert);
    }
    std::sync::Arc::new(store)
}

fn rustls_client_config(
    root_store: std::sync::Arc<rustls::RootCertStore>,
    _ip: &str,
) -> std::sync::Arc<rustls::ClientConfig> {
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates((*root_store).clone())
        .with_no_client_auth();
    std::sync::Arc::new(cfg)
}

// ─── Ana komut ────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_dns_benchmark(app: AppHandle) -> Result<DnsBenchmarkResult, String> {
    let list = get_dns_providers(); // OnceLock'tan al, yoksa fallback
    let mut results: Vec<DnsProviderResult> = Vec::with_capacity(list.len());

    // Tüm provider'ları paralel çalıştır
    let app_arc = std::sync::Arc::new(app.clone());
    let mut handles = Vec::with_capacity(list.len());

    for prov in &list {
        let app2      = app_arc.clone();
        let name      = prov.name.clone();
        let ip        = prov.ip.clone();
        let doh_host  = prov.doh_host.clone();
        let doh_path  = prov.doh_path.clone();

        let handle = tokio::spawn(async move {
            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "udp".into(), status: "running".into(), median_ms: 0.0,
            });
            let udp = probe_udp(&ip).await;
            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "udp".into(), status: "done".into(), median_ms: udp.median_ms,
            });

            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "dot".into(), status: "running".into(), median_ms: 0.0,
            });
            let dot = probe_dot(&ip).await;
            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "dot".into(), status: "done".into(), median_ms: dot.median_ms,
            });

            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "doh".into(), status: "running".into(), median_ms: 0.0,
            });
            let doh = probe_doh(&doh_host, &doh_path).await;
            let _ = app2.emit("dns_benchmark:progress", DnsBenchmarkProgress {
                provider: name.clone(), protocol: "doh".into(), status: "done".into(), median_ms: doh.median_ms,
            });

            (name, ip, udp, dot, doh)
        });
        handles.push(handle);
    }

    // Tüm task'leri bekle
    for handle in handles {
        let Ok((name, ip, udp, dot, doh)) = handle.await else { continue; };

        let candidates = [
            ("UDP", &udp),
            ("DoT", &dot),
            ("DoH", &doh),
        ];
        let best = candidates.iter()
            .filter(|(_, r)| r.reachable)
            .min_by(|(_, a), (_, b)| a.median_ms.partial_cmp(&b.median_ms).unwrap_or(std::cmp::Ordering::Equal));

        let (best_proto, best_ms) = match best {
            Some((proto, r)) => (proto.to_string(), r.median_ms),
            None             => ("UDP".into(), 9999.0),
        };

        let avg_loss = (udp.packet_loss + dot.packet_loss) / 2.0;
        let grade    = overall_grade(best_ms, avg_loss);

        results.push(DnsProviderResult {
            name,
            ip,
            udp:        udp.clone(),
            dot:        dot.clone(),
            doh:        doh.clone(),
            best_ms,
            best_proto,
            grade,
            rank: 0,
        });
    }

    // Rank — best_ms'e göre sırala (erişilemeyenler sona)
    results.sort_by(|a, b| {
        let a_ok = a.udp.reachable || a.dot.reachable || a.doh.reachable;
        let b_ok = b.udp.reachable || b.dot.reachable || b.doh.reachable;
        if a_ok && !b_ok { return std::cmp::Ordering::Less; }
        if !a_ok && b_ok { return std::cmp::Ordering::Greater; }
        a.best_ms.partial_cmp(&b.best_ms).unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, r) in results.iter_mut().enumerate() {
        r.rank = (i + 1) as u32;
    }

    let recommended = results.first().map(|r| r.name.clone()).unwrap_or_default();
    let best_ms     = results.first().map(|r| r.best_ms).unwrap_or(0.0);
    let summary     = format!(
        "Fastest: {} ({:.0} ms) — {} providers tested",
        recommended, best_ms, results.len()
    );

    Ok(DnsBenchmarkResult { providers: results, recommended, summary })
}

#[cfg(test)]
mod tests {
    use super::{build_query, dns_providers_fallback, is_valid_dns_response, valid_dns_provider, DnsProviderDef};

    fn prov(ip: &str, doh_host: &str, doh_path: &str) -> DnsProviderDef {
        DnsProviderDef { name: "t".into(), ip: ip.into(), doh_host: doh_host.into(), doh_path: doh_path.into() }
    }

    #[test]
    fn fallback_providerlar_dogrulamadan_gecer() {
        for p in dns_providers_fallback() {
            assert!(valid_dns_provider(&p), "{}", p.name);
        }
    }

    #[test]
    fn provider_dogrulama_gecersizleri_reddeder() {
        assert!(valid_dns_provider(&prov("9.9.9.9", "dns.quad9.net", "/dns-query")));
        assert!(valid_dns_provider(&prov("2606:4700:4700::1111", "", "")));
        assert!(!valid_dns_provider(&prov("192.168.1.1", "dns.example.com", "/dns-query")));
        assert!(!valid_dns_provider(&prov("127.0.0.1", "", "")));
        assert!(!valid_dns_provider(&prov("::ffff:10.0.0.1", "", "")));
        assert!(!valid_dns_provider(&prov("fe80::1", "", "")));
        assert!(!valid_dns_provider(&prov("dns.google", "dns.google", "/dns-query")));
        assert!(!valid_dns_provider(&prov("8.8.8.8", "localhost", "/dns-query")));
        assert!(!valid_dns_provider(&prov("8.8.8.8", "10.0.0.1", "/dns-query")));
        assert!(!valid_dns_provider(&prov("8.8.8.8", "dns.google", "/../x")));
        assert!(!valid_dns_provider(&prov("8.8.8.8", "dns.google", "dns-query")));
        assert!(!valid_dns_provider(&prov("8.8.8.8", "evil.com/x?", "/dns-query")));
    }

    /// Sorgudan gecerli bir cevap uret: ayni id + question, QR=1, verilen RCODE.
    fn make_response(query: &[u8], id: u16, qr: bool, rcode: u8) -> Vec<u8> {
        let mut r = query.to_vec();
        r[0] = (id >> 8) as u8;
        r[1] = (id & 0xFF) as u8;
        r[2] = if qr { 0x81 } else { 0x01 };
        r[3] = 0x80 | (rcode & 0x0F);
        r
    }

    #[test]
    fn dns_cevap_nxdomain_ve_noerror_basarili() {
        let q = build_query(0x1234);
        // .invalid adlari icin beklenen cevap NXDOMAIN (3) -> basari
        assert!(is_valid_dns_response(&q, &make_response(&q, 0x1234, true, 3), 0x1234));
        assert!(is_valid_dns_response(&q, &make_response(&q, 0x1234, true, 0), 0x1234));
    }

    #[test]
    fn dns_cevap_servfail_refused_basarisiz() {
        let q = build_query(0x1234);
        assert!(!is_valid_dns_response(&q, &make_response(&q, 0x1234, true, 2), 0x1234)); // SERVFAIL
        assert!(!is_valid_dns_response(&q, &make_response(&q, 0x1234, true, 5), 0x1234)); // REFUSED
    }

    #[test]
    fn dns_cevap_qr_id_question_dogrulanir() {
        let q = build_query(0x1234);
        // QR=0 (sorgu geri yansidi) -> gecersiz
        assert!(!is_valid_dns_response(&q, &make_response(&q, 0x1234, false, 3), 0x1234));
        // yanlis id
        assert!(!is_valid_dns_response(&q, &make_response(&q, 0x9999, true, 3), 0x1234));
        // farkli question (baska bir sorgunun cevabi)
        let mut resp = make_response(&q, 0x1234, true, 3);
        resp[13] ^= 0x01; // ilk label'in ilk karakterini boz
        assert!(!is_valid_dns_response(&q, &resp, 0x1234));
    }

    #[test]
    fn dns_cevap_kisa_veya_bozuk_basarisiz() {
        let q = build_query(0x1234);
        assert!(!is_valid_dns_response(&q, &[], 0x1234));
        assert!(!is_valid_dns_response(&q, &[0u8; 11], 0x1234));
        // header var ama question yok/kesik
        let full = make_response(&q, 0x1234, true, 3);
        assert!(!is_valid_dns_response(&q, &full[..full.len() - 3], 0x1234));
        // QDCOUNT=0
        let mut z = full.clone();
        z[4] = 0; z[5] = 0;
        assert!(!is_valid_dns_response(&q, &z, 0x1234));
        // rastgele HTTP-200 govdesi (DoH sahte cevap)
        assert!(!is_valid_dns_response(&q, b"<html>not dns at all, just some body</html>", 0x1234));
    }

    #[test]
    fn build_query_tx_id_big_endian() {
        let q = build_query(0xABCD);
        // ilk 2 byte = TX ID (big-endian)
        assert_eq!(q[0], 0xAB);
        assert_eq!(q[1], 0xCD);
    }

    #[test]
    fn build_query_header_dogru() {
        let q = build_query(0x1234);
        // flags: standard query RD=1
        assert_eq!(&q[2..4], &[0x01, 0x00]);
        // QDCOUNT=1, ANCOUNT=0, NSCOUNT=0, ARCOUNT=0
        assert_eq!(&q[4..12], &[0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn build_query_qtype_qclass_sonda() {
        let q = build_query(0x0001);
        // son 4 byte: QTYPE=A (0x0001), QCLASS=IN (0x0001)
        let n = q.len();
        assert_eq!(&q[n - 4..], &[0x00, 0x01, 0x00, 0x01]);
        // root label (0x00) QTYPE'tan hemen once
        assert_eq!(q[n - 5], 0x00);
    }

    #[test]
    fn build_query_sabit_sonek_icerir() {
        let q = build_query(0x0000);
        // "netveil-bench" ve "invalid" label'lari bulunmali
        assert!(q.windows(13).any(|w| w == b"netveil-bench"));
        assert!(q.windows(7).any(|w| w == b"invalid"));
    }
}

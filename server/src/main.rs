use axum::{
    body::Body,
    extract::{Query, Request},
    http::header,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::stream;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UdpSocket};
use tracing::info;

const DEFAULT_DL_BYTES: u64 = 104_857_600; // 100 MB
const MAX_DL_BYTES: u64     = 1_073_741_824; // 1 GB
const CHUNK: usize          = 65_536; // 64 KB

// BT seed dosyası bilgileri — sabit, her zaman aynı torrent
const BT_PIECE_SIZE: u32  = 524_288;  // 512 KB
const BT_PIECE_COUNT: u32 = 128;      // 128 piece = 64 MB
const BT_SEED_PATH: &str      = "/opt/bt-seed/data/netveil-test.bin";
const PING_TARGETS_PATH: &str = "/opt/netveil-server/ping-targets.json";
const IPV6_TARGETS_PATH: &str         = "/opt/netveil-server/ipv6-targets.json";
const TRACEROUTE_PRESETS_PATH: &str   = "/opt/netveil-server/traceroute-presets.json";
const DNS_PROVIDERS_PATH: &str        = "/opt/netveil-server/dns-providers.json";
const OUI_DB_PATH: &str                = "/opt/netveil-server/oui.json";

// ── IP-bazli aktif-test kilidi ───────────────────────────────────────────────
// Amac: ayni anda YALNIZCA 1 KULLANICI (IP) test edebilsin. Bir testin coklu
// baglantisi (DL 16x, UL 6x, BT 8x ...) AYNI IP oldugu icin tek kilidi paylasir.
// Baska bir IP gelirse mesgul (429 / sessiz red) doner.
//
// Eski tasarim (Semaphore per-baglanti, MAX=2) HATALIYDI: tek kullanicinin coklu
// baglantisi birbirini bekletip kendi testini boguyordu. Bu kilit per-KULLANICI.
//
// Deadlock-safe: permit/RAII yok. Her istek kilidi tazeler; LOCK_TTL boyunca
// aktivite gelmezse kilit otomatik serbest (kopan/terk edilen test takmaz).
const LOCK_TTL_SECS: u64 = 45; // test ~35sn; bu sure aktivite yoksa kilit dusrer

static ACTIVE_TEST: OnceLock<Mutex<Option<(IpAddr, Instant)>>> = OnceLock::new();

fn active_test() -> &'static Mutex<Option<(IpAddr, Instant)>> {
    ACTIVE_TEST.get_or_init(|| Mutex::new(None))
}

// ── UDP session generation (cakisma onleme) ──────────────────────────────────
// Her yeni UDP pump session'i (QUIC/WG/OpenVPN) global epoch'u artirir ve kendi
// epoch'unu saklar. Pump dongusu her tick'te "benim epoch hala global mi" bakar;
// degilse durur. Boylece ayni client'in ardisik testleri (WG -> OpenVPN) cakismaz:
// OpenVPN basladiginda WG'nin eski pump'i (STOP gec gelse bile) hemen kendini oldurur.
// Tek-aktif-test garantisini IP-kilidiyle birlikte saglamlastirir.
static UDP_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Yeni bir UDP pump session'i baslat: global generation'i artir, yeni degeri don.
/// Bu degerden farkli generation'a sahip eski pump'lar bir sonraki tick'te durur.
fn next_udp_generation() -> u64 {
    UDP_GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1
}

/// Bu session hala en guncel UDP session'i mi? (pump dongusu icin)
fn udp_generation_current(my_gen: u64) -> bool {
    UDP_GENERATION.load(std::sync::atomic::Ordering::SeqCst) == my_gen
}

/// Kilidi bu IP icin almaya/tazelemeye calisir.
/// true  = izin (kilit bu IP'de, test edebilir)
/// false = mesgul (baska IP aktif test ediyor)
fn try_hold_lock(ip: IpAddr) -> bool {
    let mut guard = active_test().lock().unwrap();
    let now = Instant::now();
    match *guard {
        // Kilit bos -> bu IP al
        None => { *guard = Some((ip, now)); true }
        Some((cur_ip, last)) => {
            if cur_ip == ip {
                // Ayni kullanicinin baska baglantisi -> tazele, izin ver
                *guard = Some((ip, now));
                true
            } else if now.duration_since(last).as_secs() >= LOCK_TTL_SECS {
                // Onceki test terk edilmis/kopmus (TTL gecti) -> devral
                *guard = Some((ip, now));
                true
            } else {
                // Baska IP aktif ve taze -> mesgul
                false
            }
        }
    }
}

/// Kilidi yalnizca bu IP tutuyorsa serbest birakir (test bitince).
/// Baska IP devralmissa dokunmaz (yanlislikla baskasinin kilidini acmaz).
fn release_lock(ip: IpAddr) {
    let mut guard = active_test().lock().unwrap();
    if let Some((cur_ip, _)) = *guard {
        if cur_ip == ip { *guard = None; }
    }
}

// info_hash — ilk bağlantıda hesaplanır, sonrası cache'den
static INFO_HASH: OnceLock<String> = OnceLock::new();
// Seed dosyası tek seferinde memory'e alınır — her piece request'te disk I/O yok.
// 1Gbps'te yüzlerce eş zamanlı request için kritik.
static SEED_DATA: OnceLock<Vec<u8>> = OnceLock::new();

fn get_seed_data() -> &'static [u8] {
    SEED_DATA.get_or_init(|| {
        std::fs::read(BT_SEED_PATH).unwrap_or_default()
    })
}

fn get_info_hash() -> &'static str {
    INFO_HASH.get_or_init(|| {
        // SHA1(bencode(info dict)) — basit sabit torrent için deterministik
        // Torrent bilgileri: name, piece_length, length, pieces (SHA1 of each piece)
        // Biz burada SHA1'i seed dosyasından hesaplıyoruz
        match compute_info_hash() {
            Ok(h) => h,
            Err(_) => "0000000000000000000000000000000000000000".to_string(),
        }
    })
}

fn compute_info_hash() -> Result<String, Box<dyn std::error::Error>> {
    use std::io::Read;
    // Her piece'in SHA1'ini hesapla
    let mut file = std::fs::File::open(BT_SEED_PATH)?;
    let mut pieces_hash = Vec::new();
    let mut buf = vec![0u8; BT_PIECE_SIZE as usize];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        let digest = sha1_bytes(&buf[..n]);
        pieces_hash.extend_from_slice(&digest);
    }

    // Bencode info dict — manuel (bağımlılık olmadan)
    // info = d6:lengthi67108864e4:name20:netveil-test.bin12:piece lengthi524288e6:pieces<N>:<hash bytes>e
    let name = "netveil-test.bin";
    let length = (BT_PIECE_SIZE as u64) * (BT_PIECE_COUNT as u64);
    let pieces_len = pieces_hash.len();

    let mut info_bencode = Vec::new();
    info_bencode.extend_from_slice(b"d");
    info_bencode.extend_from_slice(format!("6:lengthi{}e", length).as_bytes());
    info_bencode.extend_from_slice(format!("4:name{}:{}", name.len(), name).as_bytes());
    info_bencode.extend_from_slice(format!("12:piece lengthi{}e", BT_PIECE_SIZE).as_bytes());
    info_bencode.extend_from_slice(format!("6:pieces{}:", pieces_len).as_bytes());
    info_bencode.extend_from_slice(&pieces_hash);
    info_bencode.extend_from_slice(b"e");

    let hash = sha1_bytes(&info_bencode);
    Ok(hash.iter().map(|b| format!("{:02x}", b)).collect())
}

fn sha1_bytes(data: &[u8]) -> [u8; 20] {
    // SHA1 — pure Rust, bağımlılıksız implementasyon
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;

    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes(chunk[i*4..i*4+4].try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for i in 0..80 {
            let (f, k) = match i {
                0..=19  => ((b & c) | (!b & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d,           0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _       => (b ^ c ^ d,           0xCA62C1D6),
            };
            let temp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(w[i]);
            e = d; d = c; c = b.rotate_left(30); b = a; a = temp;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c); h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for i in 0..5 { out[i*4..i*4+4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Başlangıçta info_hash'i hesapla (seed dosyası varsa)
    info!("info_hash: {}", get_info_hash());
    info!("seed data: {} bytes preloaded", get_seed_data().len());

    tokio::join!(
        run_http_api(),
        run_bt_peer(6881),    // Gerçek BT Wire Protocol peer
        run_quic_udp(9002),         // QUIC-fingerprint UDP throughput server
        run_wireguard_udp(51820),     // WireGuard-fingerprint UDP throughput server
        run_openvpn_udp(1194),        // OpenVPN-fingerprint UDP throughput server
        run_voip_echo(9005),          // VoIP/Gaming UDP echo (gerçek-zamanlı kalite probe)
    );
}

async fn run_http_api() {
    let app = Router::new()
        .route("/health",        get(|| async { "ok" }))
        .route("/ping",          get(|| async { "pong" }))
        .route("/dl",            get(handle_dl))
        .route("/ul",            post(handle_ul))
        .route("/bt/info",       get(handle_bt_info))
        .route("/ping-targets",  get(handle_ping_targets))
        .route("/ipv6-targets",          get(handle_ipv6_targets))
        .route("/traceroute-presets",     get(handle_traceroute_presets))
        .route("/dns-providers",          get(handle_dns_providers))
        .route("/oui",                    get(handle_oui))
        .route("/version",               get(handle_version));

    let addr = SocketAddr::from(([0, 0, 0, 0], 8080));
    info!("HTTP API listening on {}", addr);
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ── /dl endpoint ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DlParams { bytes: Option<u64> }

/// nginx'in eklediği X-Real-IP header'ından gerçek client IP'sini çıkarır.
/// Bulunamazsa loopback döner (guard'ı bypass etmemek için güvenli default).
fn client_ip(headers: &axum::http::HeaderMap) -> IpAddr {
    headers
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.trim().parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]))
}

fn busy_response() -> Response {
    Response::builder()
        .status(429)
        .header("Retry-After", "10")
        .header(header::CONTENT_TYPE, "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from(r#"{"busy":true,"reason":"another_test_active"}"#))
        .unwrap()
}

async fn handle_dl(
    headers: axum::http::HeaderMap,
    Query(p): Query<DlParams>,
) -> Response {
    let ip = client_ip(&headers);
    if !try_hold_lock(ip) {
        return busy_response();
    }
    let total = p.bytes.unwrap_or(DEFAULT_DL_BYTES).min(MAX_DL_BYTES).max(1);
    // Stream periyodik olarak kilidi tazeler (uzun DL boyunca TTL dolmasin).
    // Her chunk'ta degil (~16MB'de bir) — Mutex kilitleme yukunu dusurur.
    let refresh_every = 16 * 1024 * 1024u64;
    let s = stream::unfold((total, ip, 0u64), move |(rem, ip, since)| async move {
        if rem == 0 { release_lock(ip); return None; }
        let mut since = since;
        if since >= refresh_every { try_hold_lock(ip); since = 0; }
        let n = (rem as usize).min(CHUNK);
        since += n as u64;
        Some((Ok::<_, std::convert::Infallible>(bytes::Bytes::from(vec![0xAAu8; n])), (rem - n as u64, ip, since)))
    });
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(Body::from_stream(s))
        .unwrap()
}

// ── /ul endpoint ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct UlResponse { received: u64 }

async fn handle_ul(
    headers: axum::http::HeaderMap,
    req: Request,
) -> Response {
    use http_body_util::BodyExt;
    let ip = client_ip(&headers);
    if !try_hold_lock(ip) {
        return busy_response();
    }
    let mut body = req.into_body();
    let mut received = 0u64;
    let mut since_refresh = 0u64;
    while let Some(frame) = body.frame().await {
        if let Ok(f) = frame {
            if let Ok(data) = f.into_data() {
                received += data.len() as u64;
                // Uzun UL boyunca kilidi periyodik tazele (~16MB'de bir)
                since_refresh += data.len() as u64;
                if since_refresh >= 16 * 1024 * 1024 {
                    try_hold_lock(ip);
                    since_refresh = 0;
                }
            }
        }
    }
    release_lock(ip);
    axum::Json(UlResponse { received }).into_response()
}

// ── /bt/info endpoint ────────────────────────────────────────────────────────

#[derive(Serialize)]
struct BtInfo {
    info_hash:   String,
    piece_count: u32,
    piece_size:  u32,
    host:        String,
    port:        u16,
}

async fn handle_bt_info() -> Json<BtInfo> {
    Json(BtInfo {
        info_hash:   get_info_hash().to_string(),
        piece_count: BT_PIECE_COUNT,
        piece_size:  BT_PIECE_SIZE,
        host:        "netveil.net".to_string(),
        port:        6881,
    })
}

// ── /ping-targets endpoint ───────────────────────────────────────────────────
// ping-targets.json dosyasını okur, JSON olarak döner.
// Dosya editlenirse servis restart gerekmez — her istekte taze okur.

async fn handle_ping_targets() -> impl axum::response::IntoResponse {
    match tokio::fs::read_to_string(PING_TARGETS_PATH).await {
        Ok(contents) => (
            axum::http::StatusCode::OK,
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
                ("access-control-allow-origin", "*"),
            ],
            contents,
        ).into_response(),
        Err(e) => {
            let msg = format!("{{\"error\": \"{}\"}}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                msg,
            ).into_response()
        }
    }
}

// ── /ipv6-targets endpoint ──────────────────────────────────────────────────

async fn handle_ipv6_targets() -> impl axum::response::IntoResponse {
    match tokio::fs::read_to_string(IPV6_TARGETS_PATH).await {
        Ok(contents) => (
            axum::http::StatusCode::OK,
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
                ("access-control-allow-origin", "*"),
            ],
            contents,
        ).into_response(),
        Err(e) => {
            let msg = format!("{{\"error\": \"{}\"}}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                msg,
            ).into_response()
        }
    }
}

// ── /traceroute-presets endpoint ─────────────────────────────────────────────

async fn handle_traceroute_presets() -> impl axum::response::IntoResponse {
    match tokio::fs::read_to_string(TRACEROUTE_PRESETS_PATH).await {
        Ok(contents) => (
            axum::http::StatusCode::OK,
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
                ("access-control-allow-origin", "*"),
            ],
            contents,
        ).into_response(),
        Err(e) => {
            let msg = format!("{{\"error\": \"{}\"}}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                msg,
            ).into_response()
        }
    }
}

// ── /dns-providers endpoint ──────────────────────────────────────────────────

async fn handle_dns_providers() -> impl axum::response::IntoResponse {
    match tokio::fs::read_to_string(DNS_PROVIDERS_PATH).await {
        Ok(contents) => (
            axum::http::StatusCode::OK,
            [
                ("content-type", "application/json"),
                ("cache-control", "no-store"),
                ("access-control-allow-origin", "*"),
            ],
            contents,
        ).into_response(),
        Err(e) => {
            let msg = format!("{{\"error\": \"{}\"}}", e);
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                [("content-type", "application/json")],
                msg,
            ).into_response()
        }
    }
}

// ── /version endpoint ──────────────────────────────────────────────────────────────

async fn handle_version() -> impl axum::response::IntoResponse {
    // Bu değeri her release'de güncelle
    let body = "{\"version\":\"0.9.11\",\"url\":\"https://github.com/Extreme159/NetVeil/releases/latest\",\"notes\":\"\"}";
    (
        axum::http::StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
            ("access-control-allow-origin", "*"),
        ],
        body,
    ).into_response()
}

// ── /oui endpoint (OUI → vendor lookup) ─────────────────────────────────────────
// GİZLİLİK: Client yalnızca MAC'in ilk 3 byte'ını (OUI, örn "74ACB9") gönderir;
// tam MAC ASLA gönderilmez. Sunucu sorguları LOGLAMAZ/SAKLAMAZ — sadece in-memory
// HashMap lookup yapar ve cevap döner (tamamen stateless).
// Toplu sorgu: GET /oui?prefixes=74ACB9,D893D4,...  →  {"74ACB9":"Ubiquiti Inc",...}

static OUI_DB: OnceLock<std::collections::HashMap<String, String>> = OnceLock::new();

fn get_oui_db() -> &'static std::collections::HashMap<String, String> {
    OUI_DB.get_or_init(|| {
        match std::fs::read_to_string(OUI_DB_PATH) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => std::collections::HashMap::new(),
        }
    })
}

#[derive(Deserialize)]
struct OuiParams {
    prefixes: Option<String>,
}

async fn handle_oui(Query(p): Query<OuiParams>) -> impl axum::response::IntoResponse {
    let db = get_oui_db();
    let mut out: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    if let Some(list) = p.prefixes.as_deref() {
        // En fazla 256 prefix işle (kötüye kullanım sınırı)
        for raw in list.split(',').take(256) {
            let prefix = raw.trim().to_uppercase();
            if prefix.len() != 6 || !prefix.chars().all(|c| c.is_ascii_hexdigit()) {
                continue; // yalnızca 6-hane hex OUI kabul edilir
            }
            if let Some((k, v)) = db.get_key_value(&prefix) {
                out.insert(k.as_str(), v.as_str());
            }
        }
    }
    let body = serde_json::to_string(&out).unwrap_or_else(|_| "{}".to_string());
    (
        axum::http::StatusCode::OK,
        [
            ("content-type", "application/json"),
            ("cache-control", "no-store"),
            ("access-control-allow-origin", "*"),
        ],
        body,
    ).into_response()
}

// ── BitTorrent Wire Protocol Peer ────────────────────────────────────────────
// NetVeil client BT handshake yapıp piece request atar.
// Biz de gerçek BT mesajları dönerek throughput sağlarız.

async fn run_bt_peer(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    info!("BT peer listening on port {}", port);
    loop {
        let (socket, peer) = match listener.accept().await { Ok(v) => v, Err(_) => continue };
        info!("BT connection from {}", peer);
        tokio::spawn(async move {
            let ip = peer.ip();
            if !try_hold_lock(ip) { info!("BT {} rejected: server busy", peer); return; }
            handle_bt_connection(socket).await;
            release_lock(ip);
        });
    }
}

async fn handle_bt_connection(mut stream: tokio::net::TcpStream) {
    let _ = stream.set_nodelay(true);

    // 1. Handshake al (68 byte)
    let mut hs = vec![0u8; 68];
    if tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_exact(&mut hs),
    ).await.is_err() { return; }

    // Protokol string doğrula
    if hs[0] != 19 || &hs[1..20] != b"BitTorrent protocol" {
        return;
    }

    // 2. Handshake cevabı gönder
    let mut resp_hs = Vec::with_capacity(68);
    resp_hs.push(19u8);
    resp_hs.extend_from_slice(b"BitTorrent protocol");
    resp_hs.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00, 0x01]); // extensions
    resp_hs.extend_from_slice(&hs[28..48]); // info_hash echo
    resp_hs.extend_from_slice(b"-ND0100-netveilserv0"); // peer_id
    if stream.write_all(&resp_hs).await.is_err() { return; }

    // 3. bitfield gönder — tüm piece'lere sahibiz
    let bitfield_bytes = (BT_PIECE_COUNT as usize + 7) / 8;
    let mut bitfield_msg = Vec::new();
    bitfield_msg.extend_from_slice(&((bitfield_bytes + 1) as u32).to_be_bytes()); // len
    bitfield_msg.push(5u8); // msg_id = bitfield
    bitfield_msg.extend_from_slice(&vec![0xFFu8; bitfield_bytes]);
    if stream.write_all(&bitfield_msg).await.is_err() { return; }

    // 4. Mesaj döngüsü
    let mut unchoked = false;
    let mut buf = vec![0u8; 17]; // en küçük mesaj 4+1=5, request 4+13=17

    loop {
        // Mesaj uzunluğu (4 byte)
        let mut len_buf = [0u8; 4];
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.read_exact(&mut len_buf),
        ).await {
            Ok(Ok(_)) => {}
            _ => break,
        }

        let msg_len = u32::from_be_bytes(len_buf) as usize;
        if msg_len == 0 { continue; } // keepalive

        if msg_len > 17 { // beklenmedik büyük mesaj, atla
            let mut skip = vec![0u8; msg_len.min(65536)];
            let _ = stream.read_exact(&mut skip).await;
            continue;
        }

        if buf.len() < msg_len { buf.resize(msg_len, 0); }
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            stream.read_exact(&mut buf[..msg_len]),
        ).await {
            Ok(Ok(_)) => {}
            _ => break,
        }

        match buf[0] {
            2 => {
                // interested — unchoke gönder
                let unchoke = [0u8, 0, 0, 1, 1]; // [len=1][msg_id=1]
                if stream.write_all(&unchoke).await.is_err() { break; }
                unchoked = true;
            }
            6 if unchoked && msg_len == 13 => {
                // request — [msg_id=6][index 4][begin 4][length 4]
                let index  = u32::from_be_bytes(buf[1..5].try_into().unwrap());
                let begin  = u32::from_be_bytes(buf[5..9].try_into().unwrap());
                let length = u32::from_be_bytes(buf[9..13].try_into().unwrap());

                // piece mesajı gönder — gerçek dosyadan oku (varsa), yoksa sıfır
                let data = read_piece_block(index, begin, length);
                let piece_len = (9 + data.len()) as u32;
                let mut piece_msg = Vec::with_capacity(4 + piece_len as usize);
                piece_msg.extend_from_slice(&piece_len.to_be_bytes());
                piece_msg.push(7u8); // msg_id = piece
                piece_msg.extend_from_slice(&index.to_be_bytes());
                piece_msg.extend_from_slice(&begin.to_be_bytes());
                piece_msg.extend_from_slice(&data);
                if stream.write_all(&piece_msg).await.is_err() { break; }
            }
            _ => {}
        }
    }
}

fn read_piece_block(index: u32, begin: u32, length: u32) -> Vec<u8> {
    // OnceLock ile memory'deki seed'den oku — disk I/O yok
    let data = get_seed_data();
    let offset = ((index as u64 * BT_PIECE_SIZE as u64) + begin as u64) as usize;
    let length = (length as usize).min(BT_PIECE_SIZE as usize);
    if !data.is_empty() && offset + length <= data.len() {
        return data[offset..offset + length].to_vec();
    }
    // Dosya yoksa sıfır döndür
    vec![0xAAu8; length]
}

// ── QUIC-fingerprint UDP Throughput Server ────────────────────────────────────
//
// Protokol:
//   1. Client → Server: QUIC Long Header Initial Packet (RFC 9000 §17.2)
//      Byte 0 = 0xC0, bytes 1-4 = QUIC v1 (0x00000001)
//      Bu parmak izi ISP DPI'ının QUIC trafiği olarak sınıflandırmasını sağlar.
//
//   2. Server → Client: Accept — [0xF0][session_id 8 byte LE]
//
//   3. Client → Server: DATA_REQ — [0xD0][session_id 8 byte]
//
//   4. Server → Client: DATA datagramları — [0xD1][seq 8 byte LE][payload ~1191 byte]
//      1200 byte/datagram (QUIC PMTU-safe), 30sn veya STOP'a kadar.
//
//   5. Client → Server: STOP — [0xFF][session_id 8 byte]

const QUIC_MTU: usize       = 1200;
const QUIC_HEADER_SZ: usize = 9;   // prefix(1) + seq(8)
const QUIC_PAYLOAD: usize   = QUIC_MTU - QUIC_HEADER_SZ;
const SESSION_TIMEOUT_SECS: u64 = 35;

async fn run_quic_udp(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = match UdpSocket::bind(addr).await {
        Ok(s) => {
            info!("QUIC-UDP listening on port {}", port);
            std::sync::Arc::new(s)
        }
        Err(e) => {
            info!("QUIC-UDP bind failed on port {}: {}", port, e);
            return;
        }
    };
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        if n < 5 { continue; }
        let first_byte = buf[0];
        let sock2 = sock.clone();
        // QUIC Long Header Initial Packet: ilk byte 0xC0 veya 0xC3 (fixed bit set),
        // ve version field tam olarak 0x00000001 olmali.
        // 0xD0 (DATA_REQ) de 0xC0 maskesini gectiginden n==1200 ile ayirt ediyoruz.
        if first_byte == 0xC0 && n >= 5 && &buf[1..5] == &[0x00, 0x00, 0x00, 0x01] && n >= 100 {
            info!("QUIC handshake from {}", peer);
            let init = buf[..n].to_vec();
            tokio::spawn(async move {
                let ip = peer.ip();
                if !try_hold_lock(ip) { info!("QUIC {} rejected: server busy", peer); return; }
                handle_quic_session(sock2, peer, init).await;
                release_lock(ip);
            });
        }
    }
}

async fn handle_quic_session(sock: std::sync::Arc<UdpSocket>, peer: SocketAddr, init: Vec<u8>) {
    use std::time::Duration;

    // rate_kbps: QUIC Initial Packet'teki DCID'nin ilk 4 byte'u (LE)
    // Client baseline'a göre hesaplayıp buraya gömer. 0 ise sınırsız.
    let rate_kbps: u32 = if init.len() >= 10 {
        u32::from_le_bytes(init[6..10].try_into().unwrap_or([0u8; 4]))
    } else { 0 };

    let session_id: u64 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64
    };
    // 1. Accept: [0xF0][session_id 8 byte LE]
    let mut accept = Vec::with_capacity(9);
    accept.push(0xF0u8);
    accept.extend_from_slice(&session_id.to_le_bytes());
    if sock.send_to(&accept, peer).await.is_err() { return; }
    // DATA_REQ bekleme yok — rate initial packet'ten okundu, direkt pump.
    info!("QUIC session {} — pumping to {}, rate_kbps={}", session_id, peer, rate_kbps);
    let my_gen = next_udp_generation();

    // 3. DATA: [0xD1][seq 8 byte LE][payload 1191 byte]
    //
    // Rate limiting: interval-based batch sender
    // -------------------------------------------------------------------------
    // Eski yontem (per-pkt token bucket + sleep) sorunu:
    //   Yuksek hizda (900 Mbps) her pkt arasi ~10us beklemeli,
    //   ama Tokio/kernel min sleep granularitesi ~1ms -> rate tutmuyor.
    // Yeni yontem: 2ms tick'te kac pkt gonderilmeli hesapla, hepsini burst at.
    //   Fractional accumulator: kesirli pkt sayisini biriktir (20 Mbps icin kritik).
    // -------------------------------------------------------------------------
    let rate_bytes_per_sec = if rate_kbps > 0 {
        rate_kbps as f64 * 1000.0   // KB/s -> bytes/s
    } else {
        // rate_kbps=0: sinirsiz, NIC kapasitesine birak
        125_000_000.0  // 1 Gbps
    };

    const TICK_MS: u64 = 2;
    let pkts_per_tick = rate_bytes_per_sec * (TICK_MS as f64 / 1000.0) / QUIC_MTU as f64;

    // Pre-allocated paket tamponu: her gonderimd alloc yok
    let mut pkt = vec![0u8; QUIC_MTU];
    pkt[0] = 0xD1u8;
    pkt[9..].fill(0xAAu8);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(SESSION_TIMEOUT_SECS);
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    let mut seq: u64   = 0;
    let mut accum: f64 = 0.0;  // kesirli pkt biriktiricisi
    let mut ctrl = vec![0u8; 32];

    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline { break; }
        // Ayni client yeni bir UDP testi baslattiysa bu eski session'i durdur (cakisma onleme).
        if !udp_generation_current(my_gen) {
            info!("QUIC session {} superseded, stopping", session_id);
            break;
        }

        accum += pkts_per_tick;
        let send_count = accum as usize;
        accum -= send_count as f64;

        for _ in 0..send_count {
            pkt[1..9].copy_from_slice(&seq.to_le_bytes());
            seq += 1;
            if sock.send_to(&pkt, peer).await.is_err() { return; }
        }

        // Her tick'te STOP kontrol (non-blocking, 50us timeout)
        match tokio::time::timeout(Duration::from_micros(50), sock.recv_from(&mut ctrl)).await {
            Ok(Ok((n, src))) if src == peer && n >= 1 && ctrl[0] == 0xFF => {
                info!("QUIC session {} STOP at {} pkts", session_id, seq);
                return;
            }
            _ => {}
        }
    }
    info!("QUIC session {} done, {} datagrams -> {}", session_id, seq, peer);
}

// ── WireGuard-fingerprint UDP Throughput Server ───────────────────────────
//
// Protokol:
//   1. Client → Server: WireGuard Handshake Initiation (whitepaper §5.4.2)
//      type=0x01, 148 byte, Curve25519 ephemeral key içerir.
//      ISP DPI bu yapıyı WireGuard VPN trafiği olarak sınıflandırır.
//
//   2. Server → Client: WG-style Accept — [0xF2][session_id 8 byte LE]
//      (type=0x02 WG Handshake Response formatı taklit edilir)
//
//   3. Client → Server: DATA_REQ — [0xD0][session_id 8 byte]
//
//   4. Server → Client: DATA datagramları — WireGuard transport paketi formatı
//      [0x04][receiver_index 4B][counter 8B][payload]
//      Max 1420 byte (WireGuard MTU). 30sn veya STOP'a kadar.
//
//   5. Client → Server: STOP — [0xFF][session_id 8 byte]
//
// ISP DPI tetikleyici:
//   - UDP port 51820 (standart WireGuard portu)
//   - İlk paket: type=0x01 + 148 byte boyut = WG Handshake Initiation imzası
//   - Sonraki trafik: type=0x04 (transport) formatı, 1420 byte datagramlar

const WG_MTU: usize           = 1420; // WireGuard MTU (IP MTU 1500 - 80 byte WG overhead)
const WG_HEADER_SZ: usize     = 16;   // type(1) + reserved(3) + receiver(4) + counter(8)
const WG_PAYLOAD: usize       = WG_MTU - WG_HEADER_SZ;
const WG_SESSION_TIMEOUT: u64 = 35;

async fn run_wireguard_udp(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = match UdpSocket::bind(addr).await {
        Ok(s) => {
            info!("WireGuard-UDP listening on port {}", port);
            std::sync::Arc::new(s)
        }
        Err(e) => {
            info!("WireGuard-UDP bind failed on port {}: {}", port, e);
            return;
        }
    };

    let mut buf = vec![0u8; 2048];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        // WireGuard Handshake Initiation: type=0x01, boyut 148 byte
        if n == 148 && buf[0] == 0x01 && buf[1] == 0x00 && buf[2] == 0x00 && buf[3] == 0x00 {
            info!("WireGuard handshake from {}", peer);
            let sock2 = sock.clone();
            let init = buf[..n].to_vec();
            tokio::spawn(async move {
                let ip = peer.ip();
                if !try_hold_lock(ip) { info!("WG {} rejected: server busy", peer); return; }
                handle_wireguard_session(sock2, peer, init).await;
                release_lock(ip);
            });
        }
    }
}

async fn handle_wireguard_session(
    sock: std::sync::Arc<UdpSocket>,
    peer: SocketAddr,
    init: Vec<u8>,
) {
    use std::time::Duration;

    // rate_kbps: client'in handshake'teki sender_index alanına gömdüğü değer
    // (byte 4-7, little-endian). 0 ise sınırsız.
    let rate_kbps = u32::from_le_bytes(init[4..8].try_into().unwrap_or([0u8; 4]));

    let session_id: u64 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64
    };
    let receiver_index = (session_id & 0xFFFFFFFF) as u32;

    // 1. Accept: [0xF2][session_id 8B]
    let mut accept = Vec::with_capacity(9);
    accept.push(0xF2u8);
    accept.extend_from_slice(&session_id.to_le_bytes());
    if sock.send_to(&accept, peer).await.is_err() { return; }
    // DATA_REQ bekleme yok — rate handshake'ten okundu, direkt pump başlat.
    info!("WG session {} — pumping to {}, rate_kbps={}", session_id, peer, rate_kbps);
    let my_gen = next_udp_generation();

    // 3. WireGuard Transport Data: [0x04][reserved 3B][receiver_index 4B][counter 8B][payload]
    //
    // Rate limiting: interval-based batch sender (QUIC ile ayni mantik)
    let rate_bytes_per_sec = if rate_kbps > 0 {
        rate_kbps as f64 * 1000.0
    } else {
        125_000_000.0  // 1 Gbps
    };

    const TICK_MS: u64 = 2;
    let pkts_per_tick = rate_bytes_per_sec * (TICK_MS as f64 / 1000.0) / WG_MTU as f64;

    // Pre-allocated paket tamponu
    let mut pkt = vec![0u8; WG_MTU];
    pkt[0] = 0x04u8;
    // reserved: pkt[1..4] = 0x00 (zaten sifir)
    pkt[4..8].copy_from_slice(&receiver_index.to_le_bytes());
    pkt[16..].fill(0xAAu8);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(WG_SESSION_TIMEOUT);
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    let mut counter: u64 = 0;
    let mut accum:   f64 = 0.0;
    let mut ctrl = vec![0u8; 32];

    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline { break; }
        if !udp_generation_current(my_gen) {
            info!("WG session {} superseded, stopping", session_id);
            break;
        }

        accum += pkts_per_tick;
        let send_count = accum as usize;
        accum -= send_count as f64;

        for _ in 0..send_count {
            // counter her pakette degisiyor, in-place yaz
            pkt[8..16].copy_from_slice(&counter.to_le_bytes());
            counter += 1;
            if sock.send_to(&pkt, peer).await.is_err() { return; }
        }

        // Her tick'te STOP kontrol
        match tokio::time::timeout(Duration::from_micros(50), sock.recv_from(&mut ctrl)).await {
            Ok(Ok((n, src))) if src == peer && n >= 1 && ctrl[0] == 0xFF => {
                info!("WG session {} STOP at {} pkts", session_id, counter);
                return;
            }
            _ => {}
        }
    }
    info!("WG session {} done, {} datagrams -> {}", session_id, counter, peer);
}

// ── OpenVPN-fingerprint UDP throughput server (port 1194) ────────────────────
//
// Akis (WG ile AYNI veri formati; yalniz tetikleyici imza OpenVPN):
//   1. Client -> Server: OpenVPN P_CONTROL_HARD_RESET_CLIENT_V2
//      [0x38][session_id 8B][packet_id_array_len 1B=0][msg_packet_id 4B BE = rate_kbps]
//   2. Server -> Client: [0xF2][session_id 8B]   (accept)
//   3. Server -> Client: [0x04][reserved 3B][recv_idx 4B][counter 8B][payload] x N
//   4. Client -> Server: [0xFF][session_id 8B]   (STOP)
//
// ISP DPI tetikleyici: UDP port 1194 + opcode 0x38 (P_CONTROL_HARD_RESET_CLIENT_V2).
// Veri akisi WG transport formatiyla ayni — DPI ilk paketin port+opcode imzasina bakar.

async fn run_openvpn_udp(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = match UdpSocket::bind(addr).await {
        Ok(s) => {
            info!("OpenVPN-UDP listening on port {}", port);
            std::sync::Arc::new(s)
        }
        Err(e) => {
            info!("OpenVPN-UDP bind failed on port {}: {}", port, e);
            return;
        }
    };

    let mut buf = vec![0u8; 2048];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };

        // OpenVPN P_CONTROL_HARD_RESET_CLIENT_V2: ilk byte opcode 0x38, >=14 byte.
        if n >= 14 && buf[0] == 0x38 {
            info!("OpenVPN handshake from {}", peer);
            let sock2 = sock.clone();
            let init = buf[..n].to_vec();
            tokio::spawn(async move {
                let ip = peer.ip();
                if !try_hold_lock(ip) { info!("OpenVPN {} rejected: server busy", peer); return; }
                handle_openvpn_session(sock2, peer, init).await;
                release_lock(ip);
            });
        }
    }
}

async fn handle_openvpn_session(
    sock: std::sync::Arc<UdpSocket>,
    peer: SocketAddr,
    init: Vec<u8>,
) {
    use std::time::Duration;

    // rate_kbps: client'in hard-reset paketine BE gomdugu deger (byte 10..14).
    // Duzen: opcode(1) + session_id(8) + packet_id_array_len(1) + msg_packet_id(4 BE).
    let rate_kbps = u32::from_be_bytes(init[10..14].try_into().unwrap_or([0u8; 4]));

    let session_id: u64 = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64
    };
    let receiver_index = (session_id & 0xFFFFFFFF) as u32;

    // 1. Accept: [0xF2][session_id 8B]
    let mut accept = Vec::with_capacity(9);
    accept.push(0xF2u8);
    accept.extend_from_slice(&session_id.to_le_bytes());
    if sock.send_to(&accept, peer).await.is_err() { return; }
    info!("OpenVPN session {} — pumping to {}, rate_kbps={}", session_id, peer, rate_kbps);
    let my_gen = next_udp_generation();

    // 2. Transport Data: [0x04][reserved 3B][receiver_index 4B][counter 8B][payload]
    //    (WG ile ayni format — interval-based batch sender)
    let rate_bytes_per_sec = if rate_kbps > 0 {
        rate_kbps as f64 * 1000.0
    } else {
        125_000_000.0  // 1 Gbps
    };

    const TICK_MS: u64 = 2;
    let pkts_per_tick = rate_bytes_per_sec * (TICK_MS as f64 / 1000.0) / WG_MTU as f64;

    let mut pkt = vec![0u8; WG_MTU];
    pkt[0] = 0x04u8;
    pkt[4..8].copy_from_slice(&receiver_index.to_le_bytes());
    pkt[16..].fill(0xAAu8);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(WG_SESSION_TIMEOUT);
    let mut ticker = tokio::time::interval(Duration::from_millis(TICK_MS));
    let mut counter: u64 = 0;
    let mut accum:   f64 = 0.0;
    let mut ctrl = vec![0u8; 32];

    loop {
        ticker.tick().await;
        if tokio::time::Instant::now() >= deadline { break; }
        if !udp_generation_current(my_gen) {
            info!("OpenVPN session {} superseded, stopping", session_id);
            break;
        }

        accum += pkts_per_tick;
        let send_count = accum as usize;
        accum -= send_count as f64;

        for _ in 0..send_count {
            pkt[8..16].copy_from_slice(&counter.to_le_bytes());
            counter += 1;
            if sock.send_to(&pkt, peer).await.is_err() { return; }
        }

        match tokio::time::timeout(Duration::from_micros(50), sock.recv_from(&mut ctrl)).await {
            Ok(Ok((n, src))) if src == peer && n >= 1 && ctrl[0] == 0xFF => {
                info!("OpenVPN session {} STOP at {} pkts", session_id, counter);
                return;
            }
            _ => {}
        }
    }
    info!("OpenVPN session {} done, {} datagrams -> {}", session_id, counter, peer);
}



// ─── VoIP/Gaming UDP Echo (port 9005) ────────────────────────────────────────
// Gerçek-zamanlı kalite ölçümü: istemci RTP/oyun-benzeri küçük UDP probe gönderir,
// sunucu HER probe'u OLDUĞU GİBİ geri yansıtır. İstemci RTT/jitter(RFC3550)/loss hesaplar.
// Throughput pump YOK (bant genişliği doyurulmaz — kalite ölçer, kapasite değil).
//
// Protokol:
//   Client → [0xE0][session_id 8B LE]              (handshake)
//   Server → [0xE1][session_id 8B LE]              (accept)
//   Client → [0xE2][seq 4B][ts_us 8B][mode 1B][pad] (probe)
//   Server → [0xE2][seq 4B][ts_us 8B][mode 1B][pad] (echo — aynı paket)
//   Client → [0xFF][session_id 8B LE]              (STOP)
//
// SESSION_TIMEOUT_SECS (35) kullanılır — VoIP toplam ~31.5sn (3 mod x 10sn + drain).

const VOIP_ECHO_TIMEOUT: u64 = 35;

async fn run_voip_echo(port: u16) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = match UdpSocket::bind(addr).await {
        Ok(s) => {
            info!("VoIP-Echo listening on port {}", port);
            std::sync::Arc::new(s)
        }
        Err(e) => {
            info!("VoIP-Echo bind failed on port {}: {}", port, e);
            return;
        }
    };
    let mut buf = vec![0u8; 2048];
    loop {
        let (n, peer) = match sock.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Handshake: [0xE0][session_id 8B]
        if n >= 9 && buf[0] == 0xE0 {
            let init = buf[..n].to_vec();
            let sock2 = sock.clone();
            tokio::spawn(async move {
                let ip = peer.ip();
                if !try_hold_lock(ip) {
                    info!("VoIP {} rejected: server busy", peer);
                    return;
                }
                handle_voip_echo(sock2, peer, init).await;
                release_lock(ip);
            });
        }
    }
}

async fn handle_voip_echo(sock: std::sync::Arc<UdpSocket>, peer: SocketAddr, init: Vec<u8>) {
    use std::time::Duration;

    // session_id'yi handshake'ten al, accept'te aynen geri yolla
    let session_id: u64 = u64::from_le_bytes(init[1..9].try_into().unwrap_or([0u8; 8]));

    // Accept: [0xE1][session_id 8B LE]
    let mut accept = Vec::with_capacity(9);
    accept.push(0xE1u8);
    accept.extend_from_slice(&session_id.to_le_bytes());
    if sock.send_to(&accept, peer).await.is_err() { return; }
    info!("VoIP echo session {} ← {}", session_id, peer);

    let my_gen = next_udp_generation();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(VOIP_ECHO_TIMEOUT);
    let mut buf = vec![0u8; 2048];
    let mut echoed: u64 = 0;

    loop {
        if tokio::time::Instant::now() >= deadline { break; }
        // Aynı client yeni test başlattıysa eski session'ı durdur.
        if !udp_generation_current(my_gen) {
            info!("VoIP session {} superseded", session_id);
            break;
        }
        // 200ms timeout ile probe bekle (deadline/generation kontrolü için periyodik uyanma)
        match tokio::time::timeout(Duration::from_millis(200), sock.recv_from(&mut buf)).await {
            Ok(Ok((n, src))) if src == peer => {
                if n >= 1 && buf[0] == 0xE2 {
                    // Probe → aynı paketi geri yansıt (echo)
                    let _ = sock.send_to(&buf[..n], peer).await;
                    echoed += 1;
                } else if n >= 1 && buf[0] == 0xFF {
                    info!("VoIP session {} STOP at {} echoes", session_id, echoed);
                    return;
                }
            }
            _ => {} // timeout veya başka peer → döngü devam (deadline/gen kontrolü)
        }
    }
    info!("VoIP session {} done, {} echoes → {}", session_id, echoed, peer);
}

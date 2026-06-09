# netveil-server

NetVeil uygulamasının backend sunucusu. Frankfurt VPS üzerinde çalışıyor (netveil.net).

## Endpoints

| Endpoint | Açıklama |
|----------|----------|
| `GET /ping` | RTT ölçümü |
| `GET /dl?bytes=N` | Download throughput stream |
| `POST /ul` | Upload throughput consumer |
| `GET /bt/info` | BitTorrent metadata (info_hash, piece_count, piece_size) |
| `GET /ping-targets` | Ping hedef listesi (ping-targets.json'dan okunur) |
| `TCP :6881` | BitTorrent Wire Protocol peer |
| `UDP :9002` | QUIC-fingerprint throughput server |
| `UDP :51820` | WireGuard-fingerprint throughput server |

## Kurulum (sıfırdan)

```bash
# Rust kur
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Dosyaları kopyala
mkdir -p /opt/netveil-server/src
cp src/main.rs /opt/netveil-server/src/main.rs
cp Cargo.toml /opt/netveil-server/Cargo.toml
cp ping-targets.json /opt/netveil-server/ping-targets.json

# Build
cd /opt/netveil-server && cargo build --release

# Servis kur
cp netveil-server.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable netveil-server
systemctl start netveil-server
```

## Güncelleme

```bash
# Local'den sunucuya gönder
scp server/src/main.rs root@netveil.net:/opt/netveil-server/src/main.rs

# Sunucuda build + restart
ssh root@netveil.net "cd /opt/netveil-server && /root/.cargo/bin/cargo build --release && systemctl restart netveil-server"
```

## ping-targets.json

Servis restart gerekmez — her `/ping-targets` isteğinde dosyadan taze okunur.
Düzenleyip kaydetmek yeterli.

## Notlar

- TLS: Caddy reverse proxy üzerinden (port 443 → 8080)
- Rate limiting: Client baseline Mbps'i handshake paketine gömer, sunucu token bucket ile uygular
  - WireGuard: `sender_index` alanı (byte 4-7) = rate_kbps LE
  - QUIC: `DCID` ilk 4 byte = rate_kbps LE

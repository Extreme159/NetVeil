# NetVeil

> **Gelişmiş ağ tanılaması** — hızlı sağlık kontrolünden derin, uzman düzeyi analize kadar.

NetVeil; ping, jitter, paket kaybı, bufferbloat, hız, ISP throttle/müdahale, DNS sızıntısı ve daha fazlasını **tek bir uygulamada** ölçer. Sonuçları sadece sayı olarak değil, **"bu senin için ne anlama geliyor"** şeklinde açık bir dille anlatır.

**Neden NetVeil?**
- 🎯 **Tek tıkla teşhis** — bütün testleri çalıştırır, harf notu + net açıklama verir
- 🔍 **Rakiplerde olmayan testler** — ISP'nin BitTorrent/QUIC/WireGuard/OpenVPN trafiğini yavaşlatıp yavaşlatmadığını protokol seviyesinde tespit eder
- 💬 **Anlaşılır** — "45ms jitter" değil, "rekabetçi oyunlarda ara sıra takılma yaşayabilirsin"
- 🪶 **Windows kurulum paketi (NSIS)** — NetVeil'i kurarak kullan; uygulama WebView2 kullanır
- 🔒 **Dürüst** — ölçemediği şeyi "ölçtüm" demez, uydurma skor üretmez

![Version](https://img.shields.io/badge/version-0.9.23-purple)
![Platform](https://img.shields.io/badge/platform-Windows-blue)
![License](https://img.shields.io/badge/license-BSL%201.1-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-orange)

---

## ❓ NetVeil Hangi Sorulara Cevap Verir

Aşağıdakilerden birini hiç sorduysan, NetVeil sana gerçek bir cevap vermek için tasarlandı:

- **ISP'm hızımı kısıyor mu?** — NetVeil; BitTorrent, QUIC, WireGuard ve OpenVPN trafiğini protokol seviyesinde baseline HTTPS hızınla karşılaştırır.
- **Oyun ping'im neden yüksek?** — Oyun sunucularına gerçek ICMP ping + jitter, paket kaybı ve bufferbloat, rekabetçi eşiklere göre notlandırılır.
- **Bufferbloat nedir ve nasıl düzeltilir?** — Boş vs. yüklü gecikme ölçümü + suçlu segment teşhisi (sorun router'ında mı yoksa ISP'nde mi?).
- **CGNAT arkasında mıyım?** — Port yönlendirmeyi bozan taşıyıcı-düzeyi NAT'ı tespit eder.
- **DNS'im sızıyor mu?** — DNS/UDP trafiğinin VPN tüneli dışına çıkıp çıkmadığını denetler.
- **Bağlantımı dinleyen bir şey mi var?** — DNS hijacking, transparent proxy, SNI filtreleme, MITM sertifikaları ve daha fazlası.

NetVeil **Windows**'ta çalışır, **yerel** çalışır (hesap yok, telemetri yok, bulut skorlama yok) ve **open-core / ücretsiz**'tir. Daha fazlası için [rehberlerimize](https://www.netveil.net/blog) bak veya [diğer araçlarla karşılaştırmasını](https://www.netveil.net/compare) gör.

---

## ✨ Ne Yapar?

### 🧠 Tanılama Türleri

NetVeil ihtiyaçlarınıza göre üç teşhis modu sunar:

**Hızlı Tanı (Quick Diagnosis)** (~2.5 dk)
- Genel ağ sağlığı kontrolü, temel metriklerle
- Ping, jitter, paket kaybı, hız, bufferbloat
- IPv6 bağlantısı, DNS benchmark, MTU keşfi
- Günlük hızlı kontroller için ideal

**Oyun Tanısı (Gaming Diagnosis)** (~2.5 dk)
- Gerçek-zamanlı eşiklerle rekabetçi FPS skoru
- Oyuncular için sıkı derecelendirme standartları
- Quick ile aynı temel metrikler + oyun odaklı analiz

**Gelişmiş Tanı (Advanced Diagnosis)** (~4.5 dk)
- Tam kapsamlı uzman analizi
- ISP throttle tespiti, ISP müdahale kontrolleri, CGNAT, VoIP kalitesi dahil
- PDF rapor dışa aktarma
- Derin sorun giderme için eksiksiz resim

Tüm skorlama **yerel/offline** — LLM yok, API çağrısı yok, uydurma skor yok.

### 🏓 Ping & Gecikme
- Oyun sunucularına (CS2, Valorant, LoL, Apex vb.) ve popüler servislere gerçek **ICMP ping**
- Saf Ping aracı (IPv4/IPv6, istediğin host'a)
- Jitter (kararlılık), min/avg/max, paket kaybı, harf notu
- Hedef listesi sunucudan dinamik çekilir — uygulama güncellemesi gerekmez

### ⚡ Hız Testi
- **Cloudflare** veya kendi **netveil.net** sunucumuz (Frankfurt, 1 Gbps)
- 8 paralel akış, canlı animasyonlu gauge, p90 + peak Mbps

### 📊 Bufferbloat (yük altında gecikme)
- Endüstri standardı metodoloji: boşta / indirme / yükleme / çift yönlü fazlar
- Gerçek RTT ölçümü + **suçlu segment teşhisi** (PC↔Router mı, ISP mi?)

### 🔒 ISP Throttle Tespiti ⭐ *(rakiplerde nadir)*
İnternet sağlayıcının belirli trafiği yavaşlatıp yavaşlatmadığını **protokol seviyesinde** ölçer:
- **BitTorrent · QUIC · WireGuard · OpenVPN** — her biri baseline HTTPS hızıyla karşılaştırılır
- %30+ sapma = throttle, %80+ = engelleme. Gerçek protokol parmak izleri kullanılır (sahte değil)

### 🚨 ISP Müdahale Tespiti ⭐
Ağının arasına girilip girilmediğini denetler:
- **DNS Hijacking** (5 domain çoğunluk oylaması) · **Transparent Proxy** · **SNI Filtreleme**
- **MITM Sertifika** (sisteme enjekte CA tespiti) · **DoH/DoT erişimi** · **Port erişimi** · **VPN/Tor engeli**
- **Captive Portal** · **DNSSEC doğrulama** · **ECH (Encrypted Client Hello) teslimi**

### 🕵️ DNS & IP Sızıntı Denetimi
- DNS sorguların gerçek bağlantından mı sızıyor (VPN kullanıyorsan kritik)
- UDP trafiğinin çıkış IP'si kontrolü (split-tunnel tespiti)
- *Not: Tarayıcı WebRTC sızıntısını ölçmediğimizi açıkça belirtiriz — dürüstlük*

### 🌐 Diğer Araçlar
- **IPv6 Tanılaması** — bağlantı türü (Native/Tunnel), dual-stack RTT, ISP IPv6 kalitesi
- **Wi-Fi Kanal Analizi** — 2.4/5/6 GHz tarama, kanal yoğunluğu haritası, en iyi kanal önerisi
- **DNS Benchmark** — UDP/DoT/DoH ile sağlayıcı hız karşılaştırması (Cloudflare, Google, Quad9...)
- **MTU/MSS Keşfi** — yol MTU'su (PPPoE 1492 vb. tespiti)
- **CGNAT Tespiti** — operatörün seni CGNAT arkasına koyup koymadığı (port yönlendirme etkiler)
- **Rota İzleme** — hop-by-hop traceroute, waterfall görünüm
- **Ağ Kararlılık Monitörü** — arka planda kopma/RTT takibi, tray bildirimi
- **Twitch Yayın Testi** — yayıncılar için RTMP ingest bant genişliği + RTT (kanalı canlıya çıkarmaz)
- **VoIP Kalitesi** — VoIP hazırlık değerlendirmesi
- **LAN Tarayıcısı** — yerel ağdaki cihazları tarar
- **LAN Hızı** — yerel ağ verim ölçümü

### 📈 Gösterge Paneli
- Teşhis kısayolları, hızlı araç erişimi, IP/ISP/NIC bilgileri
- **Güncelleme bildirimi** — yeni sürüm çıktığında otomatik haber verir

---

## 🚀 Kullanım

### Windows Kurulum Paketi (önerilen)
[Releases](https://github.com/Extreme159/NetVeil/releases) sayfasından yayınlanan sürümün **NetVeil_X.Y.Z_x64-setup.exe** kurulum paketini indir (X.Y.Z sürüm yer tutucusudur):

1. NSIS kurulum paketini indir ve çalıştır
2. Kurulu NetVeil uygulamasını aç; ICMP ping için **Yönetici olarak çalıştır**
3. Gösterge Panelinde bir teşhis seç: **Hızlı**, **Oyun**, veya **Gelişmiş**
4. Sonuçları harf notu + açıklamalarla gör, istersen PDF olarak kaydet

> Bu bir kurulum paketidir, portable uygulama değildir. NetVeil WebView2 kullanır.

### Kaynaktan Build

```powershell
git clone https://github.com/Extreme159/NetVeil.git
cd netveil
npm install
cd ui && npm install && cd ..
npx tauri dev
```

**Build ortamı** (CI Node 20 ve Rust stable kullanır; bunlar doğrulanmış minimum sürümler değildir):
- Node.js 20+ (CI: 20)
- Rust stable
- Windows 10/11 (Linux/macOS desteği planda)

### NSIS Kurulum Paketi Build

```powershell
npx tauri build --bundles nsis
# Çıktı: src-tauri/target/release/bundle/nsis/*-setup.exe
```

---

## 🧰 Teknolojiler

| Katman | Teknoloji |
|--------|-----------|
| Frontend | React 19, TypeScript 6.0, Vite 8 |
| Stil | Tailwind 4 + custom glass/dark CSS |
| Backend | Rust 2021, Tokio async |
| Native köprü | Tauri 2.11, WebView2 |
| ICMP ping | surge-ping (raw socket) |
| HTTP | reqwest 0.12 (HTTP/2, 16MB stream window) |
| Windows API | windows-rs (IpHelper, WLAN API) |
| Sunucu | Axum 0.8, Frankfurt VPS, 1 Gbps |

---

## 📂 Proje Yapısı

```
netveil/
├── src-tauri/src/
│   ├── lib.rs                  # Tauri komut kaydı + dlog! macro + tray
│   ├── smart.rs                # Tanılama skorlama motoru (lokal/offline, harf notu + içgörü)
│   ├── ping.rs / ping_test.rs  # ICMP + TCP fallback, saf ping aracı
│   ├── speedtest.rs            # Cloudflare + netveil, sliding p90 + peak
│   ├── bufferbloat.rs          # endüstri std., segment teşhisi, çift yönlü faz
│   ├── throttle*.rs            # ISP throttle: baseline/BitTorrent/QUIC/WireGuard/OpenVPN
│   ├── isp_intervention.rs     # DNS hijack, MITM, SNI, DoH/DoT, captive portal, DNSSEC, ECH
│   ├── dns_leak.rs / stun.rs   # DNS & IP sızıntı denetimi (+ ortak STUN)
│   ├── dns_benchmark.rs        # UDP/DoT/DoH sağlayıcı hız karşılaştırması
│   ├── cgnat.rs / mtu.rs       # CGNAT tespiti (UPnP+STUN) · yol MTU keşfi
│   ├── ipv6.rs                 # IPv6 bağlantı & dual-stack ping
│   ├── wifi.rs / wifi_scan.rs  # Wi-Fi bilgisi + kanal taraması (2.4/5/6 GHz)
│   ├── network_stability.rs    # Arka plan kopma/RTT monitörü + tray
│   ├── twitch_test.rs / twitch_rtmp.rs  # Twitch yayın testi (RTMP ingest)
│   ├── voip_test.rs            # VoIP kalitesi değerlendirmesi
│   ├── lan_scanner.rs          # Yerel ağ cihaz taraması
│   ├── lan_speed.rs            # Yerel ağ verim ölçümü
│   ├── db.rs                   # SQLite yerel veri (telemetri, kopma logu)
│   ├── nic.rs / network_topology.rs / traceroute.rs
│   └── ...
├── ui/src/
│   ├── App.tsx                 # Sekme orchestrator + global state (güncelleme, sonuçlar)
│   ├── components/tabs/        # Her araç için bir sekme bileşeni
│   ├── components/             # Sidebar, TopBar, ConnectionBar, SpeedGauge, GradeBox...
│   └── lib/                    # i18n TR/EN, icons, PDF rapor, helpers
├── assets/png/                 # Marka logosu (app icon, favicon, mark)
└── scripts/
    └── bump-version.mjs        # Versiyon dosyalarını senkron günceller
```

---

## 🛠️ Geliştirme

### Komutlar

```bash
npx tauri dev        # Tauri dev (Rust hot reload + Vite HMR)
npm run build        # UI production build
npx tauri build --bundles nsis  # Windows NSIS kurulum paketi
npm run bump         # Patch: 0.7.10 → 0.7.11 (beta patch 10'dan başlar)
npm run bump:minor   # Minor: 0.7.x → 0.8.10
npm run bump:major   # Script davranışı: 0.x.x → 1.0.10 (final 1.0.0 hedefi değil)
```

### Mimari Notlar

**Versiyon senkronizasyonu:**
`npm run bump` yedi yerel dosyayı günceller: `package.json`, `ui/package.json`, `package-lock.json`, `ui/package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` ve `README.md` rozeti. Lockfile'larda yalnız kök paket sürümleri değişir. UI'da versiyon **Ayarlar**'da gösterilir (kaynak: `ui/package.json`).

**`dlog!` macro:**
`lib.rs`'te tanımlı. Debug build'de `println!`, release'de no-op → CMD penceresi açılmaz.

**ISP Throttle rate limiting:**
`rate_kbps` değeri WireGuard paketinde `sender_index` alanına, QUIC'te `DCID`'nin ilk 4 byte'ına LE olarak gömülür. Sunucu token bucket ile buna göre sınırlar.

**HTTP/2 flow control:**
Default 65 KB window 50 ms RTT'de ~10 Mbps/stream'le sınırlar. `http2_initial_stream_window_size(16 MB)` + `http2_initial_connection_window_size(64 MB)` ile çözüldü.

**Bufferbloat segment teşhisi:**
WAN delta ile Gateway delta karşılaştırılır. Gateway temizse ISP/modem, ikisi de sorunluysa PC↔Router (Wi-Fi, NIC driver) suçludur.

---

## 🐛 Bilinen Sınırlamalar

- **ICMP ping** yönetici izni gerektirir. Yoksa bazı testler TCP fallback'e düşer (daha az hassas)
- **Linux/macOS** desteği henüz yok (ağ tespiti büyük oranda Windows API'ye bağımlı)
- **Hız/bufferbloat/throttle** testleri netveil.net sunucusuna bağlıdır; sunucu erişilemezse o testler çalışmaz
- **Tarayıcı WebRTC sızıntısı** ölçülmez (bu bir tarayıcı konusudur — DNS & IP sızıntı aracı sadece uygulamanın kendi trafiğini denetler)

---

## 🤝 Katkı

PR'lara açığım. Branch aç, feature ekle, conventional commits kullan:

```
feat: yeni özellik
fix: bug düzeltme
docs: dokümantasyon
refactor: davranış değişmedi, kod iyileşti
test: test ekle/güncelle
chore: build/CI/araç
```

---

## 📜 Lisans

BSL 1.1 — kaynak görüntüleyebilirsiniz, fork edebilirsiniz. Ticari kullanım için iletişime geçin.

---

## 🙏 Teşekkür

- **Cloudflare** — `speed.cloudflare.com` edge endpoint
- **Valve** — CS2 SDR relay altyapısı
- **Tauri** ekibi — Electron'dan 10× küçük native desktop deneyimi için

---

> ☕ ile yapıldı — [@Extreme159](https://github.com/Extreme159)

---

🌐 [English](README.md) · [Türkçe](README.tr.md)
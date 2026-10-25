# NetVeil

> **Advanced network diagnostics** — from a quick health check to deep, expert-grade analysis.

NetVeil measures ping, jitter, packet loss, bufferbloat, speed, ISP throttle/intervention, DNS leaks and more — all in **one application**. It doesn't just show numbers — it explains **"what this means for you"** in plain language.

**Why NetVeil?**
- 🎯 **One-click diagnosis** — runs all tests, provides letter grade + clear explanation
- 🔍 **Tests you won't find elsewhere** — detects whether your ISP throttles BitTorrent/QUIC/WireGuard/OpenVPN traffic at the protocol level
- 💬 **Understandable** — not "45ms jitter", but "you may experience occasional stutter in competitive games"
- 🪶 **Windows installer (NSIS)** — install NetVeil; the app uses WebView2
- 🔒 **Honest** — doesn't claim to measure what it can't, no fabricated scores

![Version](https://img.shields.io/badge/version-0.9.23-purple)
![Platform](https://img.shields.io/badge/platform-Windows-blue)
![License](https://img.shields.io/badge/license-BSL%201.1-orange)
![Tauri](https://img.shields.io/badge/Tauri-2.11-orange)

---

## ❓ Questions NetVeil Answers

If you've ever asked any of these, NetVeil is built to give you a real answer:

- **Is my ISP throttling me?** — NetVeil compares BitTorrent, QUIC, WireGuard and OpenVPN traffic against your baseline HTTPS speed at the protocol level.
- **Why is my gaming ping so high?** — Real ICMP ping to game servers + jitter, packet loss and bufferbloat, graded against competitive thresholds.
- **What is bufferbloat and how do I fix it?** — Idle vs. loaded latency measurement with culprit-segment diagnosis (is it your router or your ISP?).
- **Am I behind CGNAT?** — Detects carrier-grade NAT that breaks port forwarding.
- **Is my DNS leaking?** — Audits whether DNS/UDP traffic escapes your VPN tunnel.
- **Is something intercepting my connection?** — DNS hijacking, transparent proxy, SNI filtering, MITM certificates and more.

NetVeil runs on **Windows**, works **locally** (no account, no telemetry, no cloud scoring), and is **open-core / free** to use. Read more in our [guides](https://www.netveil.net/blog) or see how it [compares to other tools](https://www.netveil.net/compare).

---

## ✨ What It Does

### 🧠 Diagnosis Types

NetVeil offers three diagnosis modes tailored to your needs:

**Quick Diagnosis** (~2.5 min)
- General network health check with core metrics
- Ping, jitter, packet loss, speed, bufferbloat
- IPv6 connectivity, DNS benchmark, MTU discovery
- Perfect for daily quick checks

**Gaming Diagnosis** (~2.5 min)
- Competitive FPS score with real-time thresholds
- Tight grading standards for gamers
- Same core metrics as Quick, plus gaming-focused analysis

**Advanced Diagnosis** (~4.5 min)
- Full-scale expert analysis
- Includes ISP throttle detection, ISP intervention checks, CGNAT, VoIP quality
- PDF report export
- Complete picture for deep troubleshooting

All scoring is **local/offline** — no LLM, no API calls, no fabricated scores.

### 🏓 Ping & Latency
- True **ICMP ping** to game servers (CS2, Valorant, LoL, Apex, etc.) and popular services
- Pure Ping tool (IPv4/IPv6, any host you want)
- Jitter (stability), min/avg/max, packet loss, letter grade
- Target list fetched dynamically from server — no app update needed

### ⚡ Speed Test
- **Cloudflare** or our own **netveil.net** server (Frankfurt, 1 Gbps)
- 8 parallel streams, live animated gauge, p90 + peak Mbps

### 📊 Bufferbloat (Latency Under Load)
- Industry standard methodology: idle / download / upload / bidirectional phases
- True RTT measurement + **culprit segment diagnosis** (PC↔Router or ISP?)

### 🔒 ISP Throttle Detection ⭐ *(rare in competitors)*
Detects whether your ISP slows down specific traffic **at the protocol level**:
- **BitTorrent · QUIC · WireGuard · OpenVPN** — each compared against baseline HTTPS speed
- 30%+ deviation = throttle, 80%+ = blocking. Uses real protocol fingerprints (not fake)

### 🚨 ISP Intervention Detection ⭐
Checks whether something is intercepting your network:
- **DNS Hijacking** (5 domain majority voting) · **Transparent Proxy** · **SNI Filtering**
- **MITM Certificate** (system-injected CA detection) · **DoH/DoT access** · **Port access** · **VPN/Tor block**
- **Captive Portal** · **DNSSEC validation** · **ECH (Encrypted Client Hello) delivery**

### 🕵️ DNS & IP Leak Audit
- Whether your DNS queries leak from your real connection (critical if you use VPN)
- Check UDP traffic exit IP (split-tunnel detection)
- *Note: We explicitly state we don't measure browser WebRTC leaks — honesty*

### 🌐 Other Tools
- **IPv6 Diagnosis** — connection type (Native/Tunnel), dual-stack RTT, ISP IPv6 quality
- **Wi-Fi Channel Analysis** — 2.4/5/6 GHz scan, channel density map, best channel recommendation
- **DNS Benchmark** — provider speed comparison (Cloudflare, Google, Quad9...) via UDP/DoT/DoH
- **MTU/MSS Discovery** — path MTU (PPPoE 1492, etc.)
- **CGNAT Detection** — whether your ISP placed you behind CGNAT (affects port forwarding)
- **Traceroute** — hop-by-hop route trace, waterfall view
- **Network Stability Monitor** — background drop/RTT tracking, tray notification
- **Twitch Stream Test** — for broadcasters: RTMP ingest bandwidth + RTT (doesn't go live)
- **VoIP Quality** — VoIP readiness assessment
- **LAN Scanner** — scan local network for devices
- **LAN Speed** — local network throughput measurement

### 📈 Dashboard
- Quick diagnosis shortcuts, fast tool access, IP/ISP/NIC info
- **Update notification** — auto-notify when new version is available

---

## 🚀 Usage

### Windows Installer (recommended)
Download the **NetVeil_X.Y.Z_x64-setup.exe** installer for the published version from [Releases](https://github.com/Extreme159/NetVeil/releases) (X.Y.Z is the version placeholder):

1. Download and run the NSIS installer
2. Launch the installed NetVeil app; **Run as administrator** for ICMP ping
3. In the Dashboard, pick a diagnosis: **Quick**, **Gaming**, or **Advanced**
4. View results with letter grades and explanations, optionally save as PDF

> This is an installer, not a portable executable. NetVeil uses WebView2.

### Build from Source

```powershell
git clone https://github.com/Extreme159/NetVeil.git
cd netveil
npm install
cd ui && npm install && cd ..
npx tauri dev
```

**Build environment** (CI uses Node 20 and Rust stable; these are not verified minimum versions):
- Node.js 20+ (CI: 20)
- Rust stable
- Windows 10/11 (Linux/macOS support planned)

### NSIS Installer Build

```powershell
npx tauri build --bundles nsis
# Output: src-tauri/target/release/bundle/nsis/*-setup.exe
```

---

## 🧰 Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | React 19, TypeScript 6.0, Vite 8 |
| Styling | Tailwind 4 + custom glass/dark CSS |
| Backend | Rust 2021, Tokio async |
| Native bridge | Tauri 2.11, WebView2 |
| ICMP ping | surge-ping (raw socket) |
| HTTP | reqwest 0.12 (HTTP/2, 16MB stream window) |
| Windows API | windows-rs (IpHelper, WLAN API) |
| Server | Axum 0.8, Frankfurt VPS, 1 Gbps |

---

## 📂 Project Structure

```
netveil/
├── src-tauri/src/
│   ├── lib.rs                  # Tauri command registration + dlog! macro + tray
│   ├── smart.rs                # Diagnosis scoring engine (local/offline, letter grade + insight)
│   ├── ping.rs / ping_test.rs  # ICMP + TCP fallback, pure ping tool
│   ├── speedtest.rs            # Cloudflare + netveil, sliding p90 + peak
│   ├── bufferbloat.rs          # industry std, segment diagnosis, bidirectional phases
│   ├── throttle*.rs            # ISP throttle: baseline/BitTorrent/QUIC/WireGuard/OpenVPN
│   ├── isp_intervention.rs     # DNS hijack, MITM, SNI, DoH/DoT, captive portal, DNSSEC, ECH
│   ├── dns_leak.rs / stun.rs   # DNS & IP leak audit (+ shared STUN)
│   ├── dns_benchmark.rs        # UDP/DoT/DoH provider speed comparison
│   ├── cgnat.rs / mtu.rs       # CGNAT detection (UPnP+STUN) · path MTU discovery
│   ├── ipv6.rs                 # IPv6 connectivity & dual-stack ping
│   ├── wifi.rs / wifi_scan.rs  # Wi-Fi info + channel scan (2.4/5/6 GHz)
│   ├── network_stability.rs    # Background drop/RTT monitor + tray
│   ├── twitch_test.rs / twitch_rtmp.rs  # Twitch stream test (RTMP ingest)
│   ├── voip_test.rs            # VoIP quality assessment
│   ├── lan_scanner.rs          # Local network device scan
│   ├── lan_speed.rs            # Local network throughput
│   ├── db.rs                   # SQLite local data (telemetry, drop log)
│   ├── nic.rs / network_topology.rs / traceroute.rs
│   └── ...
├── ui/src/
│   ├── App.tsx                 # Tab orchestrator + global state (update, results)
│   ├── components/tabs/        # One tab component per tool
│   ├── components/             # Sidebar, TopBar, ConnectionBar, SpeedGauge, GradeBox...
│   └── lib/                    # i18n TR/EN, icons, PDF report, helpers
├── assets/png/                 # Brand logo (app icon, favicon, mark)
└── scripts/
    └── bump-version.mjs        # Syncs version files
```

---

## 🛠️ Development

### Commands

```bash
npx tauri dev        # Tauri dev (Rust hot reload + Vite HMR)
npm run build        # UI production build
npx tauri build --bundles nsis  # Windows NSIS installer
npm run bump         # Patch: 0.7.10 → 0.7.11 (beta patch starts at 10)
npm run bump:minor   # Minor: 0.7.x → 0.8.10
npm run bump:major   # Script behavior: 0.x.x → 1.0.10 (not the final 1.0.0 target)
```

### Architecture Notes

**Version synchronization:**
`npm run bump` updates seven local files: `package.json`, `ui/package.json`, `package-lock.json`, `ui/package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and the `README.md` badge. Only root package versions change in the lockfiles. The UI version is shown in **Settings** (source: `ui/package.json`).

**`dlog!` macro:**
Defined in `lib.rs`. `println!` in debug build, no-op in release → no CMD window.

**ISP Throttle rate limiting:**
`rate_kbps` value embedded in WireGuard packet's `sender_index` field, or in first 4 bytes of QUIC's `DCID` as LE. Server limits accordingly with token bucket.

**HTTP/2 flow control:**
Default 65 KB window limits to ~10 Mbps/stream at 50 ms RTT. Solved with `http2_initial_stream_window_size(16 MB)` + `http2_initial_connection_window_size(64 MB)`.

**Bufferbloat segment diagnosis:**
WAN delta compared against Gateway delta. If Gateway is clean, ISP/modem is at fault; if both are problematic, PC↔Router (Wi-Fi, NIC driver) is the culprit.

---

## 🐛 Known Limitations

- **ICMP ping** requires administrator permission. Without it, some tests fall back to TCP (less sensitive)
- **Linux/macOS** support not yet available (network detection heavily depends on Windows APIs)
- **Speed/bufferbloat/throttle** tests depend on netveil.net server; if server is unreachable, those tests won't run
- **Browser WebRTC leaks** are not measured (this is a browser concern — DNS & IP leak tool only audits the app's own traffic)

---

## 🤝 Contributing

PRs welcome. Open a branch, add a feature, use conventional commits:

```
feat: new feature
fix: bug fix
docs: documentation
refactor: behavior unchanged, code improved
test: add/update test
chore: build/CI/tools
```

---

## 🧩 Open-Core Model

NetVeil follows an **open-core** model:

- **This repository** contains the full **free core** of NetVeil — Quick & Gaming diagnosis, ping/latency, speed test, bufferbloat, Wi-Fi scan, DNS benchmark, IPv6, MTU, traceroute and more. It builds and runs as a complete free application.
- A set of **Pro tools** (ISP throttle & intervention detection, network stability monitor, CGNAT, DNS leak, VoIP quality, LAN scanner, LAN speed, Twitch RTMP) are part of the commercial edition. Their source is **not** included here; the corresponding modules appear only as placeholder stubs so the open core compiles cleanly.

The official build is a single installer that contains everything and unlocks the Pro tools with a license key — there are **no separate "free" and "pro" downloads**. Building this repository yields the free edition. The netveil.net API server is not part of this open-core repository.

Learn more or get Pro at **[netveil.net](https://netveil.net)**.

---

## 📜 License

BSL 1.1 — you can view source, fork it (non-commercial). For commercial use, get in touch.

---

## 🙏 Acknowledgments

- **Cloudflare** — `speed.cloudflare.com` edge endpoint
- **Valve** — CS2 SDR relay infrastructure
- **Tauri** team — 10× smaller native desktop experience than Electron

---

> Made with ☕ by [@Extreme159](https://github.com/Extreme159)

---

🌐 [English](README.md) · [Türkçe](README.tr.md)
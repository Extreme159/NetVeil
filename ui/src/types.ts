export interface NicInfo {
  name: string;
  mac: string;
  ip_addresses: string[];
  adapter_type: string;
  is_active: boolean;
  received_bytes: number;
  transmitted_bytes: number;
  link_speed: string | null;
}

export interface WifiInfo {
  ssid: string;
  bssid: string;
  signal_percent: number;
  signal_dbm: number;
  quality: string;
  channel: number;
  band: string;
  phy_type: string;
  tx_rate: string;
  rx_rate: string;
  auth: string;
  cipher: string;
  connected: boolean;
}

export type PingCategory = "services" | "games";

export interface PingTarget {
  category: PingCategory;
  name: string;
  region: string;
  host: string;
  display: string;
  /** TCP-only hedefler için port (GFN edge). undefined → normal ICMP. */
  tcp_port?: number;
  /** Services kategorisinde UI grup başlığı (i18n anahtarı). games'te yok. */
  group?: string | null;
}

export interface ServerPingResult {
  /** UI alt-tab filtresi (Services / Game Servers). */
  category: PingCategory;
  /** Servisin/oyunun adı (CS2, Cloudflare, Google, ...). */
  name: string;
  region: string;
  host: string;
  display: string;
  ip: string;
  avg_ms: number;
  min_ms: number;
  max_ms: number;
  jitter_ms: number;
  packet_loss: number;
  grade: string;
  reachable: boolean;
}

export interface LatencySample {
  time_ms: number;
  latency_ms: number;
  phase: string;
}

export interface SegmentStats {
  target: "gateway" | "wan" | string;
  target_ip: string;
  idle_p50_ms: number;
  idle_p95_ms: number;
  loaded_p95_ms: number;
  delta_ms: number;
  grade: string;
  bidi_p95_ms: number;
  bidi_delta_ms: number;
}

export interface BufferbloatResult {
  idle_latency_ms: number;
  loaded_latency_ms: number;
  latency_delta_ms: number;
  grade: string;
  grade_description: string;
  download_mbps: number;
  upload_mbps: number;
  samples: LatencySample[];
  segments: SegmentStats[];
  diagnosis: string;
}

export interface SpeedTestResult {
  download_mbps: number;
  upload_mbps: number;
  /** null = ping could not be measured (NOT 0 ms). */
  ping_ms: number | null;
  jitter_ms: number | null;
  server: string;
  download_peak_mbps: number;
  upload_peak_mbps: number;
}

export interface GamingScore {
  // null = olculemedi (ping/bufferbloat verisi yok)
  latency: number | null;
  jitter: number | null;
  bufferbloat: number | null;
  packet_loss: number | null;
  /** null = hicbir bilesen olculemedi (grade "-") */
  total: number | null;
  grade: string;
}

export interface GeneralScore {
  // latency/jitter/bufferbloat/packet_loss: null = olculemedi (ping/bufferbloat verisi yok)
  latency: number | null;
  jitter: number | null;
  bufferbloat: number | null;
  packet_loss: number | null;
  speed: number | null;
  wifi: number;
  throttle: number | null;
  isp_intervention: number | null;
  ipv6: number | null;
  cgnat: number | null;
  mtu: number | null;
  voip: number | null;
  dns_benchmark: number | null;
  twitch: number | null;
  /** null = yetersiz veri: ag-kalitesi olcumu yok (grade "-"); Wi-Fi tek basina not uretmez */
  total: number | null;
  grade: string;
}

export interface Insight {
  kind: "positive" | "neutral" | "negative";
  text_key: string;     // i18n anahtari
}

export interface ModuleVerdict {
  module: string;       // "latency" | "jitter" | "loss" | "bufferbloat" | "wifi" | "speed" | "throttle" | "isp_intervention" | "ipv6"
  score: number;        // 0-100
  grade: string;        // A+/A/B/C/D/F veya "-" (olculmedi)
  metric: number;       // olculen ham deger
  metric_unit: string;  // "ms" | "%" | "Mbps" | ""
  detail_key: string;   // i18n anahtari
  available: boolean;
  insights: Insight[];  // zengin yorum (pozitif/notr/negatif)
}

export interface Synthesis {
  /** null = yetersiz veri (grade "-", headline_key "synth_insufficient_data") */
  score: number | null;
  grade: string;
  headline_key: string;
  insights: Insight[];
}

export interface SmartDiagnosis {
  summary: string;
  issues: string[];
  suggestions: string[];
  /** Sadece tam test (speed+throttle+isp+ipv6) yapıldıysa dolu gelir, yoksa null */
  general_score: GeneralScore | null;
  source: string;
  /** Modul-modul yapilandirilmis yorumlar (Smart Diagnosis Engine). */
  modules: ModuleVerdict[];
  /** Toptan durum: excellent|good|fair|poor|critical */
  overall_status: string;
  /** Buyuk resim sentezi — genel ag kalitesi (yalnizca tam testte). */
  general_synthesis: Synthesis | null;
}

export interface FpsScore {
  // null = olculemedi (ping/bufferbloat verisi yok)
  latency: number | null;
  jitter: number | null;
  packet_loss: number | null;
  bufferbloat: number | null;
  wifi: number;
  /** null = yetersiz veri: ag-kalitesi olcumu yok (grade "-") */
  total: number | null;
  grade: string;
}

export interface FpsDiagnosis {
  fps_score: FpsScore;
  fps_synthesis: Synthesis;
  modules: ModuleVerdict[];
}

export interface TopologyInfo {
  local_ip: string;
  gateway_ip: string;
  wan_ip: string;
  isp: string;
  country: string;
  city: string;
}

/** "Blocked (RST)"/"Blocked (DROP)" = blocked + mechanism; "Error" = not measured. */
export type ThrottleVerdict =
  "Normal" | "Throttled" | "Blocked" | "Blocked (RST)" | "Blocked (DROP)" | "Error" | "Pending";

export interface ThrottleTestResult {
  baseline_mbps: number;
  baseline_upload_mbps: number;
  baseline_peak_mbps: number;
  bittorrent_mbps: number;
  quic_mbps: number;
  wireguard_mbps: number;
  openvpn_mbps: number;
  bittorrent_throttle: number;
  quic_throttle: number;
  wireguard_throttle: number;
  openvpn_throttle: number;
  bittorrent_verdict: ThrottleVerdict;
  quic_verdict: ThrottleVerdict;
  wireguard_verdict: ThrottleVerdict;
  openvpn_verdict: ThrottleVerdict;
  isp_throttles: boolean;
  /** true = measurement incomplete (no baseline / a protocol errored) and no throttling found; NOT a healthy result. */
  inconclusive?: boolean;
}

// ─── IPv6

export interface Ipv6AddressInfo {
  address: string;
  prefix_len: number;
  scope: string;
  address_type: string; // "Native" | "Teredo" | "6to4" | "ISATAP" | "LinkLocal"
  is_temporary: boolean;
}

export interface Ipv6ConnectivityInfo {
  has_ipv6: boolean;
  has_native_ipv6: boolean;
  has_tunnel_only: boolean;
  addresses: Ipv6AddressInfo[];
  gateway_v6: string | null;
  wan_ipv6: string | null;
  isp_ipv6: string | null;
  dns_ipv6_works: boolean;
  /** En az bir IPv6 TCP tasima probu basarili (Full icin gerekli). Eski sonuclarda yok. */
  ipv6_transport_works?: boolean;
  connectivity_grade: "Full" | "Partial" | "Tunnel" | "None";
}

export interface Ipv6QualityScore {
  score: number;
  grade: string;
  has_connectivity: boolean;
  has_native: boolean;
  dns_works: boolean;
  isp_assessment: string;
  recommendations: string[];
}

export interface Ipv6TestResult {
  connectivity: Ipv6ConnectivityInfo;
  quality: Ipv6QualityScore;
}

export interface Ipv6Progress {
  phase: "connectivity" | "scoring";
  status: "running" | "done" | "error";
  message: string;
}

// ─── ISP Intervention ────────────────────────────────────────────────────────

export interface DnsHijackResult {
  status: string;
  verdict: "Normal" | "Hijacked" | "Inconclusive";
  details: string;
  resolved_ips: string[];
  direct_ips: string[];
}

export interface TransparentProxyResult {
  status: string;
  verdict: "Normal" | "Detected" | "Inconclusive";
  details: string;
  injected_headers: string[];
}

export interface SniFilteringResult {
  status: string;
  verdict: "Normal" | "Filtered" | "Inconclusive";
  details: string;
  blocked_domains: string[];
  tested_domains: string[];
}

export interface MitmResult {
  status: string;
  verdict: "Normal" | "Detected" | "Inconclusive";
  details: string;
  cert_issuer: string;
  cert_valid: boolean;
  fingerprint_mismatch: boolean;
}

export interface DohProviderDetail {
  name: string;
  kind: "doh" | "dot";
  reachable: boolean;
}

export interface DohDotResult {
  status: string;
  verdict: "Normal" | "Partial" | "Blocked" | "Inconclusive";
  details: string;
  doh_works: boolean;
  dot_works: boolean;
  tested_providers: string[];
  blocked_providers: string[];
  providers: DohProviderDetail[];
}

export interface PortEntry {
  port: number;
  description: string;
  /** TCP baglantisi kabul edildi (sunucuda dinleyen var). */
  open: boolean;
  latency_ms: number | null;
  /** Yol filtrelenmiyor: kabul VEYA RST. Eski sonuclarda yok. */
  reachable?: boolean;
  /** "open" | "closed" (RST: port kapali, yol acik) | "filtered" (yanit yok). Eski sonuclarda yok. */
  state?: "open" | "closed" | "filtered";
}

export interface PortTestResult {
  status: string;
  verdict: "Normal" | "Blocked" | "Inconclusive";
  details: string;
  port_results: PortEntry[];
  blocked_count: number;
}

export interface AsEntry {
  name: string;
  host: string;
  reachable: boolean;
  note: string;
}

export interface AsBlockingResult {
  status: string;
  verdict: "Normal" | "Blocked" | "Inconclusive";
  details: string;
  tested_services: AsEntry[];
  blocked_count: number;
}

export interface CaptivePortalResult {
  status: string;
  verdict: "Normal" | "Detected" | "Inconclusive";
  details: string;
  redirect_url: string;
}

export interface DnssecResult {
  status: string;
  /** Bilgi amacli, banner tetiklemez: "Validating" | "Info" | "Inconclusive" */
  verdict: "Validating" | "Info" | "Inconclusive";
  details: string;
  validates: boolean;
  ad_bit: boolean;
  resolver: string;
}

export interface EchResult {
  status: string;
  /** Bilgi amacli, banner tetiklemez: "Normal" | "Info" | "Inconclusive" */
  verdict: "Normal" | "Info" | "Inconclusive";
  details: string;
  system_has_ech: boolean;
  public_has_ech: boolean;
  domain: string;
}

export interface IspInterventionResult {
  dns_hijack: DnsHijackResult;
  transparent_proxy: TransparentProxyResult;
  sni_filtering: SniFilteringResult;
  mitm: MitmResult;
  doh_dot: DohDotResult;
  port_test: PortTestResult;
  as_blocking: AsBlockingResult;
  captive_portal: CaptivePortalResult;
  dnssec: DnssecResult;
  ech: EchResult;
  any_intervention_detected: boolean;
  /** En az bir alt-test Inconclusive (olculemedi). Eski sonuclarda yok. */
  any_inconclusive?: boolean;
}

export interface UpdateInfo {
  current_version: string;
  latest_version: string;
  has_update: boolean;
  download_url: string;
  release_notes: string;
}

/** LemonSqueezy lisans durumu (Rust license.rs get_license_status/validate_license döner). */
export interface LicenseStatus {
  valid: boolean;
  license_key: string | null;
  /** "none" | "monthly" | "lifetime" */
  license_type: string;
  /** "active" | "inactive" | "expired" | "disabled" | "none" | "grace" */
  status: string;
  activated_at: string | null;
  expires_at: string | null;
  error: string | null;
}

export interface IspInterventionProgress {
  phase:
    | "dns_hijack"
    | "transparent_proxy"
    | "sni_filtering"
    | "mitm"
    | "doh_dot"
    | "port_test"
    | "as_blocking"
    | "captive_portal"
    | "dnssec"
    | "ech";
  status: "running" | "done" | "error";
  message: string;
}

// ─── DNS Benchmark ──────────────────────────────────────────────────────────

export interface DnsProbeResult {
  min_ms:      number;
  avg_ms:      number;
  max_ms:      number;
  median_ms:   number;
  jitter_ms:   number;
  packet_loss: number;
  grade:       string;
  reachable:   boolean;
}

export interface DnsProviderResult {
  name:       string;
  ip:         string;
  udp:        DnsProbeResult;
  dot:        DnsProbeResult;
  doh:        DnsProbeResult;
  best_ms:    number;
  best_proto: string;
  grade:      string;
  rank:       number;
}

export interface DnsBenchmarkResult {
  providers:   DnsProviderResult[];
  recommended: string;
  summary:     string;
}

export interface DnsBenchmarkProgress {
  provider:  string;
  protocol:  string;   // "udp" | "dot" | "doh"
  status:    string;   // "running" | "done"
  median_ms: number;
}

// ─── Wi-Fi Scan ─────────────────────────────────────────────────────────────

export interface AccessPoint {
  ssid: string;
  bssid: string;
  rssi: number;
  signal_quality: number;
  channel: number;
  center_freq_mhz: number;
  band: string;
  channel_width: string;
  phy_type: string;
  is_connected: boolean;
  /** Bağlı AP ile aynı fiziksel cihaz / aynı SSID — Rust is_own_ap ile hesaplanır. */
  is_own: boolean;
}

export interface ChannelRecommendation {
  band: string;
  current_channel: number;
  recommended_channel: number;
  reason: string;
  congestion_score: number;
}

export interface WifiScanResult {
  access_points: AccessPoint[];
  current_ssid: string;
  current_bssid: string;
  current_channel: number;
  current_band: string;
  recommendations: ChannelRecommendation[];
  interface_found: boolean;
}

// ── Ağ Kararlılık Monitörü ──────────────────────────────────────────────────

export interface StabilitySample {
  ts: number;          // unix ms
  gateway_rtt: number | null;
  wan_rtt: number | null;
  gateway_ok: boolean;
  wan_ok: boolean;
}

export interface OutageEvent {
  ts: number;
  kind: "gateway" | "wan" | "full";
  duration_ms: number | null;
}

export interface StabilityState {
  running: boolean;
  gateway: string;
}

// ─── Twitch Streaming Test (Premium) ─────────────────────────────────────────

export interface TwitchServerResult {
  id: number;
  name: string;
  host: string;
  /** TCP connect (RTMP 1935) süresi (ms). null = ulaşılamadı. */
  rtt_ms: number | null;
  /** Ulaşılan upload bandwidth (Mbps). null = ölçülmedi/başarısız. */
  bandwidth_mbps: number | null;
  /** Auto/manuel seçimle bandwidth testine dahil edildi mi. */
  selected: boolean;
  /** Bandwidth ölçülemediyse neden kodu: "rtmps_unsupported" | "tls" | "failed". */
  bandwidth_error?: string | null;
}

export interface TwitchTestResult {
  servers: TwitchServerResult[];
  best_server: string | null;
}

// ─── CGNAT Tespiti (Premium) ──────────────────────────────────────────

// ─── MTU / MSS Keşfi (Lite) ─────────────────────────────────────────

export interface MtuTarget {
  label: string;
  host: string;
  /** Keşfedilen Path MTU (byte). null = ICMP bloklu/ulaşılamadı. */
  mtu: number | null;
  /** MSS = MTU - 40. null = MTU yok. */
  mss: number | null;
}

export interface MtuResult {
  targets: MtuTarget[];
  primary_mtu: number | null;
  detail_key: string;
}

export interface CgnatResult {
  /** Router'ın WAN arayüz IP'si (UPnP IGD). null = UPnP yok/erişilemedi. */
  router_wan_ip: string | null;
  /** STUN reflexive public IP. */
  stun_public_ip: string | null;
  /** Web public IP (ipinfo/ipify). */
  web_public_ip: string | null;
  /** Cihazın yerel IP'si (bilgi amaçlı; karara KATILMAZ). */
  local_ip: string | null;
  /** "cgnat" | "double_nat" | "public" | "unknown" */
  verdict: string;
  upnp_available: boolean;
  /** i18n açıklama anahtarı. */
  detail_key: string;
}

export interface DnsLeakResult {
  /** Sistemin yapılandırılmış DNS sunucu IP'leri (bilgi amaçlı). */
  system_dns_servers: string[];
  /** Resolver'ın internete çıkış (egress) IP'si — whoami.akamai.net A kaydı. */
  resolver_egress_ip: string | null;
  /** İnternetin gördüğü WAN public IP (ipify). */
  wan_public_ip: string | null;
  /** STUN reflexive (UDP çıkış) public IP. */
  stun_public_ip: string | null;
  /** Aktif VPN arayüzü tespit edildi mi. */
  vpn_active: boolean;
  /** "info" (egress olculdu, kullanici yorumlar) | "unknown"; eski sonuclarda "safe" | "normal" | "leak" */
  verdict: string;
  /** Karar açıklaması i18n anahtarı. */
  detail_key: string;
  /** UDP/STUN ek bulgusu: "match" | "mismatch" | "unknown" */
  udp_finding: string;
  /** UDP bulgusu i18n anahtarı. */
  udp_detail_key: string;
}

// ─── VoIP / Gaming UDP Kalite Testi (Premium) ────────────────────────────────

export interface VoipModeResult {
  rtt_avg_ms: number;
  rtt_min_ms: number;
  rtt_max_ms: number;
  jitter_ms: number;
  loss_pct: number;
  packets_sent: number;
  packets_recv: number;
  mos: number | null;
  mos_grade: string | null;
  grade: string;
  tier: string | null;
}

export interface VoipQualityResult {
  voice: VoipModeResult;
  error: string | null;
}

export interface VoipQualityProgress {
  mode: string;
  status: string;
  message: string;
  packets_sent: number;
  packets_recv: number;
}

// ─── LAN IP Scanner ────────────────────────────────────────────────────────

export interface LanDevice {
  ip: string;
  mac: string | null;
  hostname: string | null;
  vendor: string | null;
  rtt_ms: number | null;
  open_ports: number[];
  is_gateway: boolean;
  is_self: boolean;
}

export interface LanScanResult {
  devices: LanDevice[];
  subnet: string;
  scan_duration_ms: number;
  total_probed: number;
}

export interface LanPeer {
  ip: string;
  hostname: string;
  port: number;
}

// ─── LAN Hız Testi ────────────────────────────────────────────────────────

export interface LanSpeedProgress {
  phase: "handshake" | "upload" | "download" | "done" | "error";
  direction: "server" | "client";
  mbps: number;
  peak_mbps: number;
  elapsed_ms: number;
  status: "running" | "done" | "error";
  message: string;
}

export interface LanSpeedResult {
  upload_mbps: number;
  upload_peak_mbps: number;
  download_mbps: number;
  download_peak_mbps: number;
  streams: number;
  duration_secs: number;
  target_ip: string;
  error: string | null;
  peer_hostname: string | null;
}

export interface TestRunRow {
  id: number;
  ts: number;
  day: string;
  module: string;
  metrics: string;          // JSON string — UI parse eder
  summary_score: number | null;
  summary_grade: string | null;
  created_at: number;
}

export type TestPhase =
  | "idle"
  | "running_nic"
  | "running_wifi"
  | "running_ping"
  | "running_speedtest"
  | "running_bufferbloat"
  | "running_throttle"
  | "running_ipv6"
  | "running_cgnat"
  | "running_mtu"
  | "running_voip"
  | "running_isp_intervention"
  | "running_dns_benchmark"
  | "running_smart"
  | "done"
  | "error";

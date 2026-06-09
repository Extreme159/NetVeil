import type { SVGProps } from "react";
import {
  LayoutDashboard,
  Network,
  Gauge,
  Activity,
  Server,
  Gamepad2,
  Waves,
  Route,
  Radar,
  Globe,
  TrendingDown,
  ShieldAlert,
  DatabaseZap,
  RadioTower,
  HeartPulse,
  Sparkles,
  Settings as LSettings,
  Play as LPlay,
  RefreshCw,
  ArrowDown,
  ArrowUp,
  Bell as LBell,
  Bot,
  Droplet,
  Zap,
  Eye as LEye,
  EyeOff as LEyeOff,
  Shield as LShield,
  Cloud as LCloud,
  AlertTriangle,
  Lightbulb,
  Link as LLink,
  Tv,
  Lock,
  Plug,
  Key,
  DoorOpen,
  UserSearch,
  Cable,
  Monitor,
  Bluetooth,
  Radio,
  Circle,
  Trophy,
  BarChart3,
  Search,
  Rocket,
  MessageCircle,
  Music,
  Clapperboard,
  ShoppingCart,
  AppWindow,
  Crosshair,
  Apple as LApple,
  Users,
  ChevronRight,
  Wifi as LWifi,
  ShieldOff,
  Waypoints,
  Headphones,
  History as LHistory,
} from "lucide-react";

type IconProps = SVGProps<SVGSVGElement>;

// Marka logosu — lucide'de birebir karsiligi yok, custom kalir (ileride degisir).
const base: IconProps = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 2,
  strokeLinecap: "round",
  strokeLinejoin: "round",
};

/**
 * Ikon sozlugu — lucide-react tabanli (Logo haric).
 * API korundu: tum cagri yerleri `Icon.X` kullanmaya devam eder.
 * Sidebar her tool icin tekrarsiz, anlamli ikon kullanir.
 */
export const Icon = {
  // ── Sidebar / Tanilama ──────────────────────────────────────────────
  Dashboard:  LayoutDashboard,   // Dashboard
  NetworkIco: Network,           // Ag / Wi-Fi
  Speed:      Gauge,             // Hiz Testi
  Latency:    Activity,          // Gecikme (grup)
  Server:     Server,            // Services
  Game:       Gamepad2,          // Game Servers
  Buffer:     Waves,             // Bufferbloat
  Route:      Route,             // Rota Izleme
  PingTool:   Radar,             // Saf Ping araci
  Globe:      Globe,             // IPv6

  // ── Sidebar / Premium ───────────────────────────────────────────────
  Throttle:   TrendingDown,      // ISP Daraltma
  Intervention: ShieldAlert,     // ISP Mudahale
  Dns:        DatabaseZap,       // DNS Benchmark
  WifiChannel: LWifi,            // Wi-Fi Kanal Analizi
  Stability:  HeartPulse,        // Ag Kararlilik
  TwitchTest: Radio,             // Twitch Streaming Test (yayin/ingest)
  Cgnat:      Waypoints,         // CGNAT Tespiti (NAT katmanlari / dugumler)
  Mtu:        Cable,             // MTU/MSS Kesfi (paket boyutu)
  DnsLeak:    ShieldOff,          // DNS & IP Sizinti Denetimi (kalkan delik = sizinti)
  VoipQuality: Headphones,       // VoIP/Gaming Kalite Testi (ses + oyun UDP)
  Brain:      Sparkles,          // AI

  // ── Genel UI ─────────────────────────────────────────────────────────
  Wifi:       RadioTower,        // (eski Wifi kullanimlari — sinyal)
  Ping:       Activity,          // (DiagnosticProgress adim ikonu)
  Settings:   LSettings,
  Play:       LPlay,
  Refresh:    RefreshCw,
  Down:       ArrowDown,
  Up:         ArrowUp,
  Bell:       LBell,
  Robot:      Bot,
  Drop:       Droplet,
  Wave:       Waves,
  Bolt:       Zap,
  Eye:        LEye,
  EyeOff:     LEyeOff,
  Shield:     LShield,
  Cloud:      LCloud,           // Cloudflare kaynak secici
  Alert:      AlertTriangle,    // Sorunlar/uyari
  Idea:       Lightbulb,        // Oneriler

  // ── Test turu / kategori (sabit, tek renk) ──────────────────────────
  Link:       LLink,            // BitTorrent / P2P
  Tv:         Tv,               // QUIC / Streaming
  Lock:       Lock,             // WireGuard / VPN / SNI / DoT
  Plug:       Plug,             // Transparent proxy / Ethernet
  Key:        Key,              // DoH/DoT anahtar
  Door:       DoorOpen,         // Port test
  Spy:        UserSearch,       // AS blocking / izleme
  Cable:      Cable,            // Ethernet
  Monitor:    Monitor,          // Virtual adapter
  Bluetooth:  Bluetooth,        // Bluetooth adapter
  Radio:      Radio,            // Antenna / sinyal / bos durum
  Circle:     Circle,           // UDP / nokta
  Trophy:     Trophy,           // Kazanan
  Chart:      BarChart3,        // Skor / istatistik
  History:    LHistory,          // Test gecmisi
  Search:     Search,           // Tarama / arama
  Rocket:     Rocket,           // Gaming / hizli
  Crosshair:  Crosshair,        // FPS oyun (CS2)

  // ── Servis logolari (monochrome) ────────────────────────────────────
  Chat:       MessageCircle,    // Discord / mesajlasma
  Music:      Music,            // Spotify
  Movie:      Clapperboard,     // Netflix
  Cart:       ShoppingCart,     // Amazon
  Window:     AppWindow,        // Microsoft
  Apple:      LApple,           // Apple
  Social:     Users,            // Meta
  ChevronRight: ChevronRight,   // Next / ileri (TestIntro)

  // ── Marka (custom) ───────────────────────────────────────────────────
  Logo: (p: IconProps) => (
    <svg viewBox="0 0 24 24" {...base} strokeWidth={2.2} {...p}>
      <path d="M4 14l4-4 3 3 5-7 4 6" />
      <circle cx="8" cy="10" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="16" cy="6" r="1.5" fill="currentColor" stroke="none" />
    </svg>
  ),
} as const;

export type IconKey = keyof typeof Icon;

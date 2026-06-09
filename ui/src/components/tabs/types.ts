import type { TestResults, TabId } from "../../App";
import type { TestPhase, PingTarget, TopologyInfo, BufferbloatResult, UpdateInfo, LicenseStatus } from "../../types";

export interface TabProps {
  results: TestResults;
  phase: TestPhase;
  testMode: "quick" | "full";
  testStartTime: number | null;
  totalElapsedMs: number;
  error: string | null;
  /** Ana Sayfa kisayollari icin sekme degistirme (App.setActiveTab). */
  setActiveTab: (t: TabId) => void;
  runAllTests: () => Promise<void>;
  runQuickScan: () => Promise<void>;
  runSpeedTest: (server?: "cloudflare" | "netveil") => Promise<void>;
  runPingServices: () => Promise<void>;
  runPingGames: () => Promise<void>;
  runBufferbloat: (serverChoice?: "cloudflare" | "netveil") => Promise<void>;
  runThrottleTest: () => Promise<void>;
  runIspInterventionTest: () => Promise<void>;
  runIpv6Test: () => Promise<void>;
  /** Speed test sırasında backend'den gelen anlık download Mbps. */
  liveDl: number;
  /** Speed test sırasında backend'den gelen anlık upload Mbps. */
  liveUl: number;
  /** Anlık olarak hangi faz ölçülüyor (event'ten gelir). */
  liveSpeedPhase: "download" | "upload" | null;
  speedServerChoice: "cloudflare" | "netveil";
  setSpeedServerChoice: (s: "cloudflare" | "netveil") => void;
  bufferbloatServerChoice: "cloudflare" | "netveil";
  setBufferbloatServerChoice: (s: "cloudflare" | "netveil") => void;
  bufferbloatCache: Record<string, BufferbloatResult>;
  speedResultCache: Record<string, { dl: number; ul: number; ping: number; server: string }>;
  pingTargets: PingTarget[];
  topology: TopologyInfo | null;
  topoLoading: boolean;
  gatewayPing: number | null;
  cfPing: number | null;
  /** Guncelleme bilgisi (App tek kaynak; has_update=true ise dolu, yoksa null). */
  updateInfo: UpdateInfo | null;
  /** Lisans gecerli mi (Premium kilit icin). Yoksa false. */
  isLicensed?: boolean;
  /** Tam lisans durumu (Settings panel + LicenseGate icin). */
  licenseStatus?: LicenseStatus | null;
  /** Lisans durumu guncelleme (activate/deactivate sonrasi). */
  setLicenseStatus?: (s: LicenseStatus | null) => void;
  /** Premium kilit: true ise testi baslat butonu pasif + 'Aktive Et' seridi. */
  isLocked?: boolean;
  /** Kilitli 'Aktive Et' -> Settings'e yonlendirir. */
  onActivate?: () => void;
}

export type { TestResults, TabId };

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { t } from "./lib/i18n";
import { Icon } from "./lib/icons";
import { useLang } from "./lib/useLang";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface SpeedProgress {
  phase: "download" | "upload" | "ping";
  mbps: number;
  elapsed_ms: number;
}
import type {
  NicInfo, WifiInfo, ServerPingResult, PingTarget,
  BufferbloatResult, SpeedTestResult, SmartDiagnosis, FpsDiagnosis, TestPhase, TopologyInfo, ThrottleTestResult,
} from "./types";
import { Sidebar } from "./components/Sidebar";

import { TopBar } from "./components/TopBar";
import { exportPdfReport } from "./lib/pdfReport";
import { DashboardTab } from "./components/tabs/DashboardTab";
import { SpeedTestTab } from "./components/tabs/SpeedTestTab";
import { PingTab } from "./components/tabs/PingTab";
import { BufferbloatTab } from "./components/tabs/BufferbloatTab";

import { SmartDiagnosisTab } from "./components/tabs/SmartDiagnosisTab";
import { QuickDiagnosisTab } from "./components/tabs/QuickDiagnosisTab";
import { GamingDiagnosisTab } from "./components/tabs/GamingDiagnosisTab";
import { TracerouteTab } from "./components/tabs/TracerouteTab";
import { ThrottleTab } from "./components/tabs/ThrottleTab";
import { IspInterventionTab } from "./components/tabs/IspInterventionTab";
import { Ipv6Tab } from "./components/tabs/Ipv6Tab";
import { DnsBenchmarkTab } from "./components/tabs/DnsBenchmarkTab";
import { WifiScanTab } from "./components/tabs/WifiScanTab";
import { NetworkStabilityTab } from "./components/tabs/NetworkStabilityTab";
import { PingToolTab } from "./components/tabs/PingToolTab";
import { TwitchTestTab } from "./components/tabs/TwitchTestTab";
import { CgnatTab } from "./components/tabs/CgnatTab";
import { DnsLeakTab } from "./components/tabs/DnsLeakTab";
import { MtuTab } from "./components/tabs/MtuTab";
import { SettingsTab } from "./components/tabs/SettingsTab";
import { VoipQualityTab } from "./components/tabs/VoipQualityTab";
import { LanScannerTab } from "./components/tabs/LanScannerTab";
import { LanSpeedTab } from "./components/tabs/LanSpeedTab";
import { GetProTab } from "./components/tabs/GetProTab";
import { HistoryView } from "./components/HistoryView";
import { TestGuardContext, type TestGuard } from "./components/testGuard";

export type TabId =
  | "dashboard"
  | "get-pro"
  | "speedtest"
  | "latency-services"
  | "latency-games"
  | "bufferbloat"
  | "traceroute"
  | "ping"
  | "throttle"
  | "isp-intervention"
  | "ipv6"
  | "dns-benchmark"
  | "wifi-scan"
  | "network-stability"
  | "twitch-test"
  | "cgnat"
  | "mtu"
  | "dns-leak"
  | "settings"
  | "smart"
  | "quick"
  | "gaming-diagnosis"
  | "voip-quality"
  | "lan-scanner"
  | "lan-speed"
  | "history";

export interface TestResults {
  nics: NicInfo[] | null;
  wifi: WifiInfo | null;
  wifiScan: import("./types").WifiScanResult | null;
  pings: ServerPingResult[] | null;
  bufferbloat: BufferbloatResult | null;
  throttle: ThrottleTestResult | null;
  ispIntervention: import("./types").IspInterventionResult | null;
  ipv6: import("./types").Ipv6TestResult | null;
  cgnat: import("./types").CgnatResult | null;
  mtu: import("./types").MtuResult | null;
  voip: import("./types").VoipQualityResult | null;
  dnsBenchmark: import("./types").DnsBenchmarkResult | null;
  smart: SmartDiagnosis | null;
  /** Advanced (Full) tani sonucu — Quick'ten AYRI tutulur (biri digerini ezmez). */
  smartFull: SmartDiagnosis | null;
  /** Advanced tanisinin BASLADIGI/BITTIGI andaki degismez olcum goruntusu — PDF buradan uretilir
   *  (global `results` sonradan baska testlerle degisse bile rapor tutarli kalir). */
  smartFullSnapshot: ReportSnapshot | null;
  fps: FpsDiagnosis | null;
  lastRunAt: Date | null;
  speedServer: string | null;
  speedDl: number | null;
  speedUl: number | null;
  speedPing: number | null;
}

/** Advanced tanisi + o tanisinin girdisi olan olcumler (PDF export icin). */
export type ReportSnapshot = Omit<TestResults, "smartFullSnapshot" | "smartFull" | "smart" | "fps"> & { smartFull: SmartDiagnosis };

/**
 * Tab paneli: icerik HEP mount kalir, sadece gorunurluk degisir.
 * active=false -> display:none (unmount YOK -> local state korunur).
 */
function TabPane({ active, children }: { active: boolean; children: React.ReactNode }) {
  return <div style={{ display: active ? "contents" : "none" }}>{children}</div>;
}

function getTabHeaders(): Record<TabId, { title: string; subtitle: string }> {
  return {
    dashboard:        { title: t("tab_dashboard_title"),   subtitle: t("tab_dashboard_sub") },
    "get-pro":        { title: t("getpro_title"),          subtitle: t("getpro_subtitle") },
    speedtest:        { title: t("tab_speedtest_title"),   subtitle: t("tab_speedtest_sub") },
    "latency-services":  { title: t("tab_services_title"),   subtitle: t("tab_services_sub") },
    "latency-games":     { title: t("tab_games_title"),      subtitle: t("tab_games_sub") },
    bufferbloat:      { title: t("tab_bufferbloat_title"),subtitle: t("tab_bufferbloat_sub") },
    traceroute:       { title: t("tab_traceroute_title"), subtitle: t("tab_traceroute_sub") },
    ping:                  { title: t("tab_ping_title"),          subtitle: t("tab_ping_sub") },
    throttle:              { title: t("tab_throttle_title"),      subtitle: t("tab_throttle_sub") },
    "isp-intervention":    { title: t("tab_isp_intervention_title"), subtitle: t("tab_isp_intervention_sub") },
    ipv6:                  { title: t("tab_ipv6_title"),             subtitle: t("tab_ipv6_sub") },
    "dns-benchmark":       { title: t("tab_dns_benchmark_title"),    subtitle: t("tab_dns_benchmark_sub") },
    "wifi-scan":           { title: t("tab_wifi_scan_title"),         subtitle: t("tab_wifi_scan_sub") },
    "network-stability":   { title: t("tab_stability_title"),         subtitle: t("tab_stability_sub") },
    "twitch-test":         { title: t("tab_twitch_title"),            subtitle: t("tab_twitch_sub") },
    "cgnat":               { title: t("tab_cgnat_title"),             subtitle: t("tab_cgnat_sub") },
    "mtu":                 { title: t("tab_mtu_title"),               subtitle: t("tab_mtu_sub") },
    "dns-leak":            { title: t("tab_dns_leak_title"),          subtitle: t("tab_dns_leak_sub") },
    settings:              { title: t("tab_settings_title"),          subtitle: t("tab_settings_sub") },
    smart:                 { title: t("tab_smart_title"),         subtitle: t("tab_smart_sub") },
    quick:                 { title: t("tab_quick_title"),         subtitle: t("tab_quick_sub") },
    "gaming-diagnosis":    { title: t("tab_gaming_title"),        subtitle: t("tab_gaming_sub") },
    "voip-quality":        { title: t("tab_voip_title"),           subtitle: t("tab_voip_sub") },
    "lan-scanner":         { title: t("tab_lan_scanner_title"),    subtitle: t("tab_lan_scanner_sub") },
    "lan-speed":            { title: t("tab_lan_speed_title"),      subtitle: t("tab_lan_speed_sub") },
    "history":             { title: t("nav_history"),               subtitle: "" },
  };
}

export default function App() {
  const [activeTab, setActiveTab] = useState<TabId>("dashboard");
  const [historyModule, setHistoryModule] = useState<string | null>(null);
  const [phase, setPhase] = useState<TestPhase>("idle");
  const [testMode, setTestMode] = useState<"quick" | "full">("quick");
  const [testStartTime, setTestStartTime] = useState<number | null>(null);
  const [totalElapsedMs, setTotalElapsedMs] = useState(0);
  // Paylasilan senkron test kilidi (UI-01): ref = gercek kilit, state = yalnizca UI yeniden cizimi.
  const activeTestRef = useRef<string | null>(null);
  const [activeTest, setActiveTest] = useState<string | null>(null);
  const acquireTest = useCallback((id: string) => {
    if (activeTestRef.current !== null) return false;
    activeTestRef.current = id;
    setActiveTest(id);
    return true;
  }, []);
  const releaseTest = useCallback((id: string) => {
    if (activeTestRef.current !== id) return;
    activeTestRef.current = null;
    setActiveTest(null);
  }, []);
  const testGuard = useMemo<TestGuard>(
    () => ({ activeTest, acquire: acquireTest, release: releaseTest }),
    [activeTest, acquireTest, releaseTest],
  );
  // Ping sonuçlarını ref'te de tutuyoruz — async fonksiyonlarda stale closure’dan kaçınmak için
  const pingsRef = useRef<ServerPingResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [results, setResults] = useState<TestResults>({
    nics: null, wifi: null, wifiScan: null, pings: null, bufferbloat: null, throttle: null, ispIntervention: null, ipv6: null, cgnat: null, mtu: null, voip: null, dnsBenchmark: null, smart: null, smartFull: null, smartFullSnapshot: null, fps: null, lastRunAt: null, speedServer: null, speedDl: null, speedUl: null, speedPing: null,
  });
  const [liveDl, setLiveDl] = useState(0);
  const [liveUl, setLiveUl] = useState(0);
  const [liveSpeedPhase, setLiveSpeedPhase] = useState<"download" | "upload" | null>(null);
  // Download fazi bitince final (p90) deger burada kilitlenir -> upload sirasinda/sonrasinda degismez.
  const [finalDl, setFinalDl] = useState<number | null>(null);
  const [speedServerChoice, setSpeedServerChoice] = useState<"cloudflare" | "netveil">("cloudflare");
  const [bufferbloatServerChoice, setBufferbloatServerChoice] = useState<"cloudflare" | "netveil">("cloudflare");
  // Kaynak-bazli bufferbloat sonuc cache'i: iki sunucunun sonucu ayri saklanir,
  // ust secici degisince ilgili kaynagin tamamlanmis sonucu gosterilir (karsilastirma).
  const [bufferbloatCache, setBufferbloatCache] = useState<Record<string, BufferbloatResult>>({});
  // Her sunucu için ayrı sonucu sakla — geçiş yapınca karlaştırma yapılabilsin
  const [speedResultCache, setSpeedResultCache] = useState<Record<string, { dl: number; ul: number; ping: number | null; server: string }>>({});
  const [topology, setTopology] = useState<TopologyInfo | null>(null);
  const [topoLoading, setTopoLoading] = useState(true);
  // Guncelleme durumu — tek kaynak (check_for_update bir kez). Dashboard banner +
  // Sidebar dissli pulse + Settings satiri bu state'i kullanir.
  const [updateInfo, setUpdateInfo] = useState<import("./types").UpdateInfo | null>(null);
  const [gatewayPing, setGatewayPing] = useState<number | null>(null);
  const [cfPing, setCfPing] = useState<number | null>(null);
  // Sunucudan çekilen ping hedef listesi — skeleton için kullanılır
  const [pingTargets, setPingTargets] = useState<PingTarget[]>([]);
  const [targetsLoaded, setTargetsLoaded] = useState(false);
  const [pdfBusy, setPdfBusy] = useState(false);
  // Lisans durumu — acilista hizli cache okumasi (get_license_status) + arka planda
  // online dogrulama (validate_license). Premium kilit bu state'e baglanir.
  const [licenseStatus, setLicenseStatus] = useState<import("./types").LicenseStatus | null>(null);

  // PDF raporu disa aktar — Advanced (Full) sonucundan (TopBar'dan tetiklenir).
  const onExportPdf = async () => {
    const snap = results.smartFullSnapshot;
    if (!snap) return;
    setPdfBusy(true);
    try {
      await exportPdfReport(snap.smartFull, snap);
    } catch (e) {
      console.error("[pdf] export hatasi:", e);
    } finally {
      setPdfBusy(false);
    }
  };

  // Ping hedef listesini açılışta çek (init_ping_targets tamamlanınca hazır)
  useEffect(() => {
    // init_ping_targets() setup'ta spawn edildi, genellikle çok çabuk biter.
    // İlk çağrı hemen — sunucu fetch bitmediyse default list gelir (boş olmaz).
    // 1.5sn sonra tekrar dene: sunucu fetch tamamlanmış olursa güncel liste gelir.
    let cancelled = false;
    const load = (isRetry = false) => invoke<PingTarget[]>("get_ping_targets_list")
      .then(list => {
        if (!cancelled && list.length > 0) {
          setPingTargets(list);
          // Retry sonucu sunucudan geldiyse "Güncel" say
          if (isRetry) setTargetsLoaded(true);
        }
      })
      .catch(() => {});
    load(false);
    const timer = setTimeout(() => load(true), 1500);
    return () => { cancelled = true; clearTimeout(timer); };
  }, []);

  // Topology bilgisini uygulama açılışında çek
  useEffect(() => {
    invoke<TopologyInfo>("get_topology")
      .then(t => setTopology(t))
      .catch(() => setTopology(null))
      .finally(() => setTopoLoading(false));
  }, []);

  // Guncelleme kontrolu — açılışta bir kez (tek kaynak).
  useEffect(() => {
    invoke<import("./types").UpdateInfo>("check_for_update")
      .then(r => { if (r.has_update) setUpdateInfo(r); })
      .catch(() => {});
  }, []);

  // Lisans: once hizli cache (network yok), sonra arka planda online dogrulama.
  useEffect(() => {
    invoke<import("./types").LicenseStatus>("get_license_status")
      .then(s => setLicenseStatus(s))
      .catch(() => {});
    invoke<import("./types").LicenseStatus>("validate_license")
      .then(s => setLicenseStatus(s))
      .catch(() => {});
  }, []);

  // Network / Wi-Fi — açılışta otomatik yükle
  useEffect(() => {
    const loadNetwork = async () => {
      try {
        const [nics, wifi] = await Promise.all([
          invoke<NicInfo[]>("get_nic_info"),
          invoke<WifiInfo>("get_wifi_info").catch(() => null),
        ]);
        setResults(r => ({ ...r, nics, wifi: wifi ?? null }));
      } catch {
        // sessizce geç — test sırasında tekrar çekilecek
      }
    };
    loadNetwork();
  }, []);

  // Gateway live ping — her 2 saniyede bir
  useEffect(() => {
    const tick = async () => {
      try {
        const ms = await invoke<number | null>("ping_gateway");
        setGatewayPing(ms ?? null);
      } catch {
        setGatewayPing(null);
      }
    };
    tick();
    const id = setInterval(tick, 2000);
    return () => clearInterval(id);
  }, []);

  // Cloudflare 1.1.1.1 live ping — her 2 saniyede bir, 500ms timeout
  useEffect(() => {
    const tick = async () => {
      try {
        const ms = await invoke<number | null>("ping_host", { host: "1.1.1.1", timeoutMs: 500 });
        setCfPing(ms ?? null);
      } catch {
        setCfPing(null);
      }
    };
    tick();
    const id = setInterval(tick, 1000);
    return () => clearInterval(id);
  }, []);

  // Global event listener — hangi tab'da olursak olalım anlık hızı yakala.
  // Daha önce SpeedTestTab içindeydi, ama tab mount olmazsa event kaçıyordu.
  useEffect(() => {
    const unlisten = listen<SpeedProgress>("speedtest:progress", (event) => {
      const p = event.payload;
      if (p.phase === "download") {
        setLiveDl(p.mbps);
        setLiveSpeedPhase("download");
      } else if (p.phase === "upload") {
        setLiveUl(p.mbps);
        setLiveSpeedPhase("upload");
      }
    });
    // Download fazi bitince final (p90) degeri kilitle -> upload sonrasi rakam degismesin.
    const unlistenPhaseDone = listen<SpeedProgress>("speedtest:phase_done", (event) => {
      const p = event.payload;
      if (p.phase === "download") setFinalDl(p.mbps);
    });
    return () => {
      void unlisten.then(fn => fn());
      void unlistenPhaseDone.then(fn => fn());
    };
  }, []);

  // Ping sonuçları teker teker gelince anında state'e ekle.
  useEffect(() => {
    const unlisten = listen<ServerPingResult>("ping:result", (event) => {
      const result = event.payload;
      setResults(r => {
        const next = [
          ...(r.pings?.filter(p =>
            !(p.host === result.host && p.category === result.category)
          ) ?? []),
          result,
        ];
        pingsRef.current = next;
        return { ...r, pings: next };
      });
    });
    return () => {
      void unlisten.then(fn => fn());
    };
  }, []);

  // --- Tekil test fonksiyonları ---

  // ─── DB save helpers (best-effort) ─────────────────────────────────────────
  const extractBestPingMs = (pings: ServerPingResult[] | null): number | null => {
    const reachable = (pings ?? []).filter(p => p.reachable && typeof p.avg_ms === "number");
    if (!reachable.length) return null;
    return Math.min(...reachable.map(p => p.avg_ms));
  };
  const extractAvgJitterMs = (pings: ServerPingResult[] | null): number | null => {
    const v = (pings ?? []).filter(p => p.reachable && typeof p.jitter_ms === "number").map(p => p.jitter_ms);
    return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null;
  };
  const extractAvgLossPct = (pings: ServerPingResult[] | null): number | null => {
    const v = (pings ?? []).filter(p => typeof p.packet_loss === "number").map(p => p.packet_loss);
    return v.length ? v.reduce((a, b) => a + b, 0) / v.length : null;
  };
  const saveTestRun = (module: string, metrics: object, score: number | null, grade: string | null) => {
    invoke("save_test_run", {
      ts: Date.now(), module, metrics: JSON.stringify(metrics),
      summaryScore: score, summaryGrade: grade,
    }).catch(() => {});
  };
  // Bireysel Ping testi (Services/Game Servers) gecmise kaydeder. Kategoriye gore
  // filtreler (pingsRef hem services hem games icerir). En az 1 ulasilabilir hedef sart.
  const savePingHistory = (category: "services" | "games") => {
    const subset = (pingsRef.current ?? []).filter(p => p.category === category);
    if (subset.length === 0) return;
    saveTestRun("ping", {
      best: extractBestPingMs(subset),
      jitter: extractAvgJitterMs(subset),
      loss: extractAvgLossPct(subset),
    }, null, null);
  };
  // ─────────────────────────────────────────────────────────────────────────────

  // SERVER_BUSY (sunucu IP-kilidi: ayni anda 1 kullanici) hatasini kullanici dostu
  // i18n mesajina cevirir; diger hatalari oldugu gibi gosterir.
  const formatError = (e: unknown): string => {
    type TKey = Parameters<typeof t>[0];
    const s = String(e);
    if (s.includes("SERVER_BUSY")) return t("err_server_busy");
    // Backend kararli hata kodlari (Rust) -> yerellestirilmis mesaj.
    if (s.includes("UPLOAD_UNCONFIRMED")) return t("err_upload_unconfirmed");
    const noData = s.match(/NO_DATA:([a-z_]+)/);
    if (noData) return t("err_no_data").replace("{phase}", t(`err_phase_${noData[1]}` as TKey));
    const bb = s.match(/BB_(SAMPLES|PROBES):([a-z]+):(\d+):(\d+)/);
    if (bb) {
      return t(bb[1] === "SAMPLES" ? "err_bb_samples" : "err_bb_probes")
        .replace("{phase}", t(`err_bb_phase_${bb[2]}` as TKey))
        .replace("{got}", bb[3]).replace("{need}", bb[4]);
    }
    return s;
  };

  // Tum hiz uretecleri (tek test / Quick / Full) AYNI sunucu-anahtarli kaydi yazar (UI-07):
  // Speed sekmesi dl/ul/ping'i bu kayittan okur.
  const recordSpeed = (key: "cloudflare" | "netveil", sp: SpeedTestResult) => {
    setSpeedResultCache(prev => ({ ...prev, [key]: { dl: sp.download_mbps, ul: sp.upload_mbps, ping: sp.ping_ms, server: sp.server } }));
    setResults(r => ({ ...r, speedServer: sp.server, speedDl: sp.download_mbps, speedUl: sp.upload_mbps, speedPing: sp.ping_ms }));
  };

  const runSpeedTest = async (serverChoice?: "cloudflare" | "netveil") => {
    if (!acquireTest("speed")) return;
    setError(null);
    try {
      setPhase("running_speedtest");
      setLiveDl(0); setLiveUl(0); setLiveSpeedPhase("download"); setFinalDl(null);
      const speed = await invoke<SpeedTestResult>("run_speed_test", { server: serverChoice ?? null });
      setLiveSpeedPhase(null);
      recordSpeed(serverChoice ?? "cloudflare", speed);
      setResults(r => ({ ...r, lastRunAt: new Date() }));
      saveTestRun("speed", { dl: speed.download_mbps, ul: speed.upload_mbps, ping: speed.ping_ms, jitter: speed.jitter_ms, server: speed.server }, null, null);
      setPhase("done");
    } catch (e: unknown) { setError(formatError(e)); setPhase("error"); }
    finally { releaseTest("speed"); }
  };

  const runPingServices = async () => {
    if (!acquireTest("ping-services")) return;
    setError(null);
    try {
      setPhase("running_ping");
      // Services kategorisini temizle — sonuçlar ping:result ile teker teker gelecek
      setResults(r => ({ ...r, pings: r.pings?.filter(p => p.category !== "services") ?? null }));
      await invoke<ServerPingResult[]>("ping_game_servers", { category: "services" });
      setResults(r => ({ ...r, lastRunAt: new Date() }));
      // ping:result event'lerinin JS kuyruguna islenmesi icin kisa bekleme, sonra kaydet
      await new Promise(res => setTimeout(res, 50));
      savePingHistory("services");
      setPhase("done");
    } catch (e: unknown) { setError(String(e)); setPhase("error"); }
    finally { releaseTest("ping-services"); }
  };

  const runPingGames = async () => {
    if (!acquireTest("ping-games")) return;
    setError(null);
    try {
      setPhase("running_ping");
      // Games kategorisini temizle — sonuçlar ping:result ile teker teker gelecek
      setResults(r => ({ ...r, pings: r.pings?.filter(p => p.category !== "games") ?? null }));
      await invoke<ServerPingResult[]>("ping_game_servers", { category: "games" });
      setResults(r => ({ ...r, lastRunAt: new Date() }));
      // ping:result event'lerinin JS kuyruguna islenmesi icin kisa bekleme, sonra kaydet
      await new Promise(res => setTimeout(res, 50));
      savePingHistory("games");
      setPhase("done");
    } catch (e: unknown) { setError(String(e)); setPhase("error"); }
    finally { releaseTest("ping-games"); }
  };

  const runBufferbloat = async (serverChoice?: "cloudflare" | "netveil") => {
    if (!acquireTest("bufferbloat")) return;
    setError(null);
    try {
      setPhase("running_bufferbloat");
      const bufferbloat = await invoke<BufferbloatResult>("run_bufferbloat_test", { server: serverChoice ?? null });
      const key = serverChoice ?? "cloudflare";
      setBufferbloatCache(prev => ({ ...prev, [key]: bufferbloat }));
      setResults(r => ({ ...r, bufferbloat, lastRunAt: new Date() }));
      saveTestRun("bufferbloat", { idle: bufferbloat.idle_latency_ms, loaded: bufferbloat.loaded_latency_ms, delta: bufferbloat.latency_delta_ms, grade: bufferbloat.grade }, null, bufferbloat.grade);
      setPhase("done");
    } catch (e: unknown) { setError(formatError(e)); setPhase("error"); }
    finally { releaseTest("bufferbloat"); }
  };

  const runIpv6Test = async () => {
    if (!acquireTest("ipv6")) return;
    setError(null);
    setResults(r => ({ ...r, ipv6: null }));
    try {
      setPhase("running_ipv6" as TestPhase);
      const ipv6 = await invoke<import("./types").Ipv6TestResult>("run_ipv6_test");
      setResults(r => ({ ...r, ipv6, lastRunAt: new Date() }));
      setPhase("done");
    } catch (e: unknown) {
      setError(String(e));
      setPhase("done");
    } finally {
      releaseTest("ipv6");
    }
  };

  const runIspInterventionTest = async () => {
    if (!acquireTest("isp-intervention")) return;
    setError(null);
    setResults(r => ({ ...r, ispIntervention: null }));
    try {
      setPhase("running_isp_intervention" as TestPhase);
      const ispIntervention = await invoke<import("./types").IspInterventionResult>("run_isp_intervention_test");
      setResults(r => ({ ...r, ispIntervention, lastRunAt: new Date() }));
      setPhase("done");
    } catch (e: unknown) {
      setError(String(e));
      setPhase("done");
    } finally {
      releaseTest("isp-intervention");
    }
  };

  const runThrottleTest = async () => {
    // Paylasilan senkron kilit (stale closure'dan bagimsiz)
    if (!acquireTest("throttle")) return;
    setError(null);
    // Önceki sonucu temizle — yoksa ThrottleTab live state yerine eski result'u gösterir
    setResults(r => ({ ...r, throttle: null }));
    try {
      setPhase("running_throttle" as TestPhase);
      const throttle = await invoke<ThrottleTestResult>("run_throttle_test");
      setResults(r => ({ ...r, throttle, lastRunAt: new Date() }));
      saveTestRun("throttle", { baseline: throttle.baseline_mbps, bt: throttle.bittorrent_mbps, quic: throttle.quic_mbps, wg: throttle.wireguard_mbps, ovpn: throttle.openvpn_mbps, bt_v: throttle.bittorrent_verdict, quic_v: throttle.quic_verdict, wg_v: throttle.wireguard_verdict, ovpn_v: throttle.openvpn_verdict }, null, throttle.isp_throttles ? "F" : throttle.inconclusive ? null : "A");
      setPhase("done");
    } catch (e: unknown) {
      // Invoke hata verse bile (beklenmedik Rust panic vs.) testi "done" olarak bitir,
      // phase "error" kalırsa TopBar'da buton aktif olup tekrar tetiklenebilir.
      setError(formatError(e));
      setPhase("done");
    }
    finally { releaseTest("throttle"); }
  };

  // Gaming testi: NIC + WiFi + Ping + Bufferbloat — speed/ISP/IPv6 yok
  // Full-scale test: NIC + WiFi + [Ping] + Speed + [Bufferbloat] + Throttle + ISP + IPv6 + AI
  // Oyun testi daha önce yapıldıysa ping ve bufferbloat sonuçları yeniden kullanılır
  const runAllTests = async () => {
    // Advanced Diagnosis Pro — lisans yoksa caliStirma, Settings'e (aktivasyon) yonlendir.
    if (!(licenseStatus?.valid ?? false)) { setActiveTab("settings"); return; }
    if (!acquireTest("full")) return;
    setTestMode("full");
    setTestStartTime(Date.now());
    setError(null);

    // Tam tanilama her zaman TUM testleri taze olcer (reuse/devralma yok).
    let finalNics: NicInfo[] | null        = null;
    let finalWifi: WifiInfo | null         = null;
    // finalPings: ping sonuçları pingsRef üzerinden takip edilir
    let finalBloat: BufferbloatResult | null = null;
    let finalSpeed: SpeedTestResult | null;
    let extraThrottle: ThrottleTestResult | null = null;
    // Modul bazli hatalar: bir olcum basarisiz olsa da tani devam eder (modul sonucu null kalir).
    const moduleErrors: string[] = [];
    let extraIsp: import("./types").IspInterventionResult | null = null;
    let extraIpv6: import("./types").Ipv6TestResult | null = null;
    let extraCgnat: import("./types").CgnatResult | null = null;
    let extraMtu: import("./types").MtuResult | null = null;
    let extraVoip: import("./types").VoipQualityResult | null = null;

    try {
      // NIC + WiFi her zaman alınır (hızlı, ~100ms)
      setPhase("running_nic");
      finalNics = await invoke<NicInfo[]>("get_nic_info");
      setResults(r => ({ ...r, nics: finalNics }));

      setPhase("running_wifi");
      finalWifi = await invoke<WifiInfo>("get_wifi_info");
      setResults(r => ({ ...r, wifi: finalWifi }));

      // Ping — her zaman taze olcülür
      setPhase("running_ping");
      setResults(r => ({ ...r, pings: null }));
      await invoke<ServerPingResult[]>("ping_game_servers", { category: null });
      // pings state'e event ile yazılıyor, finalPings'i aşağıda state'ten okuyacağız

      // Hız Testi her zaman yapılır
      setPhase("running_speedtest");
      setLiveDl(0); setLiveUl(0); setLiveSpeedPhase("download"); setFinalDl(null);
      try {
        finalSpeed = await invoke<SpeedTestResult>("run_speed_test", { server: speedServerChoice ?? null });
        setLiveSpeedPhase(null);
        recordSpeed(speedServerChoice ?? "cloudflare", finalSpeed);
      } catch (e: unknown) {
        finalSpeed = null;
        setLiveSpeedPhase(null);
        setResults(r => ({ ...r, speedServer: null, speedDl: null, speedUl: null, speedPing: null }));
        moduleErrors.push(formatError(e));
      }

      // Bufferbloat — her zaman taze olcülür
      setPhase("running_bufferbloat");
      try {
        finalBloat = await invoke<BufferbloatResult>("run_bufferbloat_test");
        setBufferbloatCache(prev => ({ ...prev, cloudflare: finalBloat! }));
        setResults(r => ({ ...r, bufferbloat: finalBloat }));
      } catch (e: unknown) {
        finalBloat = null;
        setResults(r => ({ ...r, bufferbloat: null }));
        moduleErrors.push(formatError(e));
      }

      // ISP Daraltma
      setPhase("running_throttle" as TestPhase);
      try {
        const th = await invoke<ThrottleTestResult>("run_throttle_test");
        extraThrottle = th;
        setResults(r => ({ ...r, throttle: th }));
        // inconclusive = olculemedi: "A" (temiz) olarak kaydedilmez.
        saveTestRun("throttle", { baseline: th.baseline_mbps, bt: th.bittorrent_mbps, quic: th.quic_mbps, wg: th.wireguard_mbps, ovpn: th.openvpn_mbps, bt_v: th.bittorrent_verdict, quic_v: th.quic_verdict, wg_v: th.wireguard_verdict, ovpn_v: th.openvpn_verdict }, null, th.isp_throttles ? "F" : th.inconclusive ? null : "A");
      } catch (e: unknown) {
        setResults(r => ({ ...r, throttle: null }));
        moduleErrors.push(formatError(e));
      }

      // ISP Müdahale
      setPhase("running_isp_intervention" as TestPhase);
      try {
        extraIsp = await invoke<import("./types").IspInterventionResult>("run_isp_intervention_test");
      } catch (e: unknown) {
        moduleErrors.push(`${t("tab_isp_intervention_title")}: ${formatError(e)}`);
      }
      setResults(r => ({ ...r, ispIntervention: extraIsp }));

      // IPv6
      setPhase("running_ipv6" as TestPhase);
      try {
        extraIpv6 = await invoke<import("./types").Ipv6TestResult>("run_ipv6_test");
      } catch (e: unknown) {
        moduleErrors.push(`${t("tab_ipv6_title")}: ${formatError(e)}`);
      }
      setResults(r => ({ ...r, ipv6: extraIpv6 }));

      // CGNAT (Premium) — Smart skoruna dusuk agirlikla katilir
      setPhase("running_cgnat" as TestPhase);
      extraCgnat = await invoke<import("./types").CgnatResult>("run_cgnat_test").catch(() => null);
      setResults(r => ({ ...r, cgnat: extraCgnat }));

      // MTU/MSS (Lite) — Smart skoruna dusuk agirlikla katilir
      setPhase("running_mtu" as TestPhase);
      extraMtu = await invoke<import("./types").MtuResult>("run_mtu_test").catch(() => null);
      setResults(r => ({ ...r, mtu: extraMtu }));

      // VoIP Quality (Lite) — Smart skoruna dusuk agirlikla katilir
      setPhase("running_voip" as TestPhase);
      extraVoip = await invoke<import("./types").VoipQualityResult>("run_voip_quality_test").catch(() => null);
      setResults(r => ({ ...r, voip: extraVoip }));

      // DNS Benchmark (Lite) — Smart skoruna katilir (DNS hizi genel deneyimi etkiler)
      setPhase("running_dns_benchmark" as TestPhase);
      const extraDns = await invoke<import("./types").DnsBenchmarkResult>("run_dns_benchmark").catch(() => null);
      setResults(r => ({ ...r, dnsBenchmark: extraDns }));

      // Wi-Fi Scan — smart'tan ÖNCE çalışır (graceful: hata olsa null)
      const wifiScan = await invoke<import("./types").WifiScanResult>("run_wifi_scan").catch(() => null);
      setResults(r => ({ ...r, wifiScan }));

      // Smart Diagnosis
      setPhase("running_smart");
      // pings event ile state'e yazılıyor; stale closure'dan kaçınmak için pingsRef kullan
      const pingsForSmart = pingsRef.current;
      const smart = await invoke<SmartDiagnosis>("run_smart_diagnosis", {
        input: {
          nic: finalNics!, wifi: finalWifi!,
          ping_results: pingsForSmart,
          bufferbloat: finalBloat,
          speed_download: finalSpeed?.download_mbps ?? null,
          speed_upload: finalSpeed?.upload_mbps ?? null,
          throttle: extraThrottle,
          isp_intervention: extraIsp,
          ipv6: extraIpv6,
          cgnat: extraCgnat,
          mtu: extraMtu,
          voip: extraVoip,
          wifi_scan: wifiScan,
          dns_benchmark: extraDns,
          twitch: null,
        },
      });
      // Rapor goruntusu: bu tanisinin girdisi olan YEREL olcumler (global results sonradan
      // baska testlerle degisse bile PDF tutarli kalir).
      const reportSnapshot: ReportSnapshot = {
        nics: finalNics, wifi: finalWifi, wifiScan, pings: pingsForSmart,
        bufferbloat: finalBloat, throttle: extraThrottle, ispIntervention: extraIsp,
        ipv6: extraIpv6, cgnat: extraCgnat, mtu: extraMtu, voip: extraVoip, dnsBenchmark: extraDns,
        smartFull: smart, lastRunAt: new Date(),
        speedServer: finalSpeed?.server ?? null, speedDl: finalSpeed?.download_mbps ?? null,
        speedUl: finalSpeed?.upload_mbps ?? null, speedPing: finalSpeed?.ping_ms ?? null,
      };
      setResults(r => ({ ...r, smartFull: smart, smartFullSnapshot: reportSnapshot, lastRunAt: new Date() }));
      // Save full run to DB (best-effort)
      const pingSnap = pingsRef.current;
      saveTestRun("full", {
        best_ping: extractBestPingMs(pingSnap),
        jitter: extractAvgJitterMs(pingSnap),
        loss: extractAvgLossPct(pingSnap),
        dl: finalSpeed?.download_mbps,
        ul: finalSpeed?.upload_mbps,
        speed_ping: finalSpeed?.ping_ms,
        bloat_delta: finalBloat?.latency_delta_ms,
        bloat_grade: finalBloat?.grade,
      }, smart?.general_score?.total ?? null, smart?.general_score?.total != null ? (smart?.general_score?.grade ?? null) : null);
      // Also save individual module records (ping data available via pingsRef)
      saveTestRun("ping", { best: extractBestPingMs(pingSnap), jitter: extractAvgJitterMs(pingSnap), loss: extractAvgLossPct(pingSnap) }, null, null);
      if (moduleErrors.length > 0) setError(moduleErrors.join(" \u00b7 "));
      setPhase("done");
    } catch (e: unknown) {
      setError(formatError(e));
      setPhase("error");
    } finally {
      releaseTest("full");
    }
  };

  // FPS Gaming tanılaması: NIC+WiFi+Ping+Bufferbloat+WiFiScan -> run_fps_diagnosis
  // Double-invoke guard: isGamingRunning ref
  // Quick tarama: tek akışta hem genel (run_smart_diagnosis) hem oyun (run_fps_diagnosis)
  // skoru hesaplanır — ikisi de aynı toplanan veriden, pro araçlar null. Toggle yok.
  const runQuickScan = async () => {
    if (!acquireTest("quick")) return;
    setTestMode("quick");
    setTestStartTime(Date.now());
    setError(null);

    try {
      setPhase("running_nic");
      const nics = await invoke<NicInfo[]>("get_nic_info");
      setResults(r => ({ ...r, nics }));

      setPhase("running_wifi");
      const wifi = await invoke<WifiInfo>("get_wifi_info");
      setResults(r => ({ ...r, wifi }));

      // Ping — hem services hem games (null): genel ve oyun skoru en iyi veriyle beslenir
      setPhase("running_ping");
      setResults(r => ({ ...r, pings: null, fps: null }));
      await invoke<ServerPingResult[]>("ping_game_servers", { category: null });

      setPhase("running_speedtest");
      setLiveDl(0); setLiveUl(0); setLiveSpeedPhase("download"); setFinalDl(null);
      // Modul bazli hata: speed/bufferbloat basarisiz olsa da tarama devam eder (sonuc null).
      const moduleErrors: string[] = [];
      let quickSpeed: SpeedTestResult | null = null;
      try {
        const sp = await invoke<SpeedTestResult>("run_speed_test", { server: speedServerChoice ?? null });
        quickSpeed = sp;
        setLiveSpeedPhase(null);
        recordSpeed(speedServerChoice ?? "cloudflare", sp);
      } catch (e: unknown) {
        setLiveSpeedPhase(null);
        setResults(r => ({ ...r, speedDl: null, speedUl: null, speedPing: null }));
        moduleErrors.push(formatError(e));
      }

      setPhase("running_bufferbloat");
      let rawBloat: BufferbloatResult | null = null;
      try {
        const bb = await invoke<BufferbloatResult>("run_bufferbloat_test");
        rawBloat = bb;
        // Quick/Full sunucu argumansiz calisir -> varsayilan kaynak (cloudflare); kaynak-bazli cache'i de guncelle.
        setBufferbloatCache(prev => ({ ...prev, cloudflare: bb }));
        setResults(r => ({ ...r, bufferbloat: bb }));
      } catch (e: unknown) {
        setResults(r => ({ ...r, bufferbloat: null }));
        moduleErrors.push(formatError(e));
      }

      const wifiScan = await invoke<import("./types").WifiScanResult>("run_wifi_scan").catch(() => null);
      setResults(r => ({ ...r, wifiScan }));

      setPhase("running_ipv6");
      const quickIpv6 = await invoke<import("./types").Ipv6TestResult>("run_ipv6_test").catch(() => null);
      setResults(r => ({ ...r, ipv6: quickIpv6 }));

      setPhase("running_mtu");
      const quickMtu = await invoke<import("./types").MtuResult>("run_mtu_test").catch(() => null);
      setResults(r => ({ ...r, mtu: quickMtu }));

      setPhase("running_dns_benchmark");
      const quickDns = await invoke<import("./types").DnsBenchmarkResult>("run_dns_benchmark").catch(() => null);
      setResults(r => ({ ...r, dnsBenchmark: quickDns }));

      setPhase("running_smart");
      const pingsForQuick = pingsRef.current;
      // Genel skor (5 çekirdek + speed, pro araçlar null)
      const smart = await invoke<SmartDiagnosis>("run_smart_diagnosis", {
        input: {
          nic: nics, wifi,
          ping_results: pingsForQuick,
          bufferbloat: rawBloat,
          speed_download: quickSpeed?.download_mbps ?? null,
          speed_upload: quickSpeed?.upload_mbps ?? null,
          throttle: null, isp_intervention: null,
          ipv6: quickIpv6, cgnat: null, mtu: quickMtu, voip: null,
          wifi_scan: wifiScan, dns_benchmark: quickDns, twitch: null,
        },
      });
      // Oyun skoru (rekabetçi/sert eşikler) — aynı toplanan veriden
      const fps = await invoke<FpsDiagnosis>("run_fps_diagnosis", {
        input: {
          nic: nics, wifi,
          ping_results: pingsForQuick,
          bufferbloat: rawBloat,
          speed_download: null, speed_upload: null,
          throttle: null, isp_intervention: null,
          ipv6: null, cgnat: null, mtu: null, voip: null,
          wifi_scan: wifiScan, dns_benchmark: null, twitch: null,
        },
      });
      setResults(r => ({ ...r, smart, fps, lastRunAt: new Date() }));

      if (moduleErrors.length > 0) setError(moduleErrors.join(" \u00b7 "));
      setPhase("done");
    } catch (e: unknown) {
      setError(formatError(e));
      setPhase("error");
    } finally {
      releaseTest("quick");
    }
  };

  // Tam tanılamada hangi adımlar oyun testinden devralındı
  // Global timer — App seviyesinde çalışır, sayfa değişiminde sıfırlanmaz
  useEffect(() => {
    const running = phase.startsWith("running_");
    if (!running || !testStartTime) {
      if (!phase.startsWith("running_")) setTotalElapsedMs(0);
      return;
    }
    setTotalElapsedMs(Date.now() - testStartTime);
    const id = setInterval(() => setTotalElapsedMs(Date.now() - testStartTime), 100);
    return () => clearInterval(id);
  }, [phase, testStartTime]);

  const tabProps = {
    results, phase, testMode, testStartTime, totalElapsedMs, error, setActiveTab, runAllTests, runSpeedTest, runPingServices, runPingGames, runBufferbloat, runThrottleTest, runIspInterventionTest, runIpv6Test,
    liveDl, liveUl, liveSpeedPhase, finalDl,
    speedServerChoice, setSpeedServerChoice, speedResultCache,
    bufferbloatServerChoice, setBufferbloatServerChoice, bufferbloatCache,
    pingTargets, topology, topoLoading, gatewayPing, cfPing, updateInfo,
    runQuickScan,
    isLicensed: licenseStatus?.valid ?? false,
    licenseStatus,
    setLicenseStatus,
  };
  // Premium kilit: lisans gecerli mi (Advanced Diagnosis + 9 premium arac).
  const isPro = licenseStatus?.valid ?? false;
  // Kilitli premium araclarda 'Aktive Et' -> Settings (lisans paneli).
  const goToSettings = () => setActiveTab("settings");

  // Tab değişince geçmiş görünümünü kapat
  useEffect(() => { setHistoryModule(null); }, [activeTab]);

  useLang(); // dil değişince re-render
  const head = getTabHeaders()[activeTab];

  return (
    <TestGuardContext.Provider value={testGuard}>
    <div className="app">
      <Sidebar
        activeTab={activeTab}
        onTabChange={setActiveTab}
        targetsLoaded={targetsLoaded}
        hasUpdate={!!updateInfo}
        isLicensed={licenseStatus?.valid ?? false}
      />

      <main className="main">
        <TopBar
          title={head.title}
          subtitle={head.subtitle}
          phase={phase}
          onRun={
            activeTab === "speedtest"     ? () => runSpeedTest(speedServerChoice) :
            activeTab === "latency-services" ? runPingServices :
            activeTab === "latency-games"    ? runPingGames :
            activeTab === "bufferbloat"   ? () => runBufferbloat(bufferbloatServerChoice) :
            activeTab === "throttle"          ? runThrottleTest :
            activeTab === "isp-intervention" ? runIspInterventionTest :
            activeTab === "ipv6"             ? runIpv6Test :
            activeTab === "history"           ? () => {} :
            activeTab === "quick"            ? runQuickScan :
            activeTab === "gaming-diagnosis" ? runQuickScan :
            runAllTests
          }
          buttonLabel={
            activeTab === "dashboard"     ? null :
            activeTab === "speedtest"     ? t("btn_run_speed") :
            activeTab === "latency-services" ? t("btn_run_services") :
            activeTab === "latency-games"    ? t("btn_run_games") :
            // Bufferbloat/Throttle/ISP/IPv6: bos durumda buton TestIntro'da -> TopBar gizli;
            // sonuc gelince tekrar-test icin TopBar butonu gorunur.
            activeTab === "bufferbloat"   ? (results.bufferbloat ? t("btn_run_bufferbloat") : null) :
            activeTab === "throttle"          ? (results.throttle ? t("btn_run_throttle") : null) :
            activeTab === "isp-intervention"  ? (results.ispIntervention ? t("btn_run_isp_intervention") : null) :
            activeTab === "ipv6"               ? (results.ipv6 ? t("btn_run_ipv6") : null) :
            activeTab === "dns-benchmark"       ? null :
            activeTab === "history"              ? null :
            null
          }
          extraButtons={
            activeTab === "smart" && results.smartFullSnapshot ? (
              <button
                className="btn btn-secondary"
                type="button"
                onClick={onExportPdf}
                disabled={pdfBusy}
              >
                <Icon.Down style={{ width: 15, height: 15 }} />
                {t("pdf_export_btn")}
              </button>
            ) :
            (activeTab === "speedtest" || activeTab === "bufferbloat" ||
             activeTab === "latency-services" || activeTab === "latency-games" ||
             activeTab === "voip-quality" || activeTab === "throttle" || activeTab === "lan-speed") ? (
              <button
                className="btn btn-secondary"
                type="button"
                onClick={() => setHistoryModule(m =>
                  m ? null : (
                    activeTab === "speedtest" ? "speed" :
                    activeTab === "bufferbloat" ? "bufferbloat" :
                    activeTab === "voip-quality" ? "voip" :
                    activeTab === "throttle" ? "throttle" :
                    activeTab === "lan-speed" ? "lan_speed" : "ping"
                  ))}
              >
                <Icon.History />
                {t("history_btn")}
              </button>
            ) : undefined
          }
        />

        {/* Kaydırılabilir içerik alanı — scrollbar gizli */}
        <div className="main-scroll">
          {error && (
            <div
              className="card"
              style={{
                padding: "10px 14px",
                marginBottom: 12,
                background: "rgba(239,68,68,0.08)",
                borderColor: "rgba(239,68,68,0.3)",
                color: "#fca5a5",
                fontSize: 13,
              }}
            >
              <strong>{t("common_error")}</strong> {error}
            </div>
          )}

          {/* Tum tab'lar HEP mount kalir; sadece aktif olan gorunur (display).
              Boylece local-state tutan tab'lar (Traceroute/Ping/DNS/WiFi) sayfa
              degisince sonucunu KAYBETMEZ. Sonuclar bellekte canli -> export'a hazir. */}
          <TabPane active={activeTab === "dashboard"}>        <DashboardTab   {...tabProps} /></TabPane>
          <TabPane active={activeTab === "speedtest"}>        <SpeedTestTab   {...tabProps} showHistory={historyModule === "speed"} /></TabPane>
          <TabPane active={activeTab === "latency-services"}> <PingTab        {...tabProps} category="services" showHistory={historyModule === "ping"} /></TabPane>
          <TabPane active={activeTab === "latency-games"}>    <PingTab        {...tabProps} category="games" showHistory={historyModule === "ping"} /></TabPane>
          <TabPane active={activeTab === "bufferbloat"}>      <BufferbloatTab {...tabProps} showHistory={historyModule === "bufferbloat"} /></TabPane>
          <TabPane active={activeTab === "traceroute"}>       <TracerouteTab /></TabPane>
          <TabPane active={activeTab === "ping"}>             <PingToolTab /></TabPane>
          <TabPane active={activeTab === "throttle"}>         <ThrottleTab {...tabProps} showHistory={historyModule === "throttle"} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "isp-intervention"}> <IspInterventionTab result={isPro ? results.ispIntervention : null} phase={phase} onRun={runIspInterventionTest} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "ipv6"}>             <Ipv6Tab result={results.ipv6} phase={phase} onRun={runIpv6Test} /></TabPane>
          <TabPane active={activeTab === "dns-benchmark"}>    <DnsBenchmarkTab /></TabPane>
          <TabPane active={activeTab === "wifi-scan"}>        <WifiScanTab /></TabPane>
          <TabPane active={activeTab === "network-stability"}><NetworkStabilityTab isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "twitch-test"}>      <TwitchTestTab isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "cgnat"}>            <CgnatTab results={results} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "mtu"}>              <MtuTab results={results} /></TabPane>
          <TabPane active={activeTab === "dns-leak"}>         <DnsLeakTab isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "settings"}>         <SettingsTab updateInfo={updateInfo} licenseStatus={licenseStatus} setLicenseStatus={setLicenseStatus} /></TabPane>
          <TabPane active={activeTab === "smart"}>            <SmartDiagnosisTab {...tabProps} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "quick"}>            <QuickDiagnosisTab smartDiagnosis={results.smart} phase={phase} onRun={runQuickScan} testMode={testMode} totalElapsedMs={totalElapsedMs} /></TabPane>
          <TabPane active={activeTab === "gaming-diagnosis"}>  <GamingDiagnosisTab fpsDiagnosis={results.fps} phase={phase} onRun={runQuickScan} testMode={testMode} totalElapsedMs={totalElapsedMs} /></TabPane>
          <TabPane active={activeTab === "voip-quality"}>  <VoipQualityTab showHistory={historyModule === "voip"} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "lan-scanner"}>   <LanScannerTab isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "lan-speed"}>     <LanSpeedTab showHistory={historyModule === "lan_speed"} isLocked={!isPro} onActivate={goToSettings} /></TabPane>
          <TabPane active={activeTab === "get-pro"}>        <GetProTab setActiveTab={setActiveTab} /></TabPane>
          <TabPane active={activeTab === "history"}>        <HistoryView title={t("nav_history")} active={activeTab === "history"} /></TabPane>
        </div>
      </main>
    </div>
    </TestGuardContext.Provider>
  );
}

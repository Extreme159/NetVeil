import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { filterRemotePresets, type PresetTarget } from "../../lib/remotePresets";
import { useTestGuard } from "../testGuard";
import { TestBusyHint } from "../TestBusyHint";

interface HopResult {
  hop: number;
  ip: string;
  hostname: string | null;
  rtt_ms: number | null;   // en düşük (min) RTT
  min_ms?: number | null;
  max_ms?: number | null;
  timed_out: boolean;
  country: string | null;
  country_name: string | null;
  run_id?: number;         // backend echo of the UI-chosen run id
}

// GeoIP paralel çekildikten sonra gelen güncelleme (hop event'inden sonra).
interface HopGeo {
  hop: number;
  country: string | null;
  country_name: string | null;
  run_id?: number;
}

// Reverse DNS (hostname) hop yayınlandıktan sonra ayrı event ile gelir.
interface HopHost {
  hop: number;
  hostname: string;
  run_id?: number;
}

type TraceState = "idle" | "running" | "done" | "error";
type ViewMode = "table" | "waterfall";
type IpVersion = "auto" | "4" | "6";

const FALLBACK_PRESETS: PresetTarget[] = [
  { label: "Cloudflare DNS",   host: "1.1.1.1",                ip_support: "4" },
  { label: "Google DNS",       host: "8.8.8.8",                ip_support: "4" },
  { label: "Google",           host: "google.com",             ip_support: "both" },
  { label: "YouTube",          host: "youtube.com",            ip_support: "both" },
  { label: "Discord",          host: "discord.com",            ip_support: "both" },
  { label: "Steam",            host: "store.steampowered.com", ip_support: "4" },
  { label: "Valve Vienna SDR", host: "146.66.155.1",           ip_support: "4" },
];

function rttColor(ms: number): string {
  if (ms < 5)   return "#4ade80";
  if (ms < 20)  return "#86efac";
  if (ms < 50)  return "#fbbf24";
  if (ms < 100) return "#f97316";
  return "#f87171";
}

// RTT gosterimi: Windows IcmpSendEcho.RoundTripTime tam-ms (u32) doner, <1ms hop'lar
// (ozellikle 1. hop LAN gateway) 0 yuvarlanir. Dururst gosterim: "< 1 ms".
function fmtRtt(ms: number): string {
  return ms < 0.05 ? "< 1 ms" : `${ms.toFixed(1)} ms`;
}

// Bayrak/ev ikonu — sabit-genislik kutuda ortalanir, yan yazi hep ayni hizadan baslar.
const FLAG_BOX: React.CSSProperties = {
  display: "inline-flex", justifyContent: "center", alignItems: "center",
  width: "1.35em", flexShrink: 0,
};
function countryFlag(code: string) {
  const glyph = (!code || code === "LAN")
    ? "🏠"
    : code
        .toUpperCase()
        .split("")
        .map((c) => String.fromCodePoint(0x1f1e6 - 65 + c.charCodeAt(0)))
        .join("");
  return <span style={FLAG_BOX}>{glyph}</span>;
}

// ── Segment tespiti ──────────────────────────────────────────────────────────
// RTT + ülke geçişine göre her hopa segment etiketi atar.
// LAN: ilk hop veya RTT < 5ms ve ülke "LAN"
// ISP: aynı ülke, RTT < 80ms
// Transit: ülke değişimi
// Target: son hop

type SegmentType = "lan" | "isp" | "transit" | "target";

interface SegmentLabel {
  type: SegmentType;
  label: string;
  color: string;
}

const SEG_COLORS: Record<SegmentType, string> = {
  lan:     "#4ade80",
  isp:     "#60a5fa",
  transit: "#f59e0b",
  target:  "#a78bfa",
};

// State-machine segment hesabı:
// LAN → ISP → TRANSIT → HEDEF — geri dönüş yok.
// Tüm hop listesi üzerinde tek geçişte her hopa segment atar.
function buildSegments(hops: HopResult[], isDone: boolean): Array<SegmentLabel | null> {
  // İlk non-LAN, non-timeout ülkeyi bul = "kendi ülken" (ISP ülkesi)
  const homeCountry = hops.find(h => !h.timed_out && h.country && h.country !== "LAN")?.country ?? null;

  type State = "lan" | "isp" | "transit";
  let state: State = "lan";
  const result: Array<SegmentLabel | null> = [];

  for (let i = 0; i < hops.length; i++) {
    const hop = hops[i];
    const isLast = isDone && i === hops.length - 1;

    if (hop.timed_out) {
      result.push(null);
      continue;
    }

    // Son hop: HEDEF
    if (isLast) {
      result.push({ type: "target", label: t("trace_seg_target"), color: SEG_COLORS.target });
      continue;
    }

    // LAN
    if (hop.country === "LAN" || i === 0) {
      state = "lan";
      result.push({ type: "lan", label: t("trace_seg_lan"), color: SEG_COLORS.lan });
      continue;
    }

    // Ülke bilgisi yok — mevcut state devam eder, etiket yok
    if (!hop.country) {
      result.push(null);
      continue;
    }

    // State machine geçişleri
    if (state === "lan") {
      state = "isp";
    }
    if (state === "isp" && hop.country !== homeCountry) {
      state = "transit";
    }
    // TRANSIT'ten geri dönüş yok

    const seg = state as SegmentType;
    result.push({ type: seg, label: t(`trace_seg_${seg}`), color: SEG_COLORS[seg] });
  }

  return result;
}

// Tek hop için segment — buildSegments sonucundan çeker
function getSegment(_hop: HopResult, idx: number, hops: HopResult[], isDone: boolean): SegmentLabel | null {
  return buildSegments(hops, isDone)[idx] ?? null;
}

// ── RTT Sıçrama tespiti ──────────────────────────────────────────────────────
// Bir önceki yanıt veren hop'a göre delta hesaplar.
// Delta > 20ms → sıçrama işareti

function getRttDelta(hop: HopResult, idx: number, hops: HopResult[]): number | null {
  if (hop.timed_out || hop.rtt_ms === null) return null;
  const prev = hops.slice(0, idx).reverse().find(h => !h.timed_out && h.rtt_ms !== null);
  if (!prev || prev.rtt_ms === null) return null;
  return hop.rtt_ms - prev.rtt_ms;
}

// ── Özet istatistikler ───────────────────────────────────────────────────────

function TraceSummary({ hops, state }: { hops: HopResult[]; state: TraceState }) {
  if (hops.length === 0) return null;

  const responding = hops.filter(h => !h.timed_out && h.rtt_ms !== null);
  const totalRtt = responding.length > 0
    ? responding[responding.length - 1].rtt_ms!.toFixed(1)
    : null;
  const timeouts = hops.filter(h => h.timed_out).length;
  const countries = new Set(
    hops.filter(h => !h.timed_out && h.country && h.country !== "LAN")
        .map(h => h.country)
  ).size;

  const items = [
    { value: hops.length, label: t("trace_summary_hops"), color: "var(--text)" },
    ...(totalRtt ? [{ value: `${totalRtt} ms`, label: t("trace_summary_total_rtt"), color: rttColor(parseFloat(totalRtt)) }] : []),
    ...(timeouts > 0 ? [{ value: timeouts, label: t("trace_summary_timeouts"), color: "#f87171" }] : []),
    ...(countries > 0 ? [{ value: countries, label: t("trace_summary_countries"), color: "#a78bfa" }] : []),
  ];

  return (
    <div style={{
      display: "flex", gap: 20, flexWrap: "wrap",
      padding: "10px 18px",
      borderTop: "1px solid var(--border)",
      background: "rgba(255,255,255,0.02)",
    }}>
      {state === "running" && (
        <span style={{ fontSize: 11, color: "var(--accent)", display: "flex", alignItems: "center", gap: 5, marginRight: 4 }}>
          <span style={{ animation: "pulse 1s ease infinite" }}>●</span> {t("trace_monitoring")}
        </span>
      )}
      {items.map((item, i) => (
        <div key={i} style={{ display: "flex", alignItems: "baseline", gap: 5 }}>
          <span style={{ fontFamily: "var(--mono)", fontSize: 14, fontWeight: 700, color: item.color }}>{item.value}</span>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{item.label}</span>
        </div>
      ))}
    </div>
  );
}

// ── Tablo görünümü ───────────────────────────────────────────────────────────

// ── Memoized segment listesi prop olarak geçilir, her render'da yeniden hesaplanmaz
function HopRow({ hop, idx, hops, isNew, isDone }: {
  hop: HopResult; idx: number; hops: HopResult[]; isNew: boolean; isDone: boolean;
}) {
  const isTimeout = hop.timed_out || hop.rtt_ms === null;
  const segment = getSegment(hop, idx, hops, isDone);
  const delta = getRttDelta(hop, idx, hops);
  const isJump = delta !== null && delta > 20;

  return (
    <tr style={{ animation: isNew ? "hopSlideIn 0.25s ease" : undefined }}>
      {/* # */}
      <td style={{ paddingLeft: 18, width: 42 }}>
        <span className="mono" style={{ color: "var(--text-muted)", fontSize: 12 }}>{hop.hop}</span>
      </td>

      {/* Segment badge */}
      <td style={{ width: 70 }}>
        {segment && (
          <span style={{
            fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 3,
            background: `${segment.color}18`, color: segment.color,
            border: `1px solid ${segment.color}40`,
          }}>
            {segment.label}
          </span>
        )}
      </td>

      {/* IP */}
      <td style={{ width: 145 }}>
        <span className="mono" style={{ fontSize: 12, color: isTimeout ? "var(--text-muted)" : "var(--text)" }}>
          {isTimeout ? "* * *" : hop.ip}
        </span>
      </td>

      {/* Hostname */}
      <td>
        <span style={{ fontSize: 11, color: "var(--text-dim)" }}>{hop.hostname ?? hop.ip}</span>
      </td>

      {/* Ülke */}
      <td style={{ width: 130 }}>
        {isTimeout ? (
          <span style={{ color: "var(--text-muted)", fontSize: 12 }}>—</span>
        ) : hop.country ? (
          <span style={{ fontSize: 12, display: "flex", alignItems: "center", gap: 5 }}>
            <span style={{ fontSize: 16, lineHeight: 1 }}>{countryFlag(hop.country)}</span>
            <span style={{ color: "var(--text-dim)" }}>{hop.country_name ?? hop.country}</span>
          </span>
        ) : (
          <span className="mono" style={{ fontSize: 11, color: "var(--text-muted)" }}>…</span>
        )}
      </td>

      {/* RTT + delta */}
      <td style={{ paddingRight: 18, textAlign: "right", width: 130 }}>
        {isTimeout ? (
          <span className="mono" style={{ color: "var(--text-muted)", fontSize: 12 }}>—</span>
        ) : (
          <span style={{ display: "flex", alignItems: "center", justifyContent: "flex-end", gap: 6 }}>
            {isJump && (
              <span style={{
                fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 3,
                background: "rgba(239,68,68,0.12)", color: "#f87171",
                border: "1px solid rgba(239,68,68,0.25)",
              }}>
                +{delta!.toFixed(0)}ms
              </span>
            )}
            <span className="mono" style={{ fontSize: 12, color: rttColor(hop.rtt_ms!) }}>
              {fmtRtt(hop.rtt_ms!)}
            </span>
          </span>
        )}
      </td>
    </tr>
  );
}

function PulsingRow({ n }: { n: number }) {
  return (
    <tr style={{ opacity: 0.4 }}>
      <td style={{ paddingLeft: 18 }}>
        <span className="mono" style={{ fontSize: 12, color: "var(--text-muted)" }}>{n}</span>
      </td>
      <td colSpan={5}>
        <span style={{ fontSize: 12, color: "var(--accent)", animation: "pulse 1.2s ease infinite" }}>
          {t("trace_waiting_row")}
        </span>
      </td>
    </tr>
  );
}

// ── Waterfall görünümü ───────────────────────────────────────────────────────

function WaterfallView({ hops, isDone }: { hops: HopResult[]; isDone: boolean }) {
  if (hops.length === 0) return (
    <div style={{ padding: "36px 0", textAlign: "center", color: "var(--text-muted)", fontSize: 13 }}>
      {t("trace_empty")}
    </div>
  );

  const maxRtt = Math.max(...hops.filter(h => h.rtt_ms !== null).map(h => h.rtt_ms!), 1);

  // Segment renk bandı için hop grupları
  let lastCountry: string | null = null;
  const segBand: Array<{ color: string; opacity: number }> = [];
  for (let i = 0; i < hops.length; i++) {
    const hop = hops[i];
    const seg = getSegment(hop, i, hops, isDone);
    segBand.push({ color: seg?.color ?? "transparent", opacity: seg ? 0.06 : 0 });
    if (!hop.timed_out && hop.country) lastCountry = hop.country;
  }
  void lastCountry;

  return (
    <div style={{ padding: "12px 18px 16px", display: "flex", flexDirection: "column", gap: 5 }}>
      {hops.map((hop, idx) => {
        const isTimeout = hop.timed_out || hop.rtt_ms === null;
        const segment = getSegment(hop, idx, hops, isDone);
        const delta = getRttDelta(hop, idx, hops);
        const isJump = delta !== null && delta > 20;
        const pct = isTimeout ? 0 : Math.min((hop.rtt_ms! / maxRtt) * 100, 100);
        const color = isTimeout ? "rgba(255,255,255,0.12)" : rttColor(hop.rtt_ms!);
        const band = segBand[idx];

        return (
          <div
            key={hop.hop}
            style={{
              display: "flex", alignItems: "center", gap: 10,
              padding: "5px 8px", borderRadius: 6,
              background: band.opacity > 0 ? `${band.color}${Math.round(band.opacity * 255).toString(16).padStart(2, "0")}` : "transparent",
              animation: idx === hops.length - 1 ? "hopSlideIn 0.2s ease" : undefined,
            }}
          >
            {/* Hop numarası */}
            <span style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--text-muted)", minWidth: 22, textAlign: "right" }}>
              {hop.hop}
            </span>

            {/* Segment badge */}
            <div style={{ minWidth: 52 }}>
              {segment && (
                <span style={{
                  fontSize: 8, fontWeight: 700, padding: "1px 4px", borderRadius: 3,
                  background: `${segment.color}18`, color: segment.color,
                  border: `1px solid ${segment.color}35`,
                }}>
                  {segment.label}
                </span>
              )}
            </div>

            {/* Bar */}
            <div style={{ flex: 1, height: 14, borderRadius: 3, background: "rgba(255,255,255,0.05)", overflow: "hidden", position: "relative" }}>
              <div style={{
                height: "100%", width: `${pct}%`, borderRadius: 3,
                background: isTimeout ? "rgba(255,255,255,0.08)" : color,
                opacity: isTimeout ? 1 : 0.75,
                transition: "width 0.3s ease",
              }} />
              {/* Sıçrama işareti */}
              {isJump && (
                <div style={{
                  position: "absolute", top: 0, left: `${pct}%`,
                  height: "100%", width: 2,
                  background: "#ef4444",
                  boxShadow: "0 0 4px #ef4444",
                }} />
              )}
            </div>

            {/* RTT değeri */}
            <div style={{ minWidth: 64, textAlign: "right" }}>
              {isTimeout ? (
                <span style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--text-muted)" }}>* * *</span>
              ) : (
                <span style={{ fontFamily: "var(--mono)", fontSize: 12, color, fontWeight: 600 }}>
                  {fmtRtt(hop.rtt_ms!)}
                </span>
              )}
            </div>

            {/* Delta badge */}
            <div style={{ minWidth: 56 }}>
              {isJump && (
                <span style={{
                  fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 3,
                  background: "rgba(239,68,68,0.12)", color: "#f87171",
                  border: "1px solid rgba(239,68,68,0.25)",
                }}>
                  +{delta!.toFixed(0)}ms
                </span>
              )}
            </div>

            {/* Ülke */}
            <div style={{ minWidth: 90, display: "flex", alignItems: "center", gap: 4 }}>
              {!isTimeout && hop.country && (
                <>
                  <span style={{ fontSize: 13 }}>{countryFlag(hop.country)}</span>
                  <span style={{ fontSize: 10, color: "var(--text-muted)" }}>
                    {hop.country === "LAN" ? "LAN" : hop.country}
                  </span>
                </>
              )}
            </div>

            {/* Hostname (kısaltılmış) — hostname yoksa IP fallback */}
            <div style={{ minWidth: 0, flex: "0 1 200px", overflow: "hidden" }}>
              {!isTimeout && (
                <span style={{
                  fontSize: 10, color: "var(--text-muted)",
                  whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis",
                  display: "block",
                }}>
                  {hop.hostname ?? hop.ip}
                </span>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ── Ana Component ─────────────────────────────────────────────────────────────

export function TracerouteTab() {
  const [presets, setPresets] = useState<PresetTarget[]>(FALLBACK_PRESETS);
  const [target, setTarget] = useState("1.1.1.1");
  const [customTarget, setCustomTarget] = useState("");
  const [useCustom, setUseCustom] = useState(false);
  const [state, setState] = useState<TraceState>("idle");
  const [hops, setHops] = useState<HopResult[]>([]);
  const [newHopIdx, setNewHopIdx] = useState<number | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("waterfall");
  const [ipVersion, setIpVersion] = useState<IpVersion>("auto");
  const [resolveHostnames, setResolveHostnames] = useState(true);
  const guard = useTestGuard();

  const unlistenRef = useRef<UnlistenFn[]>([]);
  // Id of the current run; events carrying another run_id (stale run) are ignored.
  const runIdRef = useRef(0);
  // run_traceroute returns immediately (the trace runs in a backend task), so the shared test guard is
  // owned until THIS run's done/error event, Stop/Reset, unmount or a setup/invoke failure.
  // Only the run that acquired it (guardRunRef) may release it.
  const guardRunRef = useRef<number | null>(null);
  const { release } = guard;
  const releaseOwned = useCallback((owner: number) => {
    if (guardRunRef.current !== owner) return;
    guardRunRef.current = null;
    release("traceroute");
  }, [release]);
  const tableBottomRef = useRef<HTMLTableRowElement>(null);

  const effectiveTarget = useCustom ? customTarget.trim() : target;

  // Seçili preset'in IP desteğini bul — manuel girişte kısıtlama yok
  const selectedPreset = useCustom ? null : presets.find(p => p.host === target);
  const presetIpSupport = selectedPreset?.ip_support ?? "both";

  // Preset değişince ip_version'ı otomatik uyarla
  const handleTargetChange = (host: string) => {
    setTarget(host);
    const preset = presets.find(p => p.host === host);
    const support = preset?.ip_support ?? "both";
    if (support === "4" && ipVersion === "6") setIpVersion("4");
    if (support === "6" && ipVersion === "4") setIpVersion("6");
  };

  const loadPresets = useCallback(() => {
    fetch("https://api.netveil.net/traceroute-presets")
      .then(r => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.json(); })
      .then((data: unknown) => {
        const valid = filterRemotePresets(data);
        setPresets(valid.length > 0 ? valid : FALLBACK_PRESETS);
      })
      .catch(() => {});
  }, []);

  useEffect(() => { loadPresets(); }, [loadPresets]);

  const cleanup = () => {
    const fns = unlistenRef.current;
    unlistenRef.current = [];
    fns.forEach(fn => fn());
    // Drain in place: runTrace's abortIfStale holds the same array and must not unlisten twice.
    fns.length = 0;
  };

  // Tabs stay mounted when the user switches tabs; this cleanup only runs on a real unmount.
  // It invalidates the active frontend run (pending listen() setup included) and cancels the
  // backend trace so nothing keeps probing/emitting.
  useEffect(() => {
    return () => {
      const owner = runIdRef.current;
      runIdRef.current += 1;
      cleanup();
      releaseOwned(owner);
      invoke("stop_traceroute").catch(() => {});
    };
  }, [releaseOwned]);

  useEffect(() => {
    if (newHopIdx !== null) {
      tableBottomRef.current?.scrollIntoView({ behavior: "smooth", block: "nearest" });
    }
  }, [newHopIdx]);

  const runTrace = async () => {
    if (!effectiveTarget) return;
    if (!guard.acquire("traceroute")) return;
    cleanup();
    const runId = Math.max(Date.now(), runIdRef.current + 1);
    runIdRef.current = runId;
    guardRunRef.current = runId;
    // A run is current only until Reset/Stop/unmount/new run replaces runIdRef.
    const isCurrent = () => runIdRef.current === runId;
    setState("running");
    setHops([]);
    setNewHopIdx(null);
    setErrorMsg(null);

    // Registrations of THIS run. cleanup() (Reset/Stop) detaches the ones already installed;
    // anything still pending is unlistened right after its listen() resolves (abortIfStale).
    const fns: UnlistenFn[] = [];
    unlistenRef.current = fns;
    const abortIfStale = () => {
      if (isCurrent()) return false;
      fns.forEach(fn => fn());
      fns.length = 0;
      return true;
    };

    try {
      fns.push(await listen<HopResult>("traceroute:hop", (e) => {
        if (!isCurrent() || e.payload.run_id !== runId) return;
        setHops(prev => {
          const exists = prev.findIndex(h => h.hop === e.payload.hop);
          if (exists >= 0) {
            const next = [...prev];
            next[exists] = e.payload;
            return next;
          }
          return [...prev, e.payload];
        });
        setNewHopIdx(e.payload.hop);
      }));
      if (abortIfStale()) return;

      // GeoIP paralel geldiğinde ilgili hop'un ülke alanlarını güncelle.
      fns.push(await listen<HopGeo>("traceroute:hop_geo", (e) => {
        if (!isCurrent() || e.payload.run_id !== runId) return;
        setHops(prev =>
          prev.map(h =>
            h.hop === e.payload.hop
              ? { ...h, country: e.payload.country, country_name: e.payload.country_name }
              : h,
          ),
        );
      }));
      if (abortIfStale()) return;

      // Hostname (reverse DNS) hop'tan sonra gelir; yalnızca hostname alanını birleştirir (geo/RTT korunur).
      fns.push(await listen<HopHost>("traceroute:hop_host", (e) => {
        if (!isCurrent() || e.payload.run_id !== runId) return;
        setHops(prev =>
          prev.map(h => (h.hop === e.payload.hop ? { ...h, hostname: e.payload.hostname } : h)),
        );
      }));
      if (abortIfStale()) return;

      // done is emitted by the backend only AFTER GeoIP/hostname enrichment finished, so cleaning up here
      // no longer drops late country labels or hostnames.
      fns.push(await listen<{ run_id: number }>("traceroute:done", (e) => {
        if (!isCurrent() || e.payload?.run_id !== runId) return;
        setState("done");
        setNewHopIdx(null);
        cleanup();
        releaseOwned(runId);
      }));
      if (abortIfStale()) return;

      fns.push(await listen<{ run_id: number; message: string }>("traceroute:error", (e) => {
        if (!isCurrent() || e.payload.run_id !== runId) return;
        setErrorMsg(e.payload.message);
        setState("error");
        setNewHopIdx(null);
        cleanup();
        releaseOwned(runId);
      }));
      // Stop/Reset may have landed while the last registration was pending: never start the backend run then.
      if (abortIfStale()) return;

      await invoke("run_traceroute", { host: effectiveTarget, ipVersion, resolveHostnames, runId });
    } catch (e: unknown) {
      releaseOwned(runId);
      // A stopped/replaced run must not surface its late failure on the new idle/running state.
      if (!isCurrent()) return;
      setErrorMsg(String(e));
      setState("error");
      cleanup();
    }
  };

  const reset = () => {
    // Invalidate the active run first: pending listen() setup / late events of it become no-ops.
    const owner = runIdRef.current;
    runIdRef.current += 1;
    cleanup();
    releaseOwned(owner);
    // Stop/Reset cancels the backend trace (no more probes, no more events).
    invoke("stop_traceroute").catch(() => {});
    setState("idle");
    setHops([]);
    setNewHopIdx(null);
    setErrorMsg(null);
  };

  useLang();
  const isRunning = state === "running";
  const isDone = state === "done";

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
      <style>{`
        @keyframes hopSlideIn {
          from { opacity: 0; transform: translateY(-6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
      `}</style>

      {/* Kontrol paneli */}
      <div className="card" style={{ padding: "14px 18px", display: "flex", flexDirection: "column", gap: 10 }}>

        {/* Satır 1: Hedef | IPv toggle | Hostname Resolve | input/select geniş | Preset Seç */}
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600, whiteSpace: "nowrap" }}>{t("trace_target")}</span>

          {/* IPv4 / IPv6 / Auto toggle */}
          <div style={{ display: "flex", borderRadius: 7, overflow: "hidden", border: "1px solid var(--border)", flexShrink: 0 }}>
            {(["auto", "4", "6"] as IpVersion[]).map(v => {
              const isDisabled = isRunning
                || (v === "4" && presetIpSupport === "6")
                || (v === "6" && presetIpSupport === "4");
              return (
                <button key={v} onClick={() => setIpVersion(v)} disabled={isDisabled}
                  style={{
                    padding: "4px 10px", fontSize: 11, fontWeight: ipVersion === v ? 700 : 500,
                    background: ipVersion === v ? "rgba(99,102,241,0.2)" : "transparent",
                    border: "none",
                    color: isDisabled ? "var(--text-muted)" : ipVersion === v ? "var(--accent)" : "var(--text-dim)",
                    cursor: isDisabled ? "not-allowed" : "pointer",
                    opacity: isDisabled && ipVersion !== v ? 0.35 : 1,
                  }}>
                  {v === "auto" ? "Auto" : `IPv${v}`}
                </button>
              );
            })}
          </div>

          {/* Hostname resolve toggle */}
          <button
            onClick={() => setResolveHostnames(v => !v)} disabled={isRunning}
            style={{
              display: "flex", alignItems: "center", gap: 5, flexShrink: 0,
              padding: "4px 10px", borderRadius: 7, fontSize: 11,
              fontWeight: resolveHostnames ? 700 : 500,
              background: resolveHostnames ? "rgba(16,185,129,0.12)" : "rgba(255,255,255,0.04)",
              border: `1px solid ${resolveHostnames ? "rgba(16,185,129,0.3)" : "var(--border)"}`,
              color: resolveHostnames ? "var(--success)" : "var(--text-muted)",
              cursor: isRunning ? "not-allowed" : "pointer",
            }}>
            <span style={{
              display: "inline-flex", width: 24, height: 13, borderRadius: 7,
              background: resolveHostnames ? "rgba(16,185,129,0.5)" : "rgba(255,255,255,0.12)",
              alignItems: "center", padding: "0 2px",
            }}>
              <span style={{
                width: 9, height: 9, borderRadius: "50%", background: "#fff",
                transition: "transform 0.15s",
                transform: resolveHostnames ? "translateX(11px)" : "translateX(0)",
              }} />
            </span>
            {t("trace_resolve_hostnames")}
          </button>

          {/* Input / Select — flex: 1 ile genişliyor */}
          {useCustom ? (
            <input
              type="text" value={customTarget}
              onChange={e => setCustomTarget(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter" && !isRunning) runTrace(); }}
              placeholder={t("trace_placeholder")} disabled={isRunning}
              style={{
                flex: 1, minWidth: 0,
                background: "var(--bg-card)", border: "1px solid var(--border)",
                borderRadius: 8, color: "var(--text)", padding: "5px 10px",
                fontSize: 13, outline: "none",
              }}
            />
          ) : (
            <select
              value={target}
              onChange={e => handleTargetChange(e.target.value)}
              disabled={isRunning}
              style={{
                flex: 1, minWidth: 0,
                background: "var(--bg-card)", border: "1px solid var(--border)",
                borderRadius: 8, color: "var(--text)", padding: "5px 10px",
                fontSize: 13, cursor: "pointer",
              }}
            >
              {presets.map(p => (
                <option key={p.host} value={p.host}>{p.label} ({p.host})</option>
              ))}
            </select>
          )}

          {/* Preset Seç — sağa yapışık */}
          <button
            onClick={() => setUseCustom(v => !v)} disabled={isRunning}
            style={{
              flexShrink: 0,
              background: useCustom ? "rgba(99,102,241,0.15)" : "transparent",
              border: "1px solid var(--border)", borderRadius: 7,
              color: useCustom ? "var(--accent)" : "var(--text-muted)",
              padding: "5px 10px", fontSize: 12, cursor: "pointer", whiteSpace: "nowrap",
            }}
          >
            {useCustom ? t("trace_select_preset") : t("trace_manual")}
          </button>
        </div>

        {/* Satır 2: Waterfall/Tablo toggle (sol) | Temizle + Trace Başlat (sağ) */}
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          {/* Görünüm toggle — her zaman görünür */}
          <span style={{ fontSize: 13, fontWeight: 600 }}>{t("trace_style")}</span>
          <div style={{ display: "flex", borderRadius: 7, overflow: "hidden", border: "1px solid var(--border)" }}>
            {(["waterfall", "table"] as ViewMode[]).map(mode => (
              <button
                key={mode}
                onClick={() => setViewMode(mode)}
                style={{
                  padding: "4px 12px", fontSize: 11, fontWeight: viewMode === mode ? 700 : 500,
                  background: viewMode === mode ? "rgba(99,102,241,0.2)" : "transparent",
                  border: "none", color: viewMode === mode ? "var(--accent)" : "var(--text-muted)",
                  cursor: "pointer",
                }}
              >
                {mode === "waterfall" ? t("trace_view_waterfall") : t("trace_view_table")}
              </button>
            ))}
          </div>

          <div style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center" }}>
            {(state === "done" || state === "error") && (
              <button
                onClick={reset}
                style={{
                  background: "transparent", border: "1px solid var(--border)",
                  borderRadius: 8, color: "var(--text-muted)",
                  padding: "6px 14px", fontSize: 12, cursor: "pointer",
                }}
              >
                {t("trace_clear")}
              </button>
            )}
            <button
              onClick={isRunning ? reset : runTrace}
              disabled={!isRunning && (!effectiveTarget || guard.activeTest !== null)}
              style={{
                background: isRunning ? "rgba(239,68,68,0.1)" : "rgba(99,102,241,0.15)",
                border: `1px solid ${isRunning ? "rgba(239,68,68,0.3)" : "rgba(99,102,241,0.3)"}`,
                borderRadius: 8, color: isRunning ? "#f87171" : "var(--accent)",
                padding: "6px 16px", fontSize: 13, fontWeight: 600, cursor: "pointer",
              }}
            >
              {isRunning ? t("trace_stop") : t("trace_start")}
            </button>
          </div>
        </div>

        {effectiveTarget && (
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginLeft: 2 }}>
            <span className="mono">{effectiveTarget}</span>
          </div>
        )}
        <TestBusyHint ownId="traceroute" />
      </div>

      {/* Hata */}
      {state === "error" && errorMsg && (
        <div className="card" style={{
          padding: "12px 16px", background: "rgba(239,68,68,0.08)",
          borderColor: "rgba(239,68,68,0.3)", color: "#fca5a5", fontSize: 13,
        }}>
          <strong>{t("trace_error")}:</strong> {errorMsg}
        </div>
      )}

      {/* Ana panel */}
      <div className="card" style={{ padding: 0, overflow: "hidden" }}>
        {/* Başlık */}
        <div style={{
          display: "flex", alignItems: "center", gap: 10,
          padding: "12px 18px", borderBottom: "1px solid var(--border)",
          background: "rgba(10,10,15,0.4)",
        }}>
          <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
            {viewMode === "waterfall" ? t("trace_waterfall") : t("trace_hop_list")}
          </h3>
          {isDone && (
            <span style={{ marginLeft: "auto", fontSize: 11, color: "#4ade80" }}>✓ {t("trace_done")}</span>
          )}
          {state === "idle" && (
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--text-muted)" }}>{t("trace_waiting")}</span>
          )}
        </div>

        {/* Waterfall görünümü */}
        {viewMode === "waterfall" && (
          <>
            {hops.length === 0 && !isRunning ? (
              <div style={{ padding: "36px 0", textAlign: "center", color: "var(--text-muted)", fontSize: 13 }}>
                {t("trace_empty")}
              </div>
            ) : hops.length > 0 ? (
              <WaterfallView hops={hops} isDone={isDone} />
            ) : null}
            {isRunning && (
              <div style={{ padding: "8px 18px 12px", display: "flex", alignItems: "center", gap: 6 }}>
                <span style={{ fontSize: 11, color: "var(--accent)", animation: "pulse 1.2s ease infinite" }}>●</span>
                <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{hops.length + 1} {t("trace_waiting_row")}</span>
              </div>
            )}
          </>
        )}

        {/* Tablo görünümü */}
        {viewMode === "table" && (
          <table className="servers-table" style={{ padding: "0 8px" }}>
            <thead>
              <tr>
                <th style={{ paddingLeft: 18, width: 42 }}>#</th>
                <th style={{ width: 70 }}></th>
                <th style={{ width: 145 }}>{t("col_ip")}</th>
                <th>{t("trace_hostname")}</th>
                <th style={{ width: 130 }}>{t("trace_country")}</th>
                <th style={{ paddingRight: 18, textAlign: "right", width: 130 }}>{t("col_rtt")}</th>
              </tr>
            </thead>
            <tbody>
              {hops.map((h, idx) => (
                <HopRow
                  key={h.hop} hop={h} idx={idx} hops={hops}
                  isNew={h.hop === newHopIdx} isDone={isDone}
                />
              ))}
              {isRunning && <PulsingRow n={hops.length + 1} />}
              {!isRunning && hops.length === 0 && (
                <tr>
                  <td colSpan={6} style={{ textAlign: "center", padding: "36px 0", color: "var(--text-muted)", fontSize: 13 }}>
                    {t("trace_empty")}
                  </td>
                </tr>
              )}
              <tr ref={tableBottomRef} style={{ height: 0 }} />
            </tbody>
          </table>
        )}

        {/* Özet */}
        <TraceSummary hops={hops} state={state} />
      </div>
    </div>
  );
}

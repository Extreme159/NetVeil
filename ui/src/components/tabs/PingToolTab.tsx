import { useState, useRef, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";

// Ortak istatistik alanlari (hem reply hem done payload'inda var).
interface PingStats {
  target_ip: string;
  sent: number;
  received: number;
  loss_pct: number;
  min_ms: number;
  avg_ms: number;
  max_ms: number;
  jitter_ms: number;
  hostname?: string | null; // yalnizca done'da dolu
}

interface PingReply extends PingStats {
  seq: number;
  rtt_ms: number | null;
}

interface PingDone extends PingStats {
  hostname: string | null;
}

type PingState = "idle" | "running" | "done" | "error";
type IpVersion = "auto" | "4" | "6";

interface PresetTarget { label: string; host: string; ip_support?: "4" | "6" | "both"; }

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

// ── Ping Summary Panel ────────────────────────────────────────────────────────

function PingSummary({
  sent, received, loss_pct, min_ms, avg_ms, max_ms, jitter_ms,
  hostname, target_ip
}: PingStats) {
  const items = [
    { value: sent, label: t("ping_sent"), color: "var(--text)" },
    { value: received, label: t("ping_received"), color: "var(--success)" },
    ...(loss_pct > 0 ? [{ value: `${loss_pct.toFixed(1)}%`, label: t("ping_loss"), color: "#f87171" }] : []),
    { value: `${min_ms.toFixed(1)} ms`, label: t("ping_min"), color: rttColor(min_ms) },
    { value: `${avg_ms.toFixed(1)} ms`, label: t("ping_avg"), color: rttColor(avg_ms) },
    { value: `${max_ms.toFixed(1)} ms`, label: t("ping_max"), color: rttColor(max_ms) },
    { value: `${jitter_ms.toFixed(1)} ms`, label: t("ping_jitter"), color: "var(--text-dim)" },
  ];

  return (
    <div style={{
      display: "flex", gap: 20, flexWrap: "wrap",
      padding: "10px 18px",
      borderTop: "1px solid var(--border)",
      background: "rgba(255,255,255,0.02)",
    }}>
      {items.map((item, i) => (
        <div key={i} style={{ display: "flex", alignItems: "baseline", gap: 5 }}>
          <span style={{ fontFamily: "var(--mono)", fontSize: 14, fontWeight: 700, color: item.color }}>{item.value}</span>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{item.label}</span>
        </div>
      ))}
      {hostname && (
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 5 }}>
          <span style={{ fontSize: 11, color: "var(--text-muted)" }}>{t("ping_hostname")}:</span>
          <span style={{ fontSize: 12, color: "var(--text-dim)" }}>{hostname}</span>
        </div>
      )}
      {target_ip && (
        <div style={{ display: "flex", alignItems: "center", gap: 5 }}>
          <span className="mono" style={{ fontSize: 11, color: "var(--text-muted)" }}>{target_ip}</span>
        </div>
      )}
    </div>
  );
}

// ── Ana Component ─────────────────────────────────────────────────────────────

export function PingToolTab() {
  const [presets, setPresets] = useState<PresetTarget[]>(FALLBACK_PRESETS);
  const [target, setTarget] = useState("1.1.1.1");
  const [customTarget, setCustomTarget] = useState("");
  const [useCustom, setUseCustom] = useState(false);
  const [state, setState] = useState<PingState>("idle");
  const [replies, setReplies] = useState<PingReply[]>([]);
  const [stats, setStats] = useState<PingStats | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const [ipVersion, setIpVersion] = useState<IpVersion>("auto");
  const [count, setCount] = useState<number | null>(5); // null = continuous

  const unlistenRef = useRef<UnlistenFn[]>([]);

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
    fetch("https://netveil.net/traceroute-presets")
      .then(r => r.json())
      .then((data: PresetTarget[]) => { if (data.length > 0) setPresets(data); })
      .catch(() => {});
  }, []);

  useEffect(() => { loadPresets(); }, [loadPresets]);

  const cleanup = () => {
    unlistenRef.current.forEach(fn => fn());
    unlistenRef.current = [];
  };

  useEffect(() => { return cleanup; }, []);

  const runPing = async () => {
    if (!effectiveTarget) return;
    cleanup();
    setState("running");
    setReplies([]);
    setStats(null);
    setErrorMsg(null);

    const unReply = await listen<PingReply>("ping:reply", (e) => {
      const reply = e.payload;
      setReplies(prev => {
        const exists = prev.findIndex(r => r.seq === reply.seq);
        if (exists >= 0) {
          const next = [...prev];
          next[exists] = reply;
          return next;
        }
        return [...prev, reply];
      });
      setStats(reply);
    });

    const unDone = await listen<PingDone>("ping:done", (e) => {
      setState("done");
      setStats(e.payload);
      cleanup();
    });

    const unError = await listen<string>("ping:error", (e) => {
      setErrorMsg(e.payload);
      setState("error");
      cleanup();
    });

    unlistenRef.current = [unReply, unDone, unError];

    try {
      await invoke("run_ping_test", {
        host: effectiveTarget,
        ipVersion,
        count: count === 0 ? null : count,
        resolveHostname: true,
      });
    } catch (e: unknown) {
      setErrorMsg(String(e));
      setState("error");
      cleanup();
    }
  };

  const stopPing = async () => {
    cleanup();
    setState("idle");
    try {
      await invoke("stop_ping_test");
    } catch (e: unknown) {
      setErrorMsg(String(e));
      setState("error");
    }
  };

  const reset = () => {
    cleanup();
    setState("idle");
    setReplies([]);
    setStats(null);
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

        {/* Satır 1: Hedef | IPv toggle | Count | input/select */}
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <span style={{ fontSize: 13, fontWeight: 600, whiteSpace: "nowrap" }}>{t("ping_target")}</span>

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

          {/* Count selector */}
          <select
            value={count ?? ""}
            onChange={e => setCount(e.target.value === "" ? null : parseInt(e.target.value, 10))}
            disabled={isRunning}
            style={{
              flexShrink: 0,
              background: "var(--bg-card)", border: "1px solid var(--border)",
              borderRadius: 8, color: "var(--text)", padding: "5px 10px",
              fontSize: 13, cursor: "pointer",
            }}
          >
            <option value="5">5</option>
            <option value="10">10</option>
            <option value="20">20</option>
            <option value="50">50</option>
            <option value="100">100</option>
            <option value="">{t("ping_continuous")}</option>
          </select>

          {/* Input / Select — flex: 1 ile genişliyor */}
          {useCustom ? (
            <input
              type="text" value={customTarget}
              onChange={e => setCustomTarget(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter" && !isRunning) runPing(); }}
              placeholder={t("ping_placeholder")} disabled={isRunning}
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

        {/* Satır 2: Temizle + Ping Başlat/Durdur */}
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
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
              onClick={isRunning ? stopPing : runPing}
              disabled={!effectiveTarget && !isRunning}
              style={{
                background: isRunning ? "rgba(239,68,68,0.1)" : "rgba(99,102,241,0.15)",
                border: `1px solid ${isRunning ? "rgba(239,68,68,0.3)" : "rgba(99,102,241,0.3)"}`,
                borderRadius: 8, color: isRunning ? "#f87171" : "var(--accent)",
                padding: "6px 16px", fontSize: 13, fontWeight: 600, cursor: "pointer",
              }}
            >
              {isRunning ? t("ping_stop") : t("ping_run")}
            </button>
          </div>
        </div>

        {effectiveTarget && (
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginLeft: 2 }}>
            <span className="mono">{effectiveTarget}</span>
          </div>
        )}
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
            {t("ping_results")}
          </h3>
          {isRunning && (
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--accent)", display: "flex", alignItems: "center", gap: 5 }}>
              <span style={{ animation: "pulse 1s ease infinite" }}>●</span> {t("ping_running")}
            </span>
          )}
          {isDone && (
            <span style={{ marginLeft: "auto", fontSize: 11, color: "#4ade80" }}>✓ {t("trace_done")}</span>
          )}
          {state === "idle" && (
            <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--text-muted)" }}>{t("trace_waiting")}</span>
          )}
        </div>

        {/* Ping sonuçları tablosu */}
        <table className="servers-table" style={{ padding: "0 8px" }}>
          <thead>
            <tr>
              <th style={{ paddingLeft: 18, width: 50 }}>{t("ping_seq")}</th>
              <th style={{ width: 100 }}>{t("col_rtt")}</th>
              <th>{t("ping_status")}</th>
            </tr>
          </thead>
          <tbody>
            {replies.length > 0 ? replies.map((r, idx) => {
              const isTimeout = r.rtt_ms === null;
              return (
                <tr key={r.seq} style={{ animation: idx === replies.length - 1 && isRunning ? "hopSlideIn 0.2s ease" : undefined }}>
                  <td style={{ paddingLeft: 18 }}>
                    <span className="mono" style={{ fontSize: 12, color: "var(--text-muted)" }}>{r.seq}</span>
                  </td>
                  <td>
                    {isTimeout ? (
                      <span style={{ color: "#f87171", fontSize: 12 }}>{t("ping_timeout")}</span>
                    ) : (
                      <span className="mono" style={{ fontSize: 12, color: rttColor(r.rtt_ms!) }}>
                        {r.rtt_ms!.toFixed(1)} ms
                      </span>
                    )}
                  </td>
                  <td>
                    <span style={{ fontSize: 11, color: "var(--text-dim)" }}>
                      {isTimeout ? t("ping_status_timeout") : t("ping_status_ok")}
                    </span>
                  </td>
                </tr>
              );
            }) : (
              <tr>
                <td colSpan={3} style={{ textAlign: "center", padding: "36px 0", color: "var(--text-muted)", fontSize: 13 }}>
                  {t("ping_empty")}
                </td>
              </tr>
            )}
          </tbody>
        </table>

        {/* Özet */}
        {stats && <PingSummary {...stats} />}
      </div>
    </div>
  );
}
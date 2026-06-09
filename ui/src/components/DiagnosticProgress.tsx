import { useEffect, useState } from "react";
import { Icon, type IconKey } from "../lib/icons";
import type { TestPhase } from "../types";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";

interface Step {
  phase: TestPhase;
  labelKey: string;
  iconKey: IconKey;
}

const QUICK_STEPS: Step[] = [
  { phase: "running_nic",          labelKey: "diag_step_nic",         iconKey: "Server" },
  { phase: "running_wifi",         labelKey: "diag_step_wifi",        iconKey: "Wifi"   },
  { phase: "running_ping",         labelKey: "diag_step_ping",        iconKey: "Ping"   },
  { phase: "running_speedtest",    labelKey: "diag_step_speedtest",   iconKey: "Speed"  },
  { phase: "running_bufferbloat",  labelKey: "diag_step_bufferbloat", iconKey: "Buffer" },
  { phase: "running_ipv6",         labelKey: "diag_step_ipv6",        iconKey: "Globe"  },
  { phase: "running_mtu",          labelKey: "diag_step_mtu",         iconKey: "Mtu"    },
  { phase: "running_dns_benchmark",labelKey: "diag_step_dns",         iconKey: "Dns"    },
];

const FULL_STEPS: Step[] = [
  { phase: "running_nic",              labelKey: "diag_step_nic",         iconKey: "Server"  },
  { phase: "running_wifi",             labelKey: "diag_step_wifi",        iconKey: "Wifi"    },
  { phase: "running_ping",             labelKey: "diag_step_ping",        iconKey: "Ping"    },
  { phase: "running_speedtest",        labelKey: "diag_step_speedtest",   iconKey: "Speed"        },
  { phase: "running_bufferbloat",      labelKey: "diag_step_bufferbloat", iconKey: "Buffer"       },
  { phase: "running_throttle",         labelKey: "diag_step_throttle",    iconKey: "Throttle"     },
  { phase: "running_isp_intervention", labelKey: "diag_step_isp",         iconKey: "Intervention" },
  { phase: "running_ipv6",             labelKey: "diag_step_ipv6",        iconKey: "Globe"   },
  { phase: "running_cgnat",            labelKey: "diag_step_cgnat",       iconKey: "Cgnat"   },
  { phase: "running_mtu",              labelKey: "diag_step_mtu",         iconKey: "Mtu"     },
  { phase: "running_voip",             labelKey: "diag_step_voip",        iconKey: "VoipQuality" },
  { phase: "running_dns_benchmark",    labelKey: "diag_step_dns",         iconKey: "Dns"     },
];

export type TestMode = "quick" | "full";

type Status = "pending" | "active" | "done";

function statusOf(stepIndex: number, currentIndex: number, phase: TestPhase): Status {
  if (phase === "done") return "done";
  // running_smart: scoring phase after all network tests — treat all steps as done
  if (phase === "running_smart") return "done";
  if (currentIndex === -1) return "done";
  if (stepIndex < currentIndex) return "done";
  if (stepIndex === currentIndex) return "active";
  return "pending";
}

function useStepTimer(phase: TestPhase): number {
  const [ms, setMs] = useState(0);
  useEffect(() => {
    setMs(0);
    if (!phase.startsWith("running_")) return;
    const t0 = Date.now();
    const id = setInterval(() => setMs(Date.now() - t0), 100);
    return () => clearInterval(id);
  }, [phase]);
  return ms;
}

interface Props {
  phase: TestPhase;
  mode: TestMode;

  totalElapsedMs?: number;
}

export function DiagnosticProgress({ phase, mode, totalElapsedMs = 0 }: Props) {
  useLang();
  const STEPS = mode === "quick" ? QUICK_STEPS : FULL_STEPS;
  const currentIndex    = STEPS.findIndex(s => s.phase === phase);
  const isDone          = phase === "done";
  // running_smart = scoring phase after last network test — show 100% / all done
  const isScoring       = phase === "running_smart";
  const completedSteps  = (isDone || isScoring) ? STEPS.length : Math.max(0, currentIndex);
  const overallPct      = (completedSteps / STEPS.length) * 100;
  const totalElapsedSec = (totalElapsedMs / 1000).toFixed(1);
  const stepElapsedMs   = useStepTimer(phase);
  const stepElapsedSec  = (stepElapsedMs / 1000).toFixed(1);

  const modeLabel = mode === "quick" ? t("diag_gaming_test") : t("diag_full_test");

  return (
    <div
      className="card"
      style={{
        padding: "16px 18px",
        marginBottom: 14,
        background: "radial-gradient(ellipse at 0% 50%, rgba(99,102,241,0.10), transparent 55%), var(--bg-card-soft)",
        borderColor: "rgba(99,102,241,0.25)",
      }}
    >
      {/* Header */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 12 }}>
        <div style={{
          width: 32, height: 32, borderRadius: 9,
          display: "grid", placeItems: "center",
          background: "linear-gradient(135deg, var(--accent), var(--accent-2))",
          color: "white",
          boxShadow: "0 8px 22px -6px rgba(99,102,241,0.55)",
          flexShrink: 0,
        }}>
          {isDone
            ? <Icon.Bolt    style={{ width: 16, height: 16 }} />
            : <Icon.Refresh style={{ width: 16, height: 16, animation: "spin 1s linear infinite" }} />}
        </div>
        <div style={{ flex: 1 }}>
          <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600 }}>
            {isDone
              ? `${modeLabel} ${t("diag_completed")}`
              : `${modeLabel} ${t("diag_running_label")}`}
          </h3>
          <p style={{ margin: "2px 0 0", fontSize: 11, color: "var(--text-muted)" }}>
            {(isDone || isScoring)
              ? `${STEPS.length}/${STEPS.length} ${t("diag_steps")} · ${totalElapsedSec}s`
              : `${Math.max(0, currentIndex)}/${STEPS.length} ${t("diag_steps")} · ${totalElapsedSec}s`}
          </p>
        </div>
        <div className="mono" style={{ fontSize: 20, fontWeight: 700, color: "var(--accent)", filter: "drop-shadow(0 0 12px rgba(99,102,241,0.4))" }}>
          {Math.round(overallPct)}%
        </div>
      </div>

      {/* Genel progress bar */}
      <div style={{ height: 4, borderRadius: 999, background: "rgba(255,255,255,0.04)", overflow: "hidden", border: "1px solid rgba(255,255,255,0.03)", marginBottom: 12 }}>
        <div style={{
          width: `${overallPct}%`, height: "100%", borderRadius: 999,
          background: "linear-gradient(90deg, var(--accent), var(--accent-2))",
          boxShadow: "0 0 14px rgba(99,102,241,0.6)",
          transition: "width 600ms cubic-bezier(0.22,1,0.36,1)",
        }} />
      </div>

      {/* Adımlar */}
      <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
        {STEPS.map((step, i) => {
          const status     = statusOf(i, currentIndex, phase);
          const Ico        = Icon[step.iconKey];
          const isActive   = status === "active";
          const isDoneStep = status === "done";

          const iconColor  = isDoneStep ? "var(--success)" : isActive ? "#c7d2fe" : "var(--text-muted)";
          const iconBg     = isDoneStep ? "rgba(34,197,94,0.14)"  : isActive ? "rgba(99,102,241,0.18)"  : "rgba(255,255,255,0.03)";
          const iconBorder = isDoneStep ? "rgba(34,197,94,0.4)"   : isActive ? "rgba(99,102,241,0.45)"  : "var(--border)";

          return (
            <div key={step.phase} style={{
              display: "flex", alignItems: "center", gap: 10,
              padding: "7px 10px", borderRadius: 8,
              background: isActive ? "rgba(99,102,241,0.06)" : "transparent",
              border: `1px solid ${isActive ? "rgba(99,102,241,0.2)" : "transparent"}`,
              transition: "all 300ms ease",
              opacity: status === "pending" ? 0.5 : 1,
            }}>
              <div style={{
                width: 26, height: 26, borderRadius: 7,
                display: "grid", placeItems: "center",
                background: iconBg, border: `1px solid ${iconBorder}`,
                color: iconColor, position: "relative", flexShrink: 0,
                transition: "all 300ms ease",
                boxShadow: isActive ? "0 0 12px rgba(99,102,241,0.35)" : "none",
              }}>
                {isDoneStep ? (
                  <svg viewBox="0 0 24 24" width="12" height="12" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M5 13l4 4L19 7" />
                  </svg>
                ) : (
                  <Ico style={{ width: 12, height: 12 }} />
                )}
                {isActive && (
                  <span style={{ position: "absolute", inset: -3, borderRadius: 10, border: "1.5px solid rgba(99,102,241,0.55)", animation: "pulse-ring 1.4s ease-in-out infinite" }} />
                )}
              </div>

              <div style={{ flex: 1, minWidth: 0 }}>
                <div style={{ fontSize: 12, fontWeight: 500, color: isActive ? "var(--text)" : isDoneStep ? "var(--text-dim)" : "var(--text-muted)" }}>
                  {t(step.labelKey as Parameters<typeof t>[0])}
                </div>
              </div>

              <div className="mono" style={{
                fontSize: 10, letterSpacing: "0.1em", textTransform: "uppercase",
                padding: "2px 8px", borderRadius: 999, whiteSpace: "nowrap",
                background: isDoneStep ? "rgba(34,197,94,0.1)" : isActive ? "rgba(99,102,241,0.15)" : "rgba(255,255,255,0.03)",
                color:      isDoneStep ? "#86efac"             : isActive ? "#c7d2fe"               : "var(--text-muted)",
                border: `1px solid ${isDoneStep ? "rgba(34,197,94,0.3)" : isActive ? "rgba(99,102,241,0.3)" : "var(--border)"}`,
              }}>
                {isDoneStep
                  ? t("diag_done")
                  : isActive ? `${stepElapsedSec}s` : t("diag_waiting")}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

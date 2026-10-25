import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Icon } from "../../lib/icons";
import { TestIntro } from "../TestIntro";
import { GradeCard } from "../GradeCard";
import type {
  Ipv6TestResult,
  Ipv6Progress,
  Ipv6ConnectivityInfo,
  Ipv6QualityScore,
  TestPhase,
  ModuleVerdict,
} from "../../types";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";

interface Props {
  result: Ipv6TestResult | null;
  phase: TestPhase;
  onRun: () => void;
}

// IPv6 kelime-bazlı durum rengi (Full/Tunnel/None gibi — harf grade DEĞİL).
// Harf notu (A+/A/B...) ayrıca gradeFromScore(quality.score) ile GradeBox'ta gösterilir.
function gradeColor(grade: string): string {
  switch (grade) {
    case "Excellent": case "Full":    return "#22c55e";
    case "Good":      case "Partial": return "#86efac";
    case "Tunnel":    case "Poor":    return "#f59e0b";
    case "None":                      return "#ef4444";
    default:                          return "var(--text-muted)";
  }
}

function gradeLabel(grade: string): string {
  const map: Record<string, string> = {
    Full:      t("ipv6_grade_full"),
    Partial:   t("ipv6_grade_partial"),
    Tunnel:    t("ipv6_grade_tunnel"),
    None:      t("ipv6_grade_none"),
    Excellent: t("ipv6_grade_excellent"),
    Good:      t("ipv6_grade_good"),
    Poor:      t("ipv6_grade_poor"),
  };
  return map[grade] ?? grade;
}

function addrTypeLabel(type: string): string {
  const map: Record<string, string> = {
    Native: "Native 🟢", Teredo: "Teredo 🟡", "6to4": "6to4 🟡",
    Tunnel: `${t("ipv6_type_tunnel")} (6in4) 🟡`, ISATAP: "ISATAP 🟡",
    LinkLocal: "Link-Local", UniqueLocal: "Unique-Local (ULA)", Unknown: t("ipv6_type_unknown"),
  };
  return map[type] ?? type;
}

function ConnectivityCard({ info }: { info: Ipv6ConnectivityInfo }) {
  useLang();
  const gc = gradeColor(info.connectivity_grade);
  const gl = gradeLabel(info.connectivity_grade);

  return (
    <div className="card">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 16 }}>
        <div style={{ fontWeight: 700, fontSize: 14 }}>{t("ipv6_connectivity_card_title")}</div>
        <span style={{ background: `${gc}22`, color: gc, border: `1px solid ${gc}55`, padding: "3px 12px", borderRadius: 6, fontWeight: 700, fontSize: 12 }}>{gl}</span>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "10px 24px", marginBottom: 16 }}>
        {[
          [t("ipv6_native"),  info.has_native_ipv6 ? t("ipv6_native_yes") : t("ipv6_native_no"),     info.has_native_ipv6 ? "#22c55e" : "#ef4444"],
          [t("ipv6_dns_aaaa"), info.dns_ipv6_works  ? t("ipv6_dns_works") : t("ipv6_dns_broken"),    info.dns_ipv6_works  ? "#22c55e" : "#ef4444"],
          [t("ipv6_transport"), info.ipv6_transport_works === undefined ? "\u2014" : info.ipv6_transport_works ? t("ipv6_transport_yes") : t("ipv6_transport_no"), info.ipv6_transport_works ? "#22c55e" : info.ipv6_transport_works === false ? "#ef4444" : "var(--text-muted)"],
          [t("ipv6_tunnel"),  info.has_tunnel_only  ? t("ipv6_tunnel_yes") : t("ipv6_tunnel_no"),    info.has_tunnel_only ? "#f59e0b" : "var(--text-muted)"],
          [t("ipv6_wan"),     info.wan_ipv6 ?? "—",                                                   info.wan_ipv6 ? "#86efac" : "var(--text-muted)"],
          [t("ipv6_gateway"), info.gateway_v6 ?? "—",                                                 info.gateway_v6 ? "#86efac" : "var(--text-muted)"],
          [t("ipv6_isp"),     info.isp_ipv6 ?? "—",                                                   "var(--text-muted)"],
        ].map(([label, value, color]) => (
          <div key={label as string}>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 2 }}>{label as string}</div>
            <div style={{ fontSize: 13, fontWeight: 600, color: color as string, fontFamily: (label as string).includes("IPv6") || (label as string).includes("Gateway") ? "JetBrains Mono, monospace" : undefined }}>
              {value as string}
            </div>
          </div>
        ))}
      </div>

      {info.addresses.filter(a => a.scope !== "Loopback").length > 0 && (
        <div>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 6, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em" }}>
            {t("ipv6_detected_addresses")}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 5 }}>
            {info.addresses.filter(a => a.scope !== "Loopback").map((a, i) => (
              <div key={i} style={{ background: "rgba(255,255,255,0.04)", borderRadius: 6, padding: "6px 10px", display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                <span style={{ fontFamily: "JetBrains Mono, monospace", fontSize: 12, color: "#e2e8f0", wordBreak: "break-all" }}>
                  {a.address}/{a.prefix_len}
                </span>
                <div style={{ display: "flex", gap: 6, flexShrink: 0 }}>
                  <span style={{ fontSize: 11, color: "var(--text-muted)", background: "rgba(255,255,255,0.06)", padding: "1px 6px", borderRadius: 4 }}>
                    {addrTypeLabel(a.address_type)}
                  </span>
                  {a.is_temporary && (
                    <span style={{ fontSize: 11, color: "#94a3b8", background: "rgba(148,163,184,0.1)", padding: "1px 6px", borderRadius: 4 }}>
                      {t("ipv6_temporary")}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function QualityCard({ q }: { q: Ipv6QualityScore }) {
  useLang();
  return (
    <div className="card">
      <div style={{ fontWeight: 700, fontSize: 14, marginBottom: 14 }}>{t("ipv6_quality_title")}</div>
      {q.recommendations.length > 0 && (
        <div>
          <div style={{ fontSize: 11, color: "var(--text-muted)", fontWeight: 600, marginBottom: 8, textTransform: "uppercase", letterSpacing: "0.05em" }}>
            {t("ipv6_quality_assessment_label")}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
            {q.recommendations.map((r, i) => {
              const text = t(r as Parameters<typeof t>[0]);
              return (
                <div key={i} style={{ display: "flex", gap: 8, alignItems: "flex-start" }}>
                  <span style={{ color: "#3b82f6", fontSize: 14, lineHeight: "18px", flexShrink: 0 }}>›</span>
                  <span style={{ fontSize: 13, color: "#cbd5e1", lineHeight: 1.5 }}>{text}</span>
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

export function Ipv6Tab({ result, phase, onRun }: Props) {
  useLang();
  const isRunning = phase === "running_ipv6";
  const [activePhase, setActivePhase] = useState<string | null>(null);
  const [message, setMessage] = useState("");
  const [verdict, setVerdict] = useState<ModuleVerdict | null>(null);

  useEffect(() => {
    if (!isRunning) { setActivePhase(null); return; }
    const p = listen<Ipv6Progress>("ipv6:progress", (ev) => {
      if (ev.payload.status === "running") {
        setActivePhase(ev.payload.phase);
        setMessage(ev.payload.message);
      } else {
        setActivePhase(null);
        setMessage(ev.payload.message);
      }
    });
    return () => { void p.then(fn => fn()); };
  }, [isRunning]);

  useEffect(() => {
    if (!result) { setVerdict(null); return; }
    invoke<ModuleVerdict>("run_module_verdict", {
      module: "ipv6",
      input: {
        nic: {}, wifi: {}, ping_results: [], bufferbloat: {},
        speed_download: null, speed_upload: null,
        throttle: null, isp_intervention: null,
        ipv6: result,
        cgnat: null, mtu: null, voip: null, wifi_scan: null,
        dns_benchmark: null, twitch: null,
      },
    }).then(setVerdict).catch(() => setVerdict(null));
  }, [result]);


  function phaseMsg(ph: string | null): string {
    if (!ph) return message;
    const map: Record<string, string> = {
      connectivity: t("ipv6_phase_connectivity"),
      scoring:      t("ipv6_phase_scoring"),
    };
    return map[ph] ?? message;
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>

      {/* Çalışıyor banner */}
      {isRunning && (
        <div className="card" style={{ background: "rgba(59,130,246,0.08)", borderColor: "rgba(59,130,246,0.3)", padding: "14px 18px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Icon.Search style={{ width: 18, height: 18, color: "var(--text-muted)" }} />
            <div>
              <div style={{ fontWeight: 600, fontSize: 14, color: "#93c5fd" }}>{t("ipv6_running_title")}</div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>{phaseMsg(activePhase)}</div>
            </div>
          </div>
        </div>
      )}

      {/* Smart Grade */}
      {verdict && verdict.available && (
        <GradeCard
          grade={verdict.grade}
          comment={t(verdict.detail_key as Parameters<typeof t>[0])}
        />
      )}

      {result ? (
        <>
          <ConnectivityCard info={result.connectivity} />
          <QualityCard q={result.quality} />
        </>
      ) : !isRunning ? (
        <TestIntro
          guarded
          iconKey="Globe"
          title={t("tab_ipv6_title")}
          description={t("ipv6_intro_desc")}
          buttonLabel={t("btn_run_ipv6")}
          onRun={onRun}
          warning={t("test_warning_external_traffic")}
        />
      ) : null}
    </div>
  );
}

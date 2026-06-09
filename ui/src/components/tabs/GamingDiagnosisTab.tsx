import type { FpsDiagnosis, Synthesis, ModuleVerdict, TestPhase } from "../../types";
import { Icon } from "../../lib/icons";
import { gradeColor } from "../../lib/helpers";
import { GradeBox } from "../GradeBox";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { DiagnosticProgress } from "../DiagnosticProgress";

// Dinamik i18n anahtari icin guvenli cast
const tk = (key: string) => t(key as Parameters<typeof t>[0]);

const INSIGHT_COLOR: Record<string, string> = {
  positive: "#34d399", neutral: "#fde047", negative: "#fca5a5",
};

interface Props {
  fpsDiagnosis: FpsDiagnosis | null;
  phase: TestPhase;
  onRun: () => void;
  testMode: "quick" | "full";
  totalElapsedMs: number;
}

function SynthesisCard({ s }: { s: Synthesis }) {
  const color = gradeColor(s.grade);
  return (
    <div className="card">
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        <GradeBox grade={s.grade} size="lg" glow />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 4 }}>
            <span style={{ fontFamily: "var(--mono)", fontSize: 14, fontWeight: 700, color }}>{tk("gaming_score_label")}: {s.score}/100</span>
          </div>
          <p style={{ margin: 0, fontSize: 13, color: "var(--text-dim)", lineHeight: 1.55 }}>
            {tk(s.headline_key)}
          </p>
        </div>
      </div>
      {s.insights.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 16 }}>
          {s.insights.map((it, i) => {
            const c = INSIGHT_COLOR[it.kind] ?? "var(--text-muted)";
            return (
              <div key={i} style={{ display: "flex", gap: 9, alignItems: "flex-start" }}>
                <span style={{ flexShrink: 0, width: 16, height: 16, borderRadius: 4, marginTop: 1, display: "grid", placeItems: "center", fontSize: 11, fontWeight: 700, color: "#0a0a0f", background: c }}>
                  {it.kind === "positive" ? "+" : it.kind === "neutral" ? "~" : "!"}
                </span>
                <span style={{ fontSize: 12.5, color: "var(--text-dim)", lineHeight: 1.5 }}>{tk(it.text_key)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ModuleRow({ m }: { m: ModuleVerdict }) {
  const name = tk(`diag_mod_${m.module}`);
  if (!m.available) return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", borderRadius: 10, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)", opacity: 0.6 }}>
      <GradeBox size="md" />
      <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-muted)" }}>{name}</span>
      <span style={{ flex: 1, fontSize: 12, color: "var(--text-muted)" }}>{tk(m.detail_key)}</span>
    </div>
  );
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", borderRadius: 10, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)" }}>
      <GradeBox grade={m.grade} size="md" />
      <span style={{ flex: 1, fontSize: 13, fontWeight: 600, color: "var(--text)" }}>{name}</span>
      <span style={{ fontSize: 12, color: "var(--text-dim)" }}>{tk(m.detail_key)}</span>
    </div>
  );
}

export function GamingDiagnosisTab({ fpsDiagnosis, phase, onRun, testMode, totalElapsedMs }: Props) {
  useLang();
  const isBusy = phase.startsWith("running_");

  if (!fpsDiagnosis) {
    if (isBusy) {
      return <DiagnosticProgress phase={phase} mode={testMode} totalElapsedMs={totalElapsedMs} />;
    }
    return (
      <div className="card" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 18, padding: "56px 32px", textAlign: "center" }}>
        <div style={{ opacity: 0.45, color: "var(--text-muted)" }}>
          <Icon.Crosshair style={{ width: 44, height: 44 }} />
        </div>
        <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 380, lineHeight: 1.6, whiteSpace: "pre-line" }}>
          {t("gaming_scan_empty")}
        </div>
        <div style={{ fontSize: 12, color: "#ef4444", maxWidth: 420, lineHeight: 1.6, padding: "10px 14px", borderRadius: 8, background: "rgba(239,68,68,0.08)", border: "1px solid rgba(239,68,68,0.25)" }}>
          {t("test_warning_external_traffic")}
        </div>
        <button className="btn" onClick={onRun} disabled={isBusy} type="button">
          {isBusy ? <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} /> : <Icon.Play />}
          {isBusy ? t("btn_running") : t("gaming_scan_run_btn")}
        </button>
      </div>
    );
  }

  // Sonuç var → oyun skoru (rekabetçi) + modül tablosu
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <SynthesisCard s={fpsDiagnosis.fps_synthesis} />
      <div className="card">
        <div className="card-head">
          <div className="ico-wrap"><Icon.Crosshair /></div>
          <h3>{t("diag_modules_title")}</h3>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 4 }}>
          {fpsDiagnosis.modules.map(m => <ModuleRow key={m.module} m={m} />)}
        </div>
      </div>
    </div>
  );
}

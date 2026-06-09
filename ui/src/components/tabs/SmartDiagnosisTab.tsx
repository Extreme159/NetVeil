import { useState } from "react";
import type { TabProps } from "./types";
import type { ModuleVerdict, Synthesis } from "../../types";
import { Icon } from "../../lib/icons";
import { ProActivateBar } from "../TestIntro";
import { gradeColor } from "../../lib/helpers";
import { GradeBox } from "../GradeBox";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { DiagnosticProgress } from "../DiagnosticProgress";

// Dinamik i18n anahtari icin guvenli cast (bilinmeyen key'de key string'i doner).
const tk = (key: string) => t(key as Parameters<typeof t>[0]);

const INSIGHT_COLOR: Record<string, string> = {
  positive: "#34d399", neutral: "#fde047", negative: "#fca5a5",
};

/** Tiklanabilir modul satiri (accordion): ad + grade; tiklayinca engine yorumu acilir. */
function ModuleRow({ m, defaultOpen = false }: { m: ModuleVerdict; defaultOpen?: boolean }) {
  const [open, setOpen] = useState(defaultOpen);
  const name = tk(`diag_mod_${m.module}`);

  if (!m.available) {
    return (
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", borderRadius: 10, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)", opacity: 0.6 }}>
        {/* Gri "-" kutusu: test edildi ama puanlanamadi (available modullerle gorsel tutarlilik) */}
        <GradeBox size="md" />
        <span style={{ flex: "0 0 auto", fontSize: 13, fontWeight: 600, color: "var(--text-muted)" }}>{name}</span>
        <span style={{ flex: 1, fontSize: 12, color: "var(--text-muted)", minWidth: 0 }}>{tk(m.detail_key)}</span>
      </div>
    );
  }
  return (
    <div style={{ borderRadius: 10, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)", overflow: "hidden" }}>
      {/* Tiklanabilir baslik satiri */}
      <div
        onClick={() => setOpen(o => !o)}
        style={{ display: "flex", alignItems: "center", gap: 12, padding: "11px 14px", cursor: "pointer" }}
      >
        <GradeBox grade={m.grade} size="md" />
        <span style={{ flex: 1, fontSize: 13, fontWeight: 600, color: "var(--text)" }}>{name}</span>
        {/* chevron */}
        <span style={{
          fontSize: 11, color: "var(--text-muted)", transition: "transform 160ms ease",
          transform: open ? "rotate(90deg)" : "rotate(0deg)",
        }}>▶</span>
      </div>

      {/* Genisleyen engine yorumu */}
      {open && (
        <div style={{ padding: "0 14px 14px 14px", borderTop: "1px solid var(--border)" }}>
          <div style={{ fontSize: 12.5, color: "var(--text-dim)", lineHeight: 1.6, margin: "12px 0" }}>
            {tk(m.detail_key)}
          </div>
          {m.insights && m.insights.length > 0 && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {m.insights.map((it, i) => {
                const c = INSIGHT_COLOR[it.kind] ?? "var(--text-muted)";
                return (
                  <div key={i} style={{ display: "flex", gap: 9, alignItems: "flex-start" }}>
                    <span style={{
                      flexShrink: 0, width: 16, height: 16, borderRadius: 4, marginTop: 1,
                      display: "grid", placeItems: "center", fontSize: 11, fontWeight: 700,
                      color: "#0a0a0f", background: c,
                    }}>{it.kind === "positive" ? "+" : it.kind === "neutral" ? "~" : "!"}</span>
                    <span style={{ fontSize: 12.5, color: "var(--text-dim)", lineHeight: 1.55 }}>{tk(it.text_key)}</span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Akilli Yorumlama (Smart Diagnosis) — lokal, deterministik tani motoru.
 * Backend smart.rs::run_smart_diagnosis (LLM yok) sonucunu gosterir.
 * Faz 3'te modul-modul yorumlama ile zenginlestirilecek.
 */
export function SmartDiagnosisTab({ results, phase, runAllTests, testMode, totalElapsedMs, isLocked, onActivate }: TabProps) {
  useLang();
  const smart = results.smartFull;
  // Test akisi suruyor mu (nic/wifi/ping/speed/.../smart herhangi biri).
  const isBusy = phase.startsWith("running_");

  // Test akisi surerken: Gosterge Paneli'ndeki adim-adim ilerleme listesi
  // (tamamlandi / suruyor / bekliyor). Tam tanilama -> 'full' adim seti.
  if (isBusy && !smart) {
    return (
      <DiagnosticProgress
        phase={phase}
        mode={testMode}
        totalElapsedMs={totalElapsedMs}
      />
    );
  }

  if (!smart) {
    return (
      <div className="card" style={{
        display: "flex", flexDirection: "column", alignItems: "center",
        justifyContent: "center", gap: 18, padding: "56px 32px", textAlign: "center",
      }}>
        <div style={{ opacity: 0.45, color: "var(--text-muted)" }}>
          <Icon.Brain style={{ width: 44, height: 44 }} />
        </div>
        <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 380, lineHeight: 1.6, whiteSpace: "pre-line" }}>{t("diag_empty")}</div>
        <div style={{ fontSize: 12, color: "#ef4444", maxWidth: 420, lineHeight: 1.6, padding: "10px 14px", borderRadius: 8, background: "rgba(239,68,68,0.08)", border: "1px solid rgba(239,68,68,0.25)" }}>
          {t("test_warning_external_traffic")}
        </div>
        <button className="btn" onClick={runAllTests} disabled={isBusy || isLocked} type="button" style={isLocked ? { opacity: 0.4, cursor: "not-allowed" } : undefined}>
          {isBusy
            ? <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} />
            : <Icon.Play />}
          {isBusy ? t("btn_running") : t("diag_run_btn")}
        </button>
        {isLocked && <ProActivateBar onActivate={onActivate} />}
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      {/* Buyuk resim sentezi: Genel Skor — grade karti EN USTTE (diger tablarla tutarli) */}
      {smart.general_synthesis && (
        <SynthesisCard title={t("synth_general_title")} s={smart.general_synthesis} />
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 16 }}>
        {smart.issues.length > 0 && (
          <div className="card smart-card">
            <div className="card-head">
              <div className="ico-wrap" style={{ background: "rgba(239,68,68,0.14)", borderColor: "rgba(239,68,68,0.3)", color: "#fca5a5" }}>
                <Icon.Alert />
              </div>
              <h3>{t("diag_issues")}</h3>
              <span className="head-sub">{smart.issues.length}</span>
            </div>
            <div className="smart-section issues" style={{ marginTop: 0 }}>
              <ul className="smart-list">
                {smart.issues.map((issue, i) => (
                  <li key={i}>{issue}</li>
                ))}
              </ul>
            </div>
          </div>
        )}

        {smart.suggestions.length > 0 && (
          <div className="card smart-card">
            <div className="card-head">
              <div className="ico-wrap" style={{ background: "rgba(34,197,94,0.14)", borderColor: "rgba(34,197,94,0.3)", color: "#86efac" }}>
                <Icon.Idea />
              </div>
              <h3>{t("diag_suggestions")}</h3>
              <span className="head-sub">{smart.suggestions.length}</span>
            </div>
            <div className="smart-section suggest" style={{ marginTop: 0 }}>
              <ul className="smart-list">
                {smart.suggestions.map((s, i) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
            </div>
          </div>
        )}
      </div>

      {/* Modul-modul yorumlar */}
      <div className="card">
        <div className="card-head">
          <div className="ico-wrap"><Icon.Brain /></div>
          <h3>{t("diag_modules_title")}</h3>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 4 }}>
          {smart.modules.map((m, i) => <ModuleRow key={m.module} m={m} defaultOpen={i === 0} />)}
        </div>
      </div>
    </div>
  );
}

/** Buyuk resim sentez karti: buyuk grade + headline + pozitif/notr/negatif maddeler. */
function SynthesisCard({ title, s }: { title: string; s: Synthesis }) {
  const color = gradeColor(s.grade);
  return (
    <div className="card">
      <div style={{ display: "flex", alignItems: "center", gap: 18 }}>
        {/* Buyuk grade rozeti (ortak GradeBox, glow'lu) */}
        <GradeBox grade={s.grade} size="lg" glow />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "baseline", gap: 10, marginBottom: 4 }}>
            <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700, color: "var(--text)" }}>{title}</h3>
            <span style={{ fontFamily: "var(--mono)", fontSize: 13, fontWeight: 600, color }}>{s.score}/100</span>
          </div>
          <p style={{ margin: 0, fontSize: 13, color: "var(--text-dim)", lineHeight: 1.55 }}>
            {tk(s.headline_key)}
          </p>
        </div>
      </div>

      {/* Sentez icgoruleri */}
      {s.insights.length > 0 && (
        <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 16 }}>
          {s.insights.map((it, i) => {
            const c = INSIGHT_COLOR[it.kind] ?? "var(--text-muted)";
            return (
              <div key={i} style={{ display: "flex", gap: 9, alignItems: "flex-start" }}>
                <span style={{
                  flexShrink: 0, width: 16, height: 16, borderRadius: 4, marginTop: 1,
                  display: "grid", placeItems: "center", fontSize: 11, fontWeight: 700,
                  color: "#0a0a0f", background: c,
                }}>{it.kind === "positive" ? "+" : it.kind === "neutral" ? "~" : "!"}</span>
                <span style={{ fontSize: 12.5, color: "var(--text-dim)", lineHeight: 1.5 }}>{tk(it.text_key)}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

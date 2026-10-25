import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { MtuResult, ModuleVerdict } from "../../types";
import type { TestResults } from "../../App";
import { Icon } from "../../lib/icons";
import { GradeCard } from "../GradeCard";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";

const tk = (key: string) => t(key as Parameters<typeof t>[0]);

// MTU degerine gore renk (Smart esikleriyle tutarli).
function mtuColor(mtu: number | null): string {
  if (mtu === null) return "var(--text-muted)";
  if (mtu >= 1492) return "#34d399";
  if (mtu >= 1400) return "#86efac";
  if (mtu >= 1280) return "#fdba74";
  return "#fca5a5";
}

/**
 * MTU / MSS Kesfi (Lite) — Path MTU Discovery (DF-bit ICMP binary search).
 * Backend: mtu::run_mtu_test. Self-contained.
 */
/**
 * @param results App'in merkezi sonuçları — Smart Diagnosis (runAllTests) MTU testini
 *   çalıştırıp sonucu `results.mtu`'ya yazar. Tab açılışında kendi taze sonucu yoksa
 *   bu prop'tan gelen sonuç gösterilir (intro yerine).
 */
export function MtuTab({ results }: { results: TestResults }) {
  useLang();
  const [localResult, setLocalResult] = useState<MtuResult | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [verdict, setVerdict] = useState<ModuleVerdict | null>(null);


  // Gösterilecek sonuç: tab'den manuel çalıştırılan taze sonuç öncelikli; yoksa Smart'tan gelen.
  const result = localResult ?? results.mtu;

  // Smart verdict: run_module_verdict whenever result changes.
  useEffect(() => {
    if (!result) { setVerdict(null); return; }
    invoke<ModuleVerdict>("run_module_verdict", {
      module: "mtu",
      input: {
        nic: {}, wifi: {}, ping_results: [], bufferbloat: {},
        speed_download: null, speed_upload: null,
        throttle: null, isp_intervention: null, ipv6: null, cgnat: null,
        mtu: result,
        voip: null, wifi_scan: null,
      },
    }).then(setVerdict).catch(() => setVerdict(null));
  }, [result]); // eslint-disable-line react-hooks/exhaustive-deps

  const run = async () => {
    setError(null);
    setRunning(true);
    try {
      const mtuResult = await invoke<MtuResult>("run_mtu_test");
      setLocalResult(mtuResult);
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setRunning(false);
    }
  };

  // Bos durum
  if (!result && !running) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
        {error && <ErrorBox msg={error} />}
        <div className="card" style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 16, padding: "44px 32px", textAlign: "center" }}>
          <div style={{ opacity: 0.45, color: "var(--text-muted)" }}>
            <Icon.Mtu style={{ width: 44, height: 44 }} />
          </div>
          <div style={{ fontSize: 17, fontWeight: 700, color: "var(--text)" }}>{t("mtu_intro_title")}</div>
          <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 500, lineHeight: 1.6, whiteSpace: "pre-line" }}>{t("mtu_intro_desc")}</div>
          <button className="btn" onClick={run} type="button">
            <Icon.Play />
            {t("mtu_run_btn")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      {error && <ErrorBox msg={error} />}

      <div style={{ display: "flex", justifyContent: "flex-end" }}>
        <button className="btn btn-secondary" onClick={run} disabled={running} type="button">
          {running ? <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} /> : <Icon.Refresh />}
          {t("mtu_run_btn")}
        </button>
      </div>

      {running && (
        <div className="card" style={{ display: "flex", alignItems: "center", gap: 10, padding: "14px 18px" }}>
          <Icon.Refresh style={{ animation: "spin 1s linear infinite", width: 16, height: 16 }} />
          <span style={{ fontSize: 13, color: "var(--text-dim)" }}>{t("mtu_running")}</span>
        </div>
      )}

      {/* Smart Grade */}
      {verdict && verdict.available && (
        <GradeCard
          grade={verdict.grade}
          comment={t(verdict.detail_key as Parameters<typeof t>[0])}
        />
      )}

      {result && (
        <>
          {/* Ozet/yorum karti */}
          <div className="card">
            <p style={{ margin: 0, fontSize: 13, color: "var(--text-dim)", lineHeight: 1.55 }}>{tk(result.detail_key)}</p>
          </div>

          {/* Hedef tablosu */}
          <div className="card">
            <div style={{ display: "grid", gridTemplateColumns: "1fr 120px 120px", gap: 10, padding: "0 14px 8px 14px", fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", color: "var(--text-muted)" }}>
              <span>{t("mtu_col_target")}</span>
              <span style={{ textAlign: "right" }}>{t("mtu_col_mtu")}</span>
              <span style={{ textAlign: "right" }}>{t("mtu_col_mss")}</span>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {result.targets.map((tg) => (
                <div key={tg.host} style={{ display: "grid", gridTemplateColumns: "1fr 120px 120px", alignItems: "center", gap: 10, padding: "10px 14px", borderRadius: 10, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)" }}>
                  <div style={{ minWidth: 0 }}>
                    <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text)" }}>{tg.label}</div>
                    <div style={{ fontFamily: "var(--mono)", fontSize: 10, color: "var(--text-muted)" }}>{tg.host}</div>
                  </div>
                  <span style={{ fontFamily: "var(--mono)", fontSize: 14, fontWeight: 700, color: mtuColor(tg.mtu), textAlign: "right" }}>
                    {tg.mtu ?? "—"}
                  </span>
                  <span style={{ fontFamily: "var(--mono)", fontSize: 13, color: "var(--text-dim)", textAlign: "right" }}>
                    {tg.mss ?? "—"}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {result.primary_mtu === null && (
            <div className="card" style={{ padding: "10px 14px", background: "rgba(253,224,71,0.06)", borderColor: "rgba(253,224,71,0.3)", color: "#fde047", fontSize: 12.5 }}>
              {t("mtu_blocked")}
            </div>
          )}
        </>
      )}
    </div>
  );
}

function ErrorBox({ msg }: { msg: string }) {
  return (
    <div className="card" style={{ padding: "10px 14px", background: "rgba(239,68,68,0.08)", borderColor: "rgba(239,68,68,0.3)", color: "#fca5a5", fontSize: 13 }}>
      {msg}
    </div>
  );
}

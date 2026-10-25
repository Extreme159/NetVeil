import { useEffect, useState } from "react";
import type { TabProps } from "./types";
import { Icon, type IconKey } from "../../lib/icons";
import { SpeedGauge } from "../SpeedGauge";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { HistoryView } from "../HistoryView";
import { ServerPicker } from "../ServerPicker";

type Phase = "download" | "upload";

function MiniMetric({
  iconKey, label, value, unit, sub, color,
}: {
  iconKey: IconKey;
  label: string;
  value: string;
  unit?: string;
  sub?: string;
  color: string;
}) {
  const Ico = Icon[iconKey];
  return (
    <div className="card" style={{ padding: "12px 14px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 8 }}>
        <div style={{
          width: 26, height: 26, borderRadius: 7,
          background: `color-mix(in oklab, ${color} 15%, transparent)`,
          color, display: "grid", placeItems: "center",
          border: `1px solid color-mix(in oklab, ${color} 35%, transparent)`,
        }}>
          <Ico style={{ width: 13, height: 13 }} />
        </div>
        <span style={{ fontSize: 11, textTransform: "uppercase", letterSpacing: "0.14em", color: "var(--text-muted)" }}>
          {label}
        </span>
      </div>
      <div style={{ display: "flex", alignItems: "baseline", gap: 5 }}>
        <span style={{ fontFamily: "var(--mono)", fontSize: 20, fontWeight: 700, letterSpacing: "-0.02em", color: "var(--text)" }}>
          {value}
        </span>
        {unit && <span style={{ fontFamily: "var(--mono)", fontSize: 12, color: "var(--text-muted)" }}>{unit}</span>}
      </div>
      {sub && <div style={{ marginTop: 4, fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--mono)" }}>{sub}</div>}
    </div>
  );
}

/** Adaptif max hesapla: 100 / 500 / 1000 */
function calcMax(peak: number): number {
  if (peak <= 100) return 100;
  if (peak <= 500) return 500;
  return 1000;
}

/** EMA smoothing — ani zıplamaları azalt */
const EMA_ALPHA = 0.25;


interface SpeedTestTabProps extends TabProps {
  showHistory?: boolean;
}

export function SpeedTestTab({ phase, liveDl, liveUl, liveSpeedPhase, finalDl, speedServerChoice, setSpeedServerChoice, speedResultCache, showHistory }: SpeedTestTabProps) {
  const [activePhase, setActivePhase] = useState<Phase>("download");
  const [smoothDl, setSmoothDl] = useState(0);
  const [smoothUl, setSmoothUl] = useState(0);
  const [peakDl, setPeakDl] = useState(0);
  const [peakUl, setPeakUl] = useState(0);

  const isSpeedtestRunning = phase === "running_speedtest";
  const isRunning = phase === "running_speedtest" || phase === "running_bufferbloat";

  useEffect(() => {
    if (liveSpeedPhase === "download" || liveSpeedPhase === "upload") setActivePhase(liveSpeedPhase);
  }, [liveSpeedPhase]);

  // EMA smoothing — live değerler geldiğinde
  useEffect(() => {
    if (!isSpeedtestRunning) return;
    setSmoothDl(prev => prev === 0 ? liveDl : prev + EMA_ALPHA * (liveDl - prev));
    setPeakDl(prev => Math.max(prev, liveDl));
  }, [liveDl, isSpeedtestRunning]);

  useEffect(() => {
    if (!isSpeedtestRunning) return;
    setSmoothUl(prev => prev === 0 ? liveUl : prev + EMA_ALPHA * (liveUl - prev));
    setPeakUl(prev => Math.max(prev, liveUl));
  }, [liveUl, isSpeedtestRunning]);

  // Test bitince smooth değerleri sıfırla
  useEffect(() => {
    if (!isSpeedtestRunning) {
      setSmoothDl(0); setSmoothUl(0);
      setPeakDl(0);   setPeakUl(0);
    }
  }, [isSpeedtestRunning]);

  // Seçili sunucunun cache'lenmiş sonucu (test çalışmıyorken göster)
  const cachedResult = speedResultCache[speedServerChoice] ?? null;
  const cachedDl     = cachedResult?.dl ?? 0;
  const cachedUl     = cachedResult?.ul ?? 0;
  const cachedServer = cachedResult?.server ?? null;
  const cachedPing   = cachedResult?.ping ?? null;

  // Gösterge değerleri: live çalışırken smooth; download fazi bitince final (p90) kilitli deger
  // (upload sirasinda/sonrasinda degismez); test tamamen bitince cache'den.
  const dl = isSpeedtestRunning ? (finalDl ?? smoothDl) : cachedDl;
  const ul = isSpeedtestRunning ? smoothUl : cachedUl;
  // Ping de ayni sunucu-anahtarli hiz kaydindan okunur (UI-07); bufferbloat idle gecikmesi KULLANILMAZ.
  const pingMs = cachedPing;

  // Upload sırasında DL gauge'u koru (sıfırlama), sadece UL fazda UL gauge sıfırlanmasın
  const displayDl = dl;
  const displayUl = isSpeedtestRunning && liveSpeedPhase === "download" ? 0 : ul;

  // Adaptif max
  const maxDl = isSpeedtestRunning ? calcMax(peakDl || 100) : calcMax(dl || 100);
  const maxUl = isSpeedtestRunning ? calcMax(peakUl || 100) : calcMax(ul || 100);

  useLang();
  const statusText = isRunning ? t("speed_testing") : phase === "done" ? t("speed_complete") : t("speed_idle");

  if (showHistory) {
    return <HistoryView module="speed" title={t("history_title")} />;
  }

  return (
    <div className="speed-wrap">

      {/* Sunucu seçici + Geçmiş butonu */}
      <div className="card" style={{ padding: "10px 14px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 12, flexWrap: "wrap" }}>
          <ServerPicker choice={speedServerChoice} setChoice={setSpeedServerChoice} disabled={isRunning} />

        </div>
      </div>

      {!isRunning && phase !== "done" && (
        <div className="card" style={{ padding: "10px 14px", fontSize: 12, color: "#ef4444", lineHeight: 1.6, border: "1px solid rgba(239,68,68,0.25)", background: "rgba(239,68,68,0.06)" }}>
          {t("test_warning_external_traffic")}
        </div>
      )}

      <div className="gauge-card">
        <div className="gauge-status">
          <span className="pulse" /> {statusText} · {activePhase}
        </div>

        <div style={{ display: "flex", gap: 16, alignItems: "center", justifyContent: "center", padding: "8px 24px", overflow: "visible" }}>
          <SpeedGauge value={displayDl} maxValue={maxDl} direction="ccw" label={t("stat_download").toUpperCase()} />
          <SpeedGauge value={displayUl} maxValue={maxUl} direction="cw"  label={t("stat_upload").toUpperCase()} />
        </div>
      </div>

      <div className="speed-stats">
        <MiniMetric iconKey="Down" label={t("stat_download")} value={!isSpeedtestRunning && !cachedResult ? "\u2014" : dl.toFixed(1)} unit="Mbps" color="#22d3ee" />
        <MiniMetric iconKey="Up"   label={t("stat_upload")}   value={!isSpeedtestRunning && !cachedResult ? "\u2014" : ul.toFixed(1)}  unit="Mbps" color="#a78bfa" />
        <MiniMetric iconKey="Bolt" label={t("stat_ping")}     value={pingMs != null ? pingMs.toFixed(0) : "\u2014"} unit="ms" color="#22c55e" />
        <MiniMetric
          iconKey="Globe"
          label={t("stat_server")}
          value={cachedServer ? (cachedServer.startsWith("netveil") ? "NetVeil" : "Cloudflare") : "—"}
          sub={cachedServer ? (cachedServer.startsWith("netveil") ? t("speed_srv_netveil_sub") : t("speed_cf_cdn")) : "—"}
          color="#22d3ee"
        />
      </div>

      <div className="speed-note-row" style={{
        marginTop: 12,
        padding: "10px 14px",
        borderRadius: 10,
        background: "var(--card-bg, rgba(255,255,255,0.03))",
        border: "1px solid var(--card-border, rgba(255,255,255,0.06))",
        display: "flex", alignItems: "flex-start", gap: 8,
      }}>
        <Icon.Idea size={14} style={{ color: "var(--text-muted, #8b8b9e)", flexShrink: 0, marginTop: 2 }} />
        <span style={{ fontSize: 11, lineHeight: 1.5, color: "var(--text-muted, #8b8b9e)" }}>
          {t("speed_measure_note")}
        </span>
      </div>
    </div>
  );
}

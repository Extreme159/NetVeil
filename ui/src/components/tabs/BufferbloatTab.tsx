
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TabProps } from "./types";
import type { LatencySample, ModuleVerdict } from "../../types";
import { Icon } from "../../lib/icons";
import { TestIntro } from "../TestIntro";
import { ServerPicker } from "../ServerPicker";
import { gradeColor } from "../../lib/helpers";
import { GradeBox } from "../GradeBox";
import { GradeCard } from "../GradeCard";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { HistoryView } from "../HistoryView";


interface BufferbloatTabProps extends TabProps {
  showHistory?: boolean;
}

export function BufferbloatTab({ phase, bufferbloatServerChoice, setBufferbloatServerChoice, bufferbloatCache, runBufferbloat, showHistory }: BufferbloatTabProps) {
  useLang();
  // Gosterilen sonuc secili kaynagin CACHE'inden gelir (kaynak degisince o
  // sunucunun tamamlanmis sonucu gorunur). Baska kaynagin sonucu ASLA gosterilmez (UI-08):
  // secili kaynak test edilmediyse 'test edilmedi' (intro) durumu gorunur.
  const bufferbloat = bufferbloatCache[bufferbloatServerChoice] ?? null;
  const isRunning = phase === "running_bufferbloat";
  const [verdict, setVerdict] = useState<ModuleVerdict | null>(null);

  useEffect(() => {
    if (!bufferbloat) { setVerdict(null); return; }
    invoke<ModuleVerdict>("run_module_verdict", {
      module: "bufferbloat",
      input: {
        nic: {}, wifi: {}, ping_results: [], bufferbloat: bufferbloat,
        speed_download: null, speed_upload: null,
        throttle: null, isp_intervention: null, ipv6: null, cgnat: null, mtu: null, voip: null, wifi_scan: null,
        dns_benchmark: null, twitch: null,
      },
    }).then(setVerdict).catch(() => setVerdict(null));
  }, [bufferbloat]); // eslint-disable-line react-hooks/exhaustive-deps

  if (showHistory) {
    return <HistoryView module="bufferbloat" title={t("history_title")} />;
  }

  // Çalışırken spinner göster
  if (isRunning) {
    return (
      <div className="card" style={{ padding: 28, textAlign: "center" }}>
        <div style={{ width: 56, height: 56, margin: "0 auto 16px", borderRadius: 16, border: "2px solid var(--border)", borderTopColor: "var(--accent)", animation: "spin 1s linear infinite" }} />
        <p style={{ fontSize: 14, fontWeight: 600, margin: 0 }}>{t("bloat_saturating")}</p>
      </div>
    );
  }

  // Henüz test yapılmadı — gerçek layout, değerler boş
  if (!bufferbloat) {
    // Bos durum: ortak pat1 kalibi. Default kaynak (Cloudflare) ile baslar;
    // sonuc gelince ServerPicker ust'te gorunur, kullanici 2. kaynagi secip tekrar test eder.
    // Baska kaynagin sonucu varsa picker gorunur kalir (aksi halde kullanici kaynak degistirip geri donemez).
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {Object.keys(bufferbloatCache).length > 0 && (
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <ServerPicker choice={bufferbloatServerChoice} setChoice={setBufferbloatServerChoice} disabled={isRunning} />
          </div>
        )}
        <TestIntro
          guarded
          iconKey="Buffer"
          title={t("tab_bufferbloat_title")}
          description={t("bloat_intro_desc")}
          buttonLabel={t("btn_run_bufferbloat")}
          onRun={() => runBufferbloat(bufferbloatServerChoice)}
          isRunning={isRunning}
          warning={t("test_warning_external_traffic")}
        />
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <ServerPicker choice={bufferbloatServerChoice} setChoice={setBufferbloatServerChoice} disabled={isRunning} />
      </div>

      {verdict && verdict.available && (
        <GradeCard
          grade={verdict.grade}
          comment={t(verdict.detail_key as Parameters<typeof t>[0])}
        />
      )}

      <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14 }}>
        {[
          { label: t("bloat_idle_p50"),    value: bufferbloat.idle_latency_ms.toFixed(1),       unit: "ms", accent: "#6366f1" },
          { label: t("bloat_loaded_p95"),   value: bufferbloat.loaded_latency_ms.toFixed(1),     unit: "ms", accent: "#f97316" },
          { label: t("bloat_delta"),        value: "+" + bufferbloat.latency_delta_ms.toFixed(1), unit: "ms", accent: gradeColor(bufferbloat.grade) },
        ].map(m => (
          <div key={m.label} className="stat-card" style={{ ["--accent-color" as string]: m.accent }}>
            <div className="stat-head">
              <span>{m.label}</span>
            </div>
            <div className="stat-value">
              {m.value}
              <span className="unit">{m.unit}</span>
            </div>
          </div>
        ))}
      </div>

      {bufferbloat.segments && bufferbloat.segments.length > 0 && (
        <SegmentsCard
          segments={bufferbloat.segments}
          diagnosis={bufferbloat.diagnosis}
        />
      )}

      {bufferbloat.samples.length > 0 && (() => {
        const hasGw = bufferbloat.samples.some(s => s.phase.endsWith("_gw"));
        const legend = (
          <div style={{ display: "flex", gap: 16, marginTop: 12 }}>
            {[
              { color: "#6366f1", label: t("bloat_legend_idle") },
              { color: "#f97316", label: t("bloat_legend_download") },
              { color: "#ec4899", label: t("bloat_legend_upload") },
              { color: "#14b8a6", label: t("bloat_legend_bidi") },
            ].map(({ color, label }) => (
              <div key={label} style={{ display: "flex", alignItems: "center", gap: 6 }}>
                <span style={{ width: 8, height: 8, borderRadius: "50%", background: color, boxShadow: `0 0 6px ${color}` }} />
                <span style={{ fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--mono)" }}>{label}</span>
              </div>
            ))}
          </div>
        );
        return (
          <>
            {/* PC ↔ WAN segmenti grafigi */}
            <div className="card">
              <div className="card-head">
                <div className="ico-wrap"><Icon.Wave /></div>
                <h3>{t("bloat_latency_chart")} — PC ↔ WAN</h3>
                <span className="head-sub">{bufferbloat.samples.filter(s => !s.phase.endsWith("_gw")).length} {t("bloat_sample_unit")}</span>
              </div>
              <Chart samples={bufferbloat.samples} />
              {legend}
            </div>
            {/* PC ↔ Router (gateway) segmenti grafigi — gateway tespit edildiyse */}
            {hasGw && (
              <div className="card">
                <div className="card-head">
                  <div className="ico-wrap"><Icon.Wave /></div>
                  <h3>{t("bloat_latency_chart")} — PC ↔ Router</h3>
                  <span className="head-sub">{bufferbloat.samples.filter(s => s.phase.endsWith("_gw")).length} {t("bloat_sample_unit")}</span>
                </div>
                <Chart samples={bufferbloat.samples} gateway />
                {legend}
              </div>
            )}
          </>
        );
      })()}
    </div>
  );
}

function SegmentsCard({
  segments,
  diagnosis,
}: {
  segments: import("../../types").SegmentStats[];
  diagnosis: string;
}) {
  const labelFor = (seg: string) =>
    seg === "gateway" ? t("bloat_lan") : seg === "wan" ? t("bloat_wan") : seg;
  const subFor = (seg: string) =>
    seg === "gateway" ? t("bloat_lan_sub") : t("bloat_wan_sub");

  return (
    <div className="card" style={{ padding: 22 }}>
      <div className="card-head" style={{ marginBottom: 14 }}>
        <div className="ico-wrap"><Icon.Server /></div>
        <h3>{t("bloat_segment")}</h3>
        <span className="head-sub">{t("bloat_culprit")}</span>
      </div>

      <div
        style={{
          padding: "12px 14px",
          marginBottom: 16,
          background: "rgba(99,102,241,0.08)",
          border: "1px solid rgba(99,102,241,0.25)",
          borderRadius: 10,
          fontSize: 13,
          lineHeight: 1.55,
          color: "var(--text-dim)",
        }}
      >
        {diagnosis}
      </div>

      <div style={{ display: "grid", gridTemplateColumns: `repeat(${segments.length}, 1fr)`, gap: 14 }}>
        {segments.map(s => {
          const c = gradeColor(s.grade);
          return (
            <div
              key={s.target}
              className="stat-card"
              style={{ ["--accent-color" as string]: c, padding: 16 }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                <GradeBox grade={s.grade} size="sm" />
                <span style={{ fontSize: 13, fontWeight: 600 }}>{labelFor(s.target)}</span>
              </div>
              <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 10 }}>
                {subFor(s.target)} · <span className="mono">{s.target_ip}</span>
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr 1fr 1fr", gap: 10 }}>
                <Metric label="Idle p50" value={s.idle_p50_ms.toFixed(1)} unit="ms" />
                <Metric label="Loaded p95" value={s.loaded_p95_ms.toFixed(1)} unit="ms" />
                <Metric label="Δ" value={"+" + s.delta_ms.toFixed(1)} unit="ms" color={c} />
                <Metric label="Δ Bi-Di" value={"+" + (s.bidi_delta_ms ?? 0).toFixed(1)} unit="ms" color="#14b8a6" />
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function Metric({ label, value, unit, color }: { label: string; value: string; unit: string; color?: string }) {
  return (
    <div>
      <div style={{ fontSize: 10, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.1em" }}>
        {label}
      </div>
      <div style={{ fontFamily: "var(--mono)", fontSize: 16, fontWeight: 700, color: color || "var(--text)", marginTop: 2 }}>
        {value}
        <span style={{ fontSize: 10, color: "var(--text-muted)", marginLeft: 3 }}>{unit}</span>
      </div>
    </div>
  );
}

// Adaptif "güzel" tavan: degeri okunabilir bir ust sinira yuvarlar (1/2/5 x 10^n).
function niceCeil(v: number): number {
  if (v <= 1) return 1;
  const pow = Math.pow(10, Math.floor(Math.log10(v)));
  const n = v / pow;
  const step = n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10;
  return step * pow;
}

function Chart({ samples, gateway = false }: { samples: LatencySample[]; gateway?: boolean }) {
  const W = 760;
  const H = 160;
  const PAD = 14;
  const PADL = 34; // sol pad: Y ekseni ms etiketleri icin

  // gateway=true ise '_gw' suffix'li (PC-Router) fazlari, degilse suffix'siz
  // (PC-WAN) fazlari ciz. Tek samples dizisinden ilgili segmenti suzeriz.
  const segSamples = samples.filter(s =>
    gateway ? s.phase.endsWith("_gw") : !s.phase.endsWith("_gw")
  );
  const basePhase = (p: string) => p.replace(/_gw$/, "");
  const phaseColor = (p: string) => {
    const b = basePhase(p);
    return b === "idle" ? "#6366f1" : b === "download" ? "#f97316"
      : b === "upload" ? "#ec4899" : "#14b8a6"; // bidirectional = teal
  };
  const rawMax = Math.max(...segSamples.map(s => s.latency_ms), 1);
  const maxMs = niceCeil(rawMax); // adaptif, yuvarlanmis tavan

  const pts = segSamples.map((s, i) => {
    const x = PADL + (i / Math.max(1, segSamples.length - 1)) * (W - PADL - PAD);
    const y = PAD + (1 - s.latency_ms / maxMs) * (H - PAD * 2);
    return { x, y, phase: basePhase(s.phase) };
  });

  // Y ekseni: 0, 1/4, 1/2, 3/4, max -> ms etiketleri (adaptif)
  const yTicks = [0, 0.25, 0.5, 0.75, 1].map(f => ({
    f,
    y: PAD + (1 - f) * (H - PAD * 2),
    ms: Math.round(maxMs * f),
  }));

  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      preserveAspectRatio="none"
      style={{ width: "100%", height: 160, background: "rgba(10,10,15,0.4)", borderRadius: 10 }}
    >
      {yTicks.map(t => (
        <g key={t.f}>
          <line
            x1={PADL}
            x2={W - PAD}
            y1={t.y}
            y2={t.y}
            stroke="#1a1a2e"
            strokeWidth="1"
          />
          <text
            x={PADL - 6}
            y={t.y + 3}
            fontSize={9}
            textAnchor="end"
            fontFamily="JetBrains Mono, monospace"
            fill="#6b7280"
          >
            {t.f === 1 ? `${t.ms} ms` : t.ms}
          </text>
        </g>
      ))}
      {["idle", "download", "upload", "bidirectional"].map(p => {
        const phPts = pts.filter(pt => pt.phase === p);
        if (phPts.length === 0) return null;
        const x1 = phPts[0].x;
        const x2 = phPts[phPts.length - 1].x;
        return (
          <rect
            key={p}
            x={x1}
            y={PAD}
            width={Math.max(1, x2 - x1)}
            height={H - PAD * 2}
            fill={phaseColor(p)}
            opacity={0.08}
          />
        );
      })}
      <polyline
        points={pts.map(p => `${p.x},${p.y}`).join(" ")}
        fill="none"
        stroke="#a5b4fc"
        strokeWidth="2"
        strokeLinejoin="round"
      />
      {pts.filter((_, i) => i % 3 === 0).map((p, i) => (
        <circle key={i} cx={p.x} cy={p.y} r={2.5} fill={phaseColor(p.phase)} />
      ))}

    </svg>
  );
}

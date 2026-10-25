import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AccessPoint, WifiScanResult, WifiInfo, ModuleVerdict } from "../../types";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { Icon } from "../../lib/icons";
import { TestIntro } from "../TestIntro";
import { GradeCard } from "../GradeCard";

// ── Statik Kanal Tanımları ───────────────────────────────────────────────────

const CHANNELS_24 = [1,2,3,4,5,6,7,8,9,10,11,12,13];
const NON_OVERLAPPING_24 = [1,6,11];

const CHANNELS_5 = [
  36,40,44,48,           // UNII-1 (no DFS)
  52,56,60,64,           // UNII-2A (DFS)
  100,104,108,112,116,120,124,128,132,136,140,144, // UNII-2C (DFS)
  149,153,157,161,165,   // UNII-3 (no DFS)
];
const NON_DFS_5 = [36,40,44,48,149,153,157,161,165];

const CHANNELS_6_PSC = [5,21,37,53,69,85,101,117,133,149,165,181,197,213,229];

// ── Renk Yardımcıları ────────────────────────────────────────────────────────

const BAND_COLORS = [
  "#3b82f6","#10b981","#f59e0b","#ef4444",
  "#8b5cf6","#ec4899","#06b6d4","#84cc16","#f97316","#a78bfa",
];

function signalColor(q: number): string {
  return q >= 70 ? "var(--success)" : q >= 40 ? "#f59e0b" : "#ef4444";
}
// Çakışma Skoru: 100 = hiç çakışma yok (yeşil), 0 = yoğun çakışma (kırmızı)
// Rust'tan gelen congestion_score yükleme birimidir (yüksek = kötü)
// Frontend'de 100'den çıkarıp normalize ediyoruz.
function overlapScoreColor(rawLoad: number, maxLoad: number): string {
  const score = maxLoad === 0 ? 100 : Math.max(0, 100 - Math.round((rawLoad / maxLoad) * 100));
  return score >= 70 ? "var(--success)" : score >= 40 ? "#f59e0b" : "#ef4444";
}
// Eski congestionColor — Rust'tan gelen doğrudan congestion_score için
// (0 = iyi, yüksek = kötü) — sadece optimizasyon kartında kullanılıyor
function congestionColor(rawScore: number): string {
  // rawScore yükleme toplamı, normalize yönü ters olduğundan
  // 0 = iyi (yeşil), herhangi bir değer varsa kötü
  return rawScore === 0 ? "var(--success)" : rawScore < 50 ? "#f59e0b" : "#ef4444";
}
function congestionLabel(rawScore: number): string {
  return rawScore === 0 ? t("wifi_scan_congestion_low") : rawScore < 50 ? t("wifi_scan_congestion_medium") : t("wifi_scan_congestion_high");
}

// ── Auto-scan interval: IEEE 802.11k/v passive scan budget ─────────────────
// Endüstri standardı: pasif tarama için 20-60 saniye. Biz 30 saniyeyi seçtik.
// 802.11k RRM spec § 11.10.3 - minimum passive dwell: 10-30 ms per channel
// Toplamda 100+ kanal × 30 ms = ~3s aktif tarama; driver overhead ile 4-6s
// Kullanıcı deneyimi için 30 saniye, çok agresif değil, yeterince güncel.
const AUTO_SCAN_INTERVAL_MS = 30_000;

// ── Kanal Şekil Eğrisi ───────────────────────────────────────────────────────
// inSSIDer / Acrylic / Ekahau tarzı yarım elips (cosine bell)
// Kanal genişliği px cinsinden verilir; ayaklar tam bu noktada tabana oturur.
// Cosine bell: f(t) = A × (1 + cos(π × t)) / 2,  t ∈ [-1, 1]
// Gaussian'dan farkı: kenarlar sert iner, taşma yok.
function channelBellPath(
  cx: number,
  peakY: number,
  baseY: number,
  halfWidthPx: number, // tam kanal genişliğinin yarısı - ayaklar burada sıfır
  steps = 48,
): string {
  const amplitude = baseY - peakY;
  let d = `M ${(cx - halfWidthPx).toFixed(1)} ${baseY.toFixed(1)}`;
  for (let i = 0; i <= steps; i++) {
    const t = -1 + (2 * i) / steps;           // t: -1 → +1
    const x = cx + t * halfWidthPx;
    const y = baseY - amplitude * (1 + Math.cos(Math.PI * t)) / 2;
    d += ` L ${x.toFixed(1)} ${y.toFixed(1)}`;
  }
  d += ` L ${(cx + halfWidthPx).toFixed(1)} ${baseY.toFixed(1)} Z`;
  return d;
}

// ── Gaussian Kanal Haritası ──────────────────────────────────────────────────

interface ChannelMapProps {
  aps: AccessPoint[];
  channels: number[];
  nonOverlapping: number[];
  connectedSsid: string;
  band: string;
  staleBssids: Set<string>;
}

function GaussianChannelMap({ aps, channels, nonOverlapping, band, staleBssids }: ChannelMapProps) {
  const W = 900;  // tam container genişliği - SVG %100 width ile scale eder
  const H = 260;
  const PAD_L = 46;
  const PAD_R = 20;
  const PAD_T = 40;
  const PAD_B = 30;
  const chartW = W - PAD_L - PAD_R;
  const chartH = H - PAD_T - PAD_B;
  const baseY = PAD_T + chartH;

  // ── Kanal numarası → MHz ────────────────────────────────────────────
  const chToMhz = (ch: number): number => {
    if (band === "2.4 GHz") return ch === 14 ? 2484 : 2407 + ch * 5;
    if (band === "6 GHz")   return ch === 2  ? 5935 : 5950 + ch * 5;
    return 5000 + ch * 5;
  };

  // ── Kenar padding: ilk/son kanalın eğrisi Y eksenine taşmasın ─────────
  // Her band için kenar boşluğu MHz cinsinden:
  // 2.4 GHz: 22 MHz BW → ±11 MHz kenar padding
  // 5 GHz:   en geniş olası BW/2 = 80 MHz (pratik limit)
  // 6 GHz:   80 MHz min → 40 MHz
  const EDGE_PAD_MHZ =
    band === "2.4 GHz" ? 11 :
    band === "6 GHz"   ? 40 :
    20; // 5 GHz: 40 MHz BW default

  // ── 5 GHz segment break: 64 → 100 arası boşluk yok ──────────────────
  const GAP_PX = 18;
  const SEG_BREAK_LOW  = 64;
  const SEG_BREAK_HIGH = 100;

  const seg1Chs = band === "5 GHz" ? channels.filter(ch => ch <= SEG_BREAK_LOW)  : channels;
  const seg2Chs = band === "5 GHz" ? channels.filter(ch => ch >= SEG_BREAK_HIGH) : [];
  const hasSplit = band === "5 GHz" && seg1Chs.length > 0 && seg2Chs.length > 0;

  // Kenar padding dahil MHz aralıkları
  const seg1MinF = seg1Chs.length ? chToMhz(seg1Chs[0]) - EDGE_PAD_MHZ : 0;
  const seg1MaxF = seg1Chs.length ? chToMhz(seg1Chs[seg1Chs.length - 1]) + EDGE_PAD_MHZ : 0;
  const seg2MinF = seg2Chs.length ? chToMhz(seg2Chs[0]) - EDGE_PAD_MHZ : 0;
  const seg2MaxF = seg2Chs.length ? chToMhz(seg2Chs[seg2Chs.length - 1]) + EDGE_PAD_MHZ : 0;

  // 6 GHz: eksen TUM bandi kapsar (5925-7125 MHz); `channels` (PSC) yalnizca vurgulu tick'tir.
  // Aksi halde PSC olmayan AP'ler eksen disinda kalip haritadan kayboluyordu (UI-13).
  const allMinF = band === "6 GHz" ? 5925 : chToMhz(channels[0]) - EDGE_PAD_MHZ;
  const allMaxF = band === "6 GHz" ? 7125 : chToMhz(channels[channels.length - 1]) + EDGE_PAD_MHZ;

  const seg1SpanF = hasSplit ? (seg1MaxF - seg1MinF) || 1 : 0;
  const seg2SpanF = hasSplit ? (seg2MaxF - seg2MinF) || 1 : 0;
  const totalSpanF = hasSplit ? seg1SpanF + seg2SpanF : (allMaxF - allMinF) || 1;

  const pxPerMhz = hasSplit
    ? (chartW - GAP_PX) / totalSpanF
    : chartW / totalSpanF;

  const mhzToX = (mhz: number): number => {
    if (!hasSplit) return PAD_L + ((mhz - allMinF) / totalSpanF) * chartW;
    // Segment 1 veya 2?
    const seg1RealMax = seg1Chs.length ? chToMhz(seg1Chs[seg1Chs.length - 1]) : 0;
    if (mhz <= seg1RealMax + 2) {
      return PAD_L + (mhz - seg1MinF) * pxPerMhz;
    }
    return PAD_L + seg1SpanF * pxPerMhz + GAP_PX + (mhz - seg2MinF) * pxPerMhz;
  };

  const chX = (ch: number): number => {
    if (!channels.includes(ch)) return -999;
    return mhzToX(chToMhz(ch));
  };

  // AP konumu: 6 GHz'de frekansa gore (PSC olmayan kanallar dahil); diger bantlarda kanal tick'ine gore.
  const apX = (ap: AccessPoint): number => {
    if (band === "6 GHz") return mhzToX(ap.center_freq_mhz > 0 ? ap.center_freq_mhz : chToMhz(ap.channel));
    return chX(ap.channel);
  };

  const gapX = hasSplit ? PAD_L + seg1SpanF * pxPerMhz : -1;

  // SSID → renk
  const ssidColor: Record<string, string> = {};
  let colorIdx = 0;
  for (const ap of aps) {
    if (!ssidColor[ap.ssid]) {
      ssidColor[ap.ssid] = BAND_COLORS[colorIdx % BAND_COLORS.length];
      colorIdx++;
    }
  }

  // ── Kanal genişliği → yarı-genişlik px ───────────────────────────────
  // IEEE 802.11: BW/2 MHz → px - pxPerMhz ile dönüştür
  const defaultHalfMhz =
    band === "2.4 GHz" ? 11 :
    band === "6 GHz"   ? 40 :
    10;

  const widthToHalfMhz = (w: string): number => {
    if (w.startsWith("320")) return 160;
    if (w.startsWith("160")) return 80;
    if (w.startsWith("80"))  return 40;
    if (w.startsWith("40"))  return 20;
    return 10;
  };

  // ── Y ekseni: RSSI scale ───────────────────────────────────────────
  // AP'lerin RSSI aralığına göre dinamik ama gözel görünen ticks
  const rssiValues = aps.map(ap => ap.rssi);
  const rssiMax = rssiValues.length ? Math.max(...rssiValues) : -30;
  const rssiMin = rssiValues.length ? Math.min(...rssiValues) : -90;
  // Üst sınır: en iyi sinyalin 10 dBm üstüne yuvarla, en az -20
  const yTop  = Math.min(-20, Math.floor((rssiMax + 10) / 10) * 10);
  // Alt sınır: en kötü sinyalin 10 dBm altına yuvarla, en fazla -100
  const yBot  = Math.max(-100, Math.ceil((rssiMin - 10) / 10) * 10);
  const ySpan = yBot - yTop; // negatif sayı (top < bot)

  // RSSI değeri → Y piksel
  const rssiToY = (dbm: number): number =>
    PAD_T + ((dbm - yTop) / ySpan) * chartH;

  // Y ekseni tick'leri: 10 dBm aralıklı
  const yTicks: number[] = [];
  for (let v = yTop; v >= yBot; v -= 10) yTicks.push(v);

  // 5 GHz band overlay zones
  const band5Zones = band === "5 GHz" ? [
    { from: 36, to: 48, color: "rgba(59,130,246,0.06)", label: "UNII-1" },
    { from: 52, to: 64, color: "rgba(245,158,11,0.04)", label: "UNII-2A" },
    { from: 100, to: 144, color: "rgba(245,158,11,0.04)", label: "UNII-2C" },
    { from: 149, to: 165, color: "rgba(16,185,129,0.06)", label: "UNII-3" },
  ] : [];

  return (
    <svg viewBox={`0 0 ${W} ${H}`} style={{ width: "100%", height: "auto", display: "block" }}>

      {/* Y ekseni grid + RSSI etiketleri */}
      {yTicks.map((dbm) => {
        const y = rssiToY(dbm);
        return (
          <g key={`ytick-${dbm}`}>
            <line
              x1={PAD_L} y1={y} x2={PAD_L + chartW} y2={y}
              stroke="rgba(255,255,255,0.06)" strokeWidth={1}
            />
            <text
              x={PAD_L - 4} y={y + 3.5}
              textAnchor="end" fontSize={8}
              fill="rgba(255,255,255,0.35)"
            >
              {dbm}
            </text>
          </g>
        );
      })}

      {/* Y ekseni başlığı */}
      <text
        x={10} y={PAD_T + chartH / 2}
        textAnchor="middle" fontSize={8}
        fill="rgba(255,255,255,0.3)"
        transform={`rotate(-90, 10, ${PAD_T + chartH / 2})`}
      >
        dBm
      </text>

      {/* 5 GHz band overlay'leri */}
      {band5Zones.map((zone) => {
        const x1 = chX(zone.from);
        const x2 = chX(zone.to);
        if (x1 < 0 || x2 < 0) return null;
        // Segment kırığı varsa ve zone iki segmente yayılıyorsa çizme
        const zoneW = Math.max(0, x2 - x1);
        if (zoneW === 0) return null;
        return (
          <g key={zone.label}>
            <rect x={x1} y={PAD_T} width={zoneW} height={chartH} fill={zone.color} rx={2} />
            <text x={x1 + zoneW / 2} y={PAD_T + 10} textAnchor="middle" fontSize={8} fill="rgba(255,255,255,0.25)">
              {zone.label}
            </text>
          </g>
        );
      })}

      {/* 5 GHz segment kırık işareti (// tarzı) */}
      {hasSplit && (
        <g>
          {/* Gri arka plan şerit */}
          <rect x={gapX} y={PAD_T} width={GAP_PX} height={chartH}
            fill="rgba(0,0,0,0.35)" />
          {/* Sol kesik çizgi */}
          <line x1={gapX + 5} y1={PAD_T} x2={gapX + 3} y2={baseY}
            stroke="rgba(255,255,255,0.2)" strokeWidth={1} strokeDasharray="3 2" />
          {/* Sağ kesik çizgi */}
          <line x1={gapX + GAP_PX - 5} y1={PAD_T} x2={gapX + GAP_PX - 3} y2={baseY}
            stroke="rgba(255,255,255,0.2)" strokeWidth={1} strokeDasharray="3 2" />
          {/* ⋯ noktası */}
          <text x={gapX + GAP_PX / 2} y={PAD_T + chartH / 2 + 4}
            textAnchor="middle" fontSize={9} fill="rgba(255,255,255,0.3)">
            ⋯
          </text>
        </g>
      )}

      {/* Taban çizgisi */}
      <line x1={PAD_L} y1={baseY} x2={PAD_L + chartW} y2={baseY} stroke="rgba(255,255,255,0.1)" strokeWidth={1} />

      {/* Kanal kılavuz çizgileri */}
      {channels.map((ch) => {
        const x = chX(ch);
        const isHighlight = nonOverlapping.includes(ch);
        return (
          <line
            key={`guide-${ch}`}
            x1={x} y1={PAD_T}
            x2={x} y2={baseY}
            stroke={isHighlight ? "rgba(255,255,255,0.12)" : "rgba(255,255,255,0.04)"}
            strokeWidth={1}
            strokeDasharray={isHighlight ? "none" : "2 3"}
          />
        );
      })}

      {/* AP kanal eğrileri + SSID etiketleri */}
      {aps.map((ap, i) => {
        const color = ssidColor[ap.ssid] ?? "#3b82f6";
        const isConn = ap.is_connected;
        const isOwnAp = !isConn && ap.is_own;
        const isStale = staleBssids.has(ap.bssid);
        const cx = apX(ap);
        if (cx < 0) return null;
        const peakY = rssiToY(ap.rssi);

        const halfMhz = band === "2.4 GHz"
          ? defaultHalfMhz
          : widthToHalfMhz(ap.channel_width);
        const halfWidthPx = halfMhz * pxPerMhz;

        const d = channelBellPath(cx, peakY, baseY, halfWidthPx);
        const label = ap.ssid.length > 14 ? ap.ssid.slice(0, 12) + "…" : ap.ssid;
        const labelY = peakY - 5;
        // 3 durum: bağlı (tam güçlü) / kendi AP diğer band (orta) / yabancı (soluk)
        const opacityVal = isStale ? 0.35 : (isConn ? 1 : isOwnAp ? 0.88 : 0.75);
        const fillOpacityVal = isConn ? 0.28 : isOwnAp ? 0.18 : 0.1;
        const strokeWidthVal = isConn ? 2.5 : isOwnAp ? 2.0 : 1.5;
        const strokeDashVal = isStale ? "4 3" : isOwnAp ? "5 2" : "none";
        return (
          <g key={`${ap.bssid}-${i}`} opacity={opacityVal}>
            <path
              d={d}
              fill={color}
              fillOpacity={fillOpacityVal}
              stroke={color}
              strokeWidth={strokeWidthVal}
              strokeDasharray={strokeDashVal}
            />
            <text
              x={cx}
              y={labelY}
              textAnchor="middle"
              fontSize={isConn ? 10 : 9}
              fill={color}
              fontWeight={isConn ? "bold" : isOwnAp ? "600" : "500"}
              style={{ pointerEvents: "none" }}
            >
              {label}{isStale ? " ◌" : ""}
            </text>
          </g>
        );
      })}

      {/* X ekseni - kanal etiketleri */}
      {channels.map((ch) => {
        const x = chX(ch);
        const isHighlight = nonOverlapping.includes(ch);
        let fill = isHighlight ? "var(--text)" : "var(--text-muted)";
        let fw: string | number = isHighlight ? "600" : "400";
        const fs = band === "5 GHz" ? 8 : 10;

        // 5 GHz: non-DFS mavi, DFS amber
        if (band === "5 GHz") {
          if (NON_DFS_5.includes(ch)) { fill = "#3b82f6"; fw = "700"; }
          else fill = "#f59e0b";
        }

        return (
          <g key={`label-${ch}`}>
            {/* 6 GHz PSC dot */}
            {band === "6 GHz" && (
              <circle cx={x} cy={PAD_T - 6} r={2.5} fill="rgba(99,102,241,0.7)" />
            )}
            <text
              x={x}
              y={H - 6}
              textAnchor="middle"
              fontSize={fs}
              fill={fill}
              fontWeight={fw}
            >
              {ch}
            </text>
          </g>
        );
      })}
    </svg>
  );
}

// ── Ağ Tablosu ────────────────────────────────────────────────────────────────

type SortKey = "signal" | "channel";
type SortDir = "asc" | "desc";

// Top-level — render içinde tanımlanmamalı (React "Cannot create components during render")
function SortBtn({ k, label, active, arrow, onSort }: {
  k: SortKey; label: string; active: boolean; arrow: string; onSort: (k: SortKey) => void;
}) {
  return (
    <button
      onClick={() => onSort(k)}
      style={{
        padding: "3px 10px", borderRadius: 6, fontSize: 11,
        fontWeight: active ? 700 : 500, cursor: "pointer",
        background: active ? "var(--accent)" : "rgba(255,255,255,0.06)",
        border: active ? "1px solid transparent" : "1px solid var(--border)",
        color: active ? "#fff" : "var(--text-dim)",
        transition: "all 0.15s",
      }}
    >
      {label}{arrow}
    </button>
  );
}

function NetworkTable({ aps, staleBssids }: { aps: AccessPoint[]; connectedSsid?: string; staleBssids: Set<string> }) {
  const [sortBy, setSortBy] = useState<SortKey>("signal");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const handleSort = (key: SortKey) => {
    if (sortBy === key) {
      setSortDir((d) => (d === "desc" ? "asc" : "desc"));
    } else {
      setSortBy(key);
      setSortDir("desc");
    }
  };

  const sorted = [...aps].sort((a, b) => {
    const mul = sortDir === "desc" ? -1 : 1;
    if (sortBy === "signal") return mul * (a.signal_quality - b.signal_quality);
    return mul * (a.channel - b.channel);
  });

  return (
    <>
      {/* Sıralama kontrolleri */}
      <div style={{ display: "flex", gap: 6, marginBottom: 10 }}>
        <SortBtn k="signal" label={t("wifi_scan_col_signal")} active={sortBy === "signal"} arrow={sortBy === "signal" ? (sortDir === "desc" ? " ↓" : " ↑") : " ↕"} onSort={handleSort} />
        <SortBtn k="channel" label={t("wifi_scan_col_channel")} active={sortBy === "channel"} arrow={sortBy === "channel" ? (sortDir === "desc" ? " ↓" : " ↑") : " ↕"} onSort={handleSort} />
      </div>

      <div style={{ overflowX: "auto" }}>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 12, fontFamily: "var(--mono)" }}>
          <thead>
            <tr style={{ borderBottom: "1px solid var(--border)" }}>
              {[
                t("wifi_scan_col_ssid"),
                t("wifi_scan_col_band"),
                t("wifi_scan_col_channel"),
                t("wifi_scan_col_signal"),
                t("wifi_scan_col_width"),
                t("wifi_scan_col_phy"),
                t("wifi_scan_col_bssid"),
              ].map((col) => (
                <th key={col} style={{ padding: "6px 10px", textAlign: "left", color: "var(--text-muted)", fontWeight: 500, whiteSpace: "nowrap", fontSize: 11 }}>
                  {col}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {sorted.map((ap, idx) => {
              const isConn = ap.is_connected;
              const sigColor = signalColor(ap.signal_quality);
              return (
                <tr
                  key={`${ap.bssid}-${idx}`}
                  style={{
                    borderBottom: "1px solid rgba(255,255,255,0.04)",
                    background: isConn ? "rgba(16,185,129,0.06)" : "transparent",
                    opacity: staleBssids.has(ap.bssid) ? 0.4 : 1,
                  }}
                >
                  {/* SSID */}
                  <td style={{ padding: "7px 10px", whiteSpace: "nowrap" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                      {isConn && (
                        <span style={{
                          fontSize: 9, fontWeight: 700, padding: "1px 5px", borderRadius: 3,
                          background: "rgba(16,185,129,0.2)", color: "var(--success)",
                          border: "1px solid rgba(16,185,129,0.35)", fontFamily: "var(--sans)",
                        }}>
                          {t("wifi_scan_connected_badge")}
                        </span>
                      )}
                      <span style={{ color: isConn ? "var(--text)" : "var(--text-dim)", fontFamily: "var(--sans)" }}>
                        {ap.ssid}
                      </span>
                    </div>
                  </td>
                  {/* Band */}
                  <td style={{ padding: "7px 10px", color: "var(--text-muted)" }}>{ap.band}</td>
                  {/* Channel */}
                  <td style={{ padding: "7px 10px", color: "var(--text)", fontWeight: 600 }}>{ap.channel}</td>
                  {/* Signal */}
                  <td style={{ padding: "7px 10px" }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <span style={{ color: sigColor, minWidth: 56 }}>{ap.rssi} dBm</span>
                      <div style={{ width: 52, height: 5, borderRadius: 3, background: "rgba(255,255,255,0.08)", overflow: "hidden" }}>
                        <div style={{ height: "100%", width: `${ap.signal_quality}%`, borderRadius: 3, background: sigColor }} />
                      </div>
                      <span style={{ color: "var(--text-muted)", fontSize: 10 }}>{ap.signal_quality}%</span>
                    </div>
                  </td>
                  {/* Width - gerçek genişlik IE'den */}
                  <td style={{ padding: "7px 10px", color: "var(--text-dim)" }}>{ap.channel_width}</td>
                  {/* PHY */}
                  <td style={{ padding: "7px 10px", color: "var(--text-dim)", whiteSpace: "nowrap" }}>{ap.phy_type}</td>
                  {/* BSSID */}
                  <td style={{ padding: "7px 10px", color: "var(--text-muted)", letterSpacing: "0.5px", fontSize: 11 }}>{ap.bssid}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </>
  );
}

// ── Kanal Yüklenişi Yardımcısı ─────────────────────────────────────────────
// Her aday kanal için toplam parazit yükünü hesaplar, bar grafiği ile gösterir

interface ChannelLoadBarProps {
  band: string;
  aps: AccessPoint[];
  recommended: number;
  current: number;
}

function ChannelLoadBar({ band, aps, recommended, current }: ChannelLoadBarProps) {
  const candidateChannels: number[] =
    band === "5 GHz" ? NON_DFS_5 :
    band === "6 GHz" ? CHANNELS_6_PSC :
    NON_OVERLAPPING_24;

  // Kendi AP'lerimizi hariç tut (mesh / multi-band / sanal BSS) — karar Rust'ta (is_own)
  const externalAps = aps.filter(ap => !ap.is_own);

  const scores = candidateChannels.map((ch) => {
    const load = band === "2.4 GHz"
      ? externalAps.filter((ap) => ap.band === band && Math.abs(ap.channel - ch) <= 4)
             .reduce((s, ap) => s + ap.signal_quality, 0)
      : externalAps.filter((ap) => ap.band === band && ap.channel === ch)
             .reduce((s, ap) => s + ap.signal_quality, 0);
    return { ch, load };
  });

  const maxLoad = Math.max(...scores.map((s) => s.load), 1);

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      {scores.map(({ ch, load }) => {
        const isRec = ch === recommended;
        const isCur = ch === current;
        // Çakışma skoru: 100 = boş (yeşil), 0 = dolu (kırmızı)
        const overlapScore = maxLoad === 0 ? 100 : Math.max(0, 100 - Math.round((load / maxLoad) * 100));
        const pct = Math.min((load / maxLoad) * 100, 100); // bar doluluk
        const barColor = load === 0 ? "var(--success)" : overlapScoreColor(load, maxLoad);
        return (
          <div key={ch} style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{
              fontFamily: "var(--mono)", fontSize: 11, minWidth: 38, textAlign: "right",
              color: isRec ? "var(--success)" : isCur ? "var(--text)" : "var(--text-muted)",
              fontWeight: isRec || isCur ? 700 : 400,
            }}>
              CH {ch}
            </span>
            <div style={{ flex: 1, height: 8, borderRadius: 4, background: "rgba(255,255,255,0.07)", overflow: "hidden" }}>
              <div style={{
                height: "100%", width: `${pct}%`,
                borderRadius: 4,
                background: isRec && load === 0 ? "var(--success)" : barColor,
                opacity: 0.85,
                transition: "width 0.4s",
              }} />
            </div>
            {/* Çakışma skoru: 100=iyi, 0=kötü */}
            <span style={{ fontFamily: "var(--mono)", fontSize: 10, minWidth: 28, textAlign: "right", color: barColor, fontWeight: 600 }}>
              {overlapScore}
            </span>
            <div style={{ display: "flex", gap: 4, minWidth: 90 }}>
              {load === 0 && (
                <span style={{ fontSize: 9, padding: "1px 5px", borderRadius: 3, background: "rgba(16,185,129,0.15)", color: "var(--success)", fontWeight: 700 }}>{t("wifi_scan_channel_empty")}</span>
              )}
              {isRec && (
                <span style={{ fontSize: 9, padding: "1px 5px", borderRadius: 3, background: "rgba(16,185,129,0.2)", color: "var(--success)", border: "1px solid rgba(16,185,129,0.4)", fontWeight: 700 }}>{t("wifi_scan_channel_recommended")}</span>
              )}
              {isCur && !isRec && (
                <span style={{ fontSize: 9, padding: "1px 5px", borderRadius: 3, background: "rgba(255,255,255,0.08)", color: "var(--text-muted)", fontWeight: 600 }}>{t("wifi_scan_channel_current")}</span>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

// ── Kanal Optimizasyon Kartı ─────────────────────────────────────────────────
// bandFilter: seçili band (band filtresi ile senkron)
// Bağlı SSID olmasa da her aktif band için öneri gösterir.

interface OptimizationCardProps {
  result: WifiScanResult;
  bandFilter: string; // "All" | "2.4 GHz" | "5 GHz" | "6 GHz"
}

function ChannelOptimizationCard({ result, bandFilter }: OptimizationCardProps) {
  if (result.recommendations.length === 0) return null;

  // Hangi band'ın önerilerini göstereceğiz?
  // bandFilter "All" ise: bağlı bandı, o da yoksa ilk aktif bandı göster
  const activeBands = [...new Set(result.recommendations.map(r => r.band.split(" ")[0] + (r.band.includes("GHz") ? " GHz" : "")))];

  // band → bu banda ait öneriler
  // Rust "5 GHz", "5 GHz 40MHz", "5 GHz 80MHz", "5 GHz 160MHz" şeklinde dönüyor
  const recsFor = (b: string) =>
    result.recommendations.filter(r => {
      if (b === "2.4 GHz") return r.band === "2.4 GHz";
      if (b === "5 GHz") return r.band.startsWith("5 GHz");
      if (b === "6 GHz") return r.band.startsWith("6 GHz");
      return false;
    });

  // Gösterilecek band listesi: filtre varsa sadece o, yoksa tüm aktif bandlar
  const displayBands: string[] =
    bandFilter !== "All"
      ? [bandFilter]
      : (result.current_band
          ? [result.current_band, ...activeBands.filter(b => b !== result.current_band)]
          : activeBands);

  // Gösterilecek en az 1 band var mı?
  const hasAny = displayBands.some(b => recsFor(b).length > 0);
  if (!hasAny) return null;

  const isConnected = !!result.current_ssid;

  // Standart referans metinleri
  const stdNote = (band: string): { ref: string; detail: string } => {
    if (band === "2.4 GHz") return {
      ref: t("wifi_scan_note_24_ref"),
      detail: t("wifi_scan_note_24_detail"),
    };
    if (band === "5 GHz") return {
      ref: t("wifi_scan_note_5_ref"),
      detail: t("wifi_scan_note_5_detail"),
    };
    if (band === "6 GHz") return {
      ref: t("wifi_scan_note_6_ref"),
      detail: t("wifi_scan_note_6_detail"),
    };
    return { ref: "", detail: "" };
  };

  // Router ayar ipucu
  const howTo = (band: string, ch: number): string => {
    const bStr = band === "2.4 GHz" ? "2.4 GHz" : band === "5 GHz" ? "5 GHz" : "6 GHz";
    return t("wifi_scan_howto").replace("{band}", bStr).replace("{ch}", String(ch));
  };

  // BW badge rengi
  const bwColor = (bw: string): string => {
    if (bw.includes("160")) return "#8b5cf6";
    if (bw.includes("80"))  return "#3b82f6";
    if (bw.includes("40"))  return "#10b981";
    return "var(--text-muted)";
  };

  return (
    <div className="card" style={{ padding: "16px 18px" }}>
      {/* Başlık */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 16, flexWrap: "wrap" }}>
        <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-muted)", letterSpacing: "0.6px", textTransform: "uppercase" }}>
          {t("wifi_scan_optimization_title")}
        </div>
        {isConnected ? (
          <span style={{ fontSize: 11, color: "var(--text-dim)" }}>
            — <span style={{ color: "var(--text)", fontWeight: 600 }}>{result.current_ssid}</span>
            {" "}{t("wifi_scan_connected_on_channel")}{" "}
            <span style={{ fontFamily: "var(--mono)", color: "var(--text)", fontWeight: 700 }}>CH {result.current_channel}</span>
            <span style={{ marginLeft: 6, opacity: 0.6 }}>({result.current_band})</span>
          </span>
        ) : (
          <span style={{ fontSize: 11, color: "#f59e0b", background: "rgba(245,158,11,0.1)", padding: "1px 7px", borderRadius: 4, border: "1px solid rgba(245,158,11,0.25)" }}>
            {t("wifi_scan_no_connection_badge")}
          </span>
        )}
      </div>

      {/* Her band için öneri bloğu */}
      {displayBands.map(displayBand => {
        const bandRecs = recsFor(displayBand);
        if (bandRecs.length === 0) return null;
        const note = stdNote(displayBand);
        // Bağlı AP'yi BSSID eşleşmesiyle bul (aynı SSID'li çoklu AP/mesh durumunda doğru genişliği almak için)
        const connectedAp = result.access_points.find(ap => ap.is_connected);
        const currentWidth = connectedAp?.channel_width ?? "20 MHz";
        // Ana öneri: bağlı genişliğe denk gelen kartı seç (yoksa bandRecs[0] fallback)
        const mainRec = (displayBand === "5 GHz" || displayBand === "6 GHz")
          ? (bandRecs.find(r => {
              if (currentWidth.startsWith("160")) return r.band.includes("160");
              if (currentWidth.startsWith("80"))  return r.band.includes("80") && !r.band.includes("160");
              if (currentWidth.startsWith("40"))  return r.band.includes("40");
              return r.band === displayBand; // 20 MHz ana band kartı
            }) ?? bandRecs[0])
          : bandRecs[0];
        const recWidth = mainRec.band.includes("160") ? "160 MHz"
          : mainRec.band.includes("80") ? "80 MHz"
          : mainRec.band.includes("40") ? "40 MHz" : "20 MHz";
        const isOptimal = mainRec.current_channel > 0 && mainRec.current_channel === mainRec.recommended_channel;
        const color = congestionColor(mainRec.congestion_score);
        // BW alternatifleri: 40/80/160 MHz recs
        const bwRecs = bandRecs.slice(1);

        return (
          <div key={displayBand} style={{ marginBottom: 20 }}>
            {/* Band başlığı */}
            <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 10 }}>
              <span style={{ fontSize: 11, fontWeight: 700, padding: "2px 8px", borderRadius: 4, background: "rgba(59,130,246,0.15)", color: "#3b82f6" }}>
                {displayBand}
              </span>
              {displayBands.length > 1 && displayBand === result.current_band && (
                <span style={{ fontSize: 10, color: "var(--success)", opacity: 0.8 }}>{t("wifi_scan_current_band_label")}</span>
              )}
            </div>

            {isOptimal ? (
              <div style={{ display: "flex", alignItems: "flex-start", gap: 10, padding: "10px 14px", borderRadius: 8, background: "rgba(16,185,129,0.08)", border: "1px solid rgba(16,185,129,0.2)", marginBottom: 8 }}>
                <span style={{ fontSize: 14 }}>✓</span>
                <div>
                  <div style={{ fontSize: 13, color: "var(--success)", fontWeight: 600 }}>{t("wifi_scan_already_optimal")}</div>
                  <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>{t("wifi_scan_optimal_desc").replace("{ch}", String(mainRec.current_channel))}</div>
                </div>
              </div>
            ) : (
              <>
                {/* Mevcut → Önerilen (20 MHz) */}
                <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 14px", borderRadius: 8, background: "rgba(255,255,255,0.04)", border: "1px solid var(--border)", marginBottom: 8, flexWrap: "wrap" }}>
                  {mainRec.current_channel > 0 ? (
                    <>
                      <div style={{ textAlign: "center", minWidth: 48 }}>
                        <div style={{ fontSize: 8, color: "var(--text-muted)", textTransform: "uppercase", marginBottom: 2 }}>{t("wifi_scan_label_current")}</div>
                        <div style={{ fontSize: 24, fontWeight: 700, fontFamily: "var(--mono)", color: "var(--text)", lineHeight: 1 }}>{mainRec.current_channel}</div>
                        <div style={{ fontSize: 8, color, marginTop: 2 }}>{congestionLabel(mainRec.congestion_score)}</div>
                      </div>
                      <div style={{ fontSize: 18, color: "var(--text-muted)" }}>→</div>
                    </>
                  ) : (
                    <div style={{ fontSize: 11, color: "var(--text-muted)" }}>{t("wifi_scan_recommended_label")}</div>
                  )}
                  <div style={{ textAlign: "center", minWidth: 48 }}>
                    <div style={{ fontSize: 8, color: "var(--text-muted)", textTransform: "uppercase", marginBottom: 2 }}>
                      {mainRec.current_channel > 0 ? t("wifi_scan_label_recommended") : t("wifi_scan_label_best")}
                    </div>
                    <div style={{ fontSize: 24, fontWeight: 700, fontFamily: "var(--mono)", color: "var(--success)", lineHeight: 1 }}>{mainRec.recommended_channel}</div>
                    <div style={{ fontSize: 8, color: "var(--success)", marginTop: 2 }}>{recWidth}</div>
                  </div>
                  {mainRec.current_channel > 0 && (
                    <div style={{ flex: 1, minWidth: 140 }}>
                      <div style={{ fontSize: 9, color: "var(--text-muted)", marginBottom: 4 }}>{t("wifi_scan_congestion_score")}</div>
                      <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
                        <div style={{ flex: 1, height: 5, borderRadius: 3, background: "rgba(255,255,255,0.08)", overflow: "hidden" }}>
                          <div style={{ height: "100%", width: `${Math.min(mainRec.congestion_score, 100)}%`, borderRadius: 3, background: color }} />
                        </div>
                        <span style={{ fontSize: 11, color, fontWeight: 700, fontFamily: "var(--mono)" }}>{mainRec.congestion_score}</span>
                      </div>
                    </div>
                  )}
                </div>

                {/* BW Alternatifleri (sadece 5/6 GHz) */}
                {bwRecs.length > 0 && (
                  <div style={{ padding: "10px 14px", borderRadius: 8, background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)", marginBottom: 8 }}>
                    <div style={{ fontSize: 9, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: 8 }}>
                      {t("wifi_scan_bw_alternatives_title")}
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                      {bwRecs.map(r => {
                        const bw = r.band.includes("160") ? "160 MHz" : r.band.includes("80") ? "80 MHz" : "40 MHz";
                        const bc = bwColor(bw);
                        const isOpt = r.current_channel > 0 && r.current_channel === r.recommended_channel;
                        return (
                          <div key={r.band} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                            <span style={{ fontSize: 9, fontWeight: 700, padding: "1px 6px", borderRadius: 3, background: "rgba(255,255,255,0.06)", color: bc, minWidth: 52, textAlign: "center" }}>
                              {bw}
                            </span>
                            <span style={{ fontSize: 11, fontFamily: "var(--mono)", color: "var(--success)", fontWeight: 700 }}>CH {r.recommended_channel}</span>
                            {isOpt
                              ? <span style={{ fontSize: 10, color: "var(--success)" }}>{t("wifi_scan_bw_optimal")}</span>
                              : <span style={{ fontSize: 10, color: "var(--text-muted)" }}>{t("wifi_scan_bw_least_congested")}</span>
                            }
                          </div>
                        );
                      })}
                    </div>
                  </div>
                )}

                {/* Kanal Yük Analizi */}
                <div style={{ padding: "10px 14px", borderRadius: 8, background: "rgba(255,255,255,0.025)", border: "1px solid var(--border)", marginBottom: 8 }}>
                  <div style={{ fontSize: 9, fontWeight: 600, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: 8 }}>
                    {t("wifi_scan_load_analysis_title")}
                  </div>
                  <ChannelLoadBar
                    band={displayBand}
                    aps={result.access_points}
                    recommended={mainRec.recommended_channel}
                    current={mainRec.current_channel}
                  />
                </div>
              </>
            )}

            {/* Standart referansı */}
            <div style={{ padding: "8px 12px", borderRadius: 8, background: "rgba(99,102,241,0.06)", border: "1px solid rgba(99,102,241,0.15)", marginBottom: 6 }}>
              {note.ref && (
                <div style={{ fontSize: 8, fontWeight: 700, color: "rgba(99,102,241,0.8)", textTransform: "uppercase", letterSpacing: "0.5px", marginBottom: 4 }}>
                  {note.ref}
                </div>
              )}
              <div style={{ fontSize: 11, color: "var(--text-dim)", lineHeight: 1.6 }}>{note.detail}</div>
            </div>

            {/* Router kılavuzu */}
            {!isOptimal && (
              <div style={{ padding: "8px 12px", borderRadius: 8, fontSize: 11, background: "rgba(16,185,129,0.05)", border: "1px solid rgba(16,185,129,0.18)", color: "var(--text-dim)", lineHeight: 1.6 }}>
                <Icon.Idea style={{ width: 13, height: 13, verticalAlign: "-2px", marginRight: 4, color: "var(--success)" }} />{howTo(displayBand, mainRec.recommended_channel)}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}



// ── Ana Component ─────────────────────────────────────────────────────────────

/**
 * Wi-Fi Kalitesi karti (kompakt) — Gosterge Paneli'nden bu modulun en ustune tasindi.
 * Bagli olunan Wi-Fi'nin sinyal/band/kanal/protokol/hiz ozeti. Taramadan bagimsiz.
 * Tek satir grid + sikistirilmis bosluklar (dikey yer ~yariya indirildi).
 */
function WifiQualityCard({ wifi }: { wifi: WifiInfo | null }) {
  if (!wifi) return null;
  return (
    <div className="card" style={{ padding: 14 }}>
      <div className="card-head" style={{ marginBottom: wifi.connected ? 12 : 0 }}>
        <div className="ico-wrap"><Icon.Wifi /></div>
        <h3>{t("net_wifi_quality")}</h3>
        <span className="head-sub">{wifi.connected ? t("net_connected") : t("net_disconnected")}</span>
      </div>

      {!wifi.connected ? (
        <p style={{ color: "var(--text-muted)", fontSize: 13, margin: 0 }}>{t("net_no_wifi")}</p>
      ) : (
        <>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", marginBottom: 8 }}>
            <div style={{ minWidth: 0 }}>
              <p style={{ fontSize: 15, fontWeight: 700, color: "var(--text)", margin: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{wifi.ssid}</p>
              <p className="mono" style={{ fontSize: 10, color: "var(--text-muted)", marginTop: 1 }}>{wifi.bssid}</p>
            </div>
            <div style={{ textAlign: "right", flexShrink: 0, marginLeft: 12 }}>
              <p className="mono" style={{ fontSize: 20, fontWeight: 700, color: signalColor(wifi.signal_percent), margin: 0 }}>{wifi.signal_percent}%</p>
              <p style={{ fontSize: 10, color: "var(--text-muted)", margin: 0 }}>{wifi.signal_dbm} dBm</p>
            </div>
          </div>

          <div style={{ height: 6, borderRadius: 999, background: "rgba(255,255,255,0.04)", overflow: "hidden", border: "1px solid rgba(255,255,255,0.03)", marginBottom: 12 }}>
            <div style={{
              width: `${wifi.signal_percent}%`, height: "100%", borderRadius: 999,
              background: `linear-gradient(90deg, ${signalColor(wifi.signal_percent)}, color-mix(in oklab, ${signalColor(wifi.signal_percent)} 60%, white))`,
              boxShadow: `0 0 10px ${signalColor(wifi.signal_percent)}80`,
              transition: "width 800ms cubic-bezier(0.22,1,0.36,1)",
            }} />
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "repeat(6, 1fr)", gap: 8 }}>
            {[
              [t("net_band"),     wifi.band],
              [t("net_channel"),  wifi.channel > 0 ? String(wifi.channel) : "—"],
              [t("net_protocol"), wifi.phy_type],
              [t("net_security"), wifi.auth || "—"],
              [t("net_tx"), wifi.tx_rate || (wifi.rx_rate ? wifi.rx_rate : "—")],
              [t("net_rx"), wifi.rx_rate || (wifi.tx_rate ? wifi.tx_rate : "—")],
            ].map(([label, val]) => (
              <div key={label} style={{ padding: "6px 8px", borderRadius: 8, background: "rgba(10,10,15,0.5)", border: "1px solid var(--border)", minWidth: 0 }}>
                <p style={{ fontSize: 9, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.08em", margin: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</p>
                <p className="mono" style={{ fontSize: 12, fontWeight: 500, color: "var(--text)", marginTop: 2, marginBottom: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{val}</p>
              </div>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

export function WifiScanTab() {
  useLang();
  const [wifi, setWifi] = useState<WifiInfo | null>(null);
  const [result, setResult] = useState<WifiScanResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [bandFilter, setBandFilter] = useState<string>("All");
  const [error, setError] = useState<string | null>(null);
  // Auto-scan toggle - IEEE 802.11k/v pasif tarama budget'a uygun 30s aralık
  // İlk tarama tamamlandıktan sonra default ON
  const [autoScan, setAutoScan] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [countdown, setCountdown] = useState(0);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  // Silent rescan bildirimi
  const [scanStatus, setScanStatus] = useState<"idle" | "scanning" | "done">("idle");
  const scanStatusTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Sessiz (arka plan/rescan) taramanin hatasi: mevcut sonuclari SILMEDEN kucuk bir not olarak gosterilir.
  const [scanNote, setScanNote] = useState<string | null>(null);
  // Ust uste binen taramalari (otomatik + manuel) senkron engeller.
  const scanInFlightRef = useRef(false);
  // Stale BSSID'ler (cache'de var ama son taramada görülmedi)
  const [staleBssids, setStaleBssids] = useState<Set<string>>(new Set());

  // ── AP Cache ──────────────────────────────────────────────────────────
  // Windows WLAN API sadece son taramada görünen AP'leri döner.
  // Zayıf sinyalli AP'ler bir sonraki taramada eksik olabilir.
  // Çözüm: BSSID bazlı cache - görülmeyen AP'leri "stale" olarak işaretle,
  // 3 tarama (90 saniye) görülmezse tamamen düşür.
  // IEEE 802.11-2020 §11.2.3.2: BSS cache entry max idle time ∼ 90s
  const STALE_EVICT_COUNT = 3; // kaç tarama görülmezse at
  type CachedAP = AccessPoint & { missCount: number; stale: boolean };
  const apCacheRef = useRef<Map<string, CachedAP>>(new Map());
  const scanConnectionRef = useRef<Pick<WifiScanResult, "current_ssid" | "current_bssid"> | null>(null);
  const [verdict, setVerdict] = useState<ModuleVerdict | null>(null);

  useEffect(() => {
    if (!result) { setVerdict(null); return; }
    invoke<ModuleVerdict>("run_module_verdict", {
      module: "wifi",
      input: {
        nic: {}, wifi: wifi ?? {}, ping_results: [], bufferbloat: {},
        speed_download: null, speed_upload: null,
        throttle: null, isp_intervention: null, ipv6: null, cgnat: null, mtu: null, voip: null,
        wifi_scan: result,
      },
    }).then(setVerdict).catch(() => setVerdict(null));
  }, [result, wifi]); // eslint-disable-line react-hooks/exhaustive-deps

  const mergeWithCache = (fresh: AccessPoint[]): CachedAP[] => {
    const cache = apCacheRef.current;
    const freshBssids = new Set(fresh.map(ap => ap.bssid));

    // Güncel görünenleri sıfırla / güncelle
    for (const ap of fresh) {
      cache.set(ap.bssid, { ...ap, missCount: 0, stale: false });
    }

    // Görülmeyenlerin missCount'unu artır
    for (const [bssid, cached] of cache.entries()) {
      if (!freshBssids.has(bssid)) {
        cached.missCount++;
        cached.stale = true;
        if (cached.missCount >= STALE_EVICT_COUNT) {
          cache.delete(bssid);
        }
      }
    }

    // Stale AP'ler sonda, RSSI'ya göre sırala
    return Array.from(cache.values()).sort((a, b) => {
      if (a.stale !== b.stale) return a.stale ? 1 : -1;
      return b.rssi - a.rssi;
    });
  };

  const handleScan = async (silent = false) => {
    if (loading || scanInFlightRef.current) return;
    scanInFlightRef.current = true;
    if (silent) {
      setScanStatus("scanning");
    } else {
      setLoading(true);
    }
    setError(null);
    setScanNote(null);
    try {
      const res = await invoke<WifiScanResult>("run_wifi_scan");
      // Baglanti bilgisi HER taramayla yenilenir; sonuc + bilgi ayni anda state'e yazilir
      // (verdict ayni goruntuden hesaplanir). Okunamazsa eski bilgi tutulmaz.
      const info = await invoke<WifiInfo>("get_wifi_info").catch(() => null);
      // is_own taramadaki baglantiya aittir; baglanti degisince eski AP'leri tasima.
      const previousConnection = scanConnectionRef.current;
      if (previousConnection?.current_ssid !== res.current_ssid ||
          previousConnection?.current_bssid !== res.current_bssid) {
        apCacheRef.current.clear();
      }
      scanConnectionRef.current = { current_ssid: res.current_ssid, current_bssid: res.current_bssid };
      // Cache merge: ayni baglantida stale AP'leri koru, yenileri ekle
      const mergedAps = mergeWithCache(res.access_points);
      const mergedResult: WifiScanResult = {
        ...res,
        access_points: mergedAps.map(({ missCount: _m, stale: _s, ...ap }) => ap),
      };
      setResult(mergedResult);
      setWifi(info);
      // Grafik için stale bilgisini ayrı tut
      setStaleBssids(new Set(
        mergedAps.filter(ap => ap.stale).map(ap => ap.bssid)
      ));
      if (!silent) setBandFilter("All");
      if (silent) {
        setScanStatus("done");
        if (scanStatusTimerRef.current) clearTimeout(scanStatusTimerRef.current);
        scanStatusTimerRef.current = setTimeout(() => setScanStatus("idle"), 2500);
      }
    } catch (e) {
      const msg = typeof e === "string" ? e : t("wifi_scan_error_fallback");
      if (!silent) setError(msg);
      else setScanNote(msg);
    } finally {
      scanInFlightRef.current = false;
      if (!silent) setLoading(false);
      // Basarisiz sessiz tarama 'scanning'de takili kalip Rescan'i kilitlemesin; basarida 'done' korunur.
      else setScanStatus(prev => (prev === "scanning" ? "idle" : prev));
    }
  };

  const startAutoScanLoop = () => {
    if (intervalRef.current) clearInterval(intervalRef.current);
    if (countdownRef.current) clearInterval(countdownRef.current);
    setCountdown(AUTO_SCAN_INTERVAL_MS / 1000);
    intervalRef.current = setInterval(async () => {
      await handleScan(true); // auto taramalar silent
      setCountdown(AUTO_SCAN_INTERVAL_MS / 1000);
    }, AUTO_SCAN_INTERVAL_MS);
    countdownRef.current = setInterval(() => {
      setCountdown((c) => (c > 1 ? c - 1 : 0));
    }, 1000);
  };

  const stopAutoScan = () => {
    if (intervalRef.current) { clearInterval(intervalRef.current); intervalRef.current = null; }
    if (countdownRef.current) { clearInterval(countdownRef.current); countdownRef.current = null; }
    setCountdown(0);
  };

  const toggleAutoScan = async () => {
    const next = !autoScan;
    setAutoScan(next);
    if (next) {
      await handleScan(false);
      startAutoScanLoop();
    } else {
      stopAutoScan();
    }
  };

  // Unmount temizliği
  // Bagli Wi-Fi kalite ozetini mount'ta cek (Wi-Fi Kalitesi karti icin).
  // Self-contained: App'ten prop almaz, get_wifi_info komutunu kendi cagirir.
  useEffect(() => {
    let cancelled = false;
    invoke<WifiInfo>("get_wifi_info")
      .then((w) => { if (!cancelled) setWifi(w); })
      .catch(() => { if (!cancelled) setWifi(null); });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
      if (countdownRef.current) clearInterval(countdownRef.current);
      if (scanStatusTimerRef.current) clearTimeout(scanStatusTimerRef.current);
    };
  }, []);

  // ── Idle (ortak pat1 kalibi) ──
  if (!loading && result === null && error === null) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <TestIntro
          iconKey="WifiChannel"
          title={t("wifi_scan_idle_title")}
          description={t("wifi_scan_intro_desc")}
          buttonLabel={t("wifi_scan_btn_scan")}
          onRun={() => {
            // İlk tarama: normal mod (loading ekranı), bittikten sonra auto-scan ON
            handleScan(false).then(() => {
              setAutoScan(true);
              startAutoScanLoop();
            });
          }}
        />
      </div>
    );
  }

  // ── Loading (sadece ilk tarama - sonrakiler silent) ──
  if (loading) {
    return (
      <div className="card" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 20, padding: "56px 32px" }}>
        <div style={{ width: 44, height: 44, borderRadius: "50%", border: "3px solid rgba(255,255,255,0.08)", borderTop: "3px solid var(--accent)", animation: "spin 0.9s linear infinite" }} />
        <div style={{ fontSize: 14, color: "var(--text-muted)" }}>{t("wifi_scan_btn_scanning")}</div>
        <div style={{ fontSize: 12, color: "var(--text-muted)", opacity: 0.6 }}>{t("wifi_scan_loading_hint")}</div>
      </div>
    );
  }

  // ── Hata ──
  if (error !== null) {
    return (
      <div className="card" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 14, padding: "48px 32px", textAlign: "center" }}>
        <div style={{ fontSize: 16, fontWeight: 700, color: "#ef4444" }}>{t("wifi_scan_failed")}</div>
        <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 400 }}>{error}</div>
        <button className="btn" onClick={() => handleScan(false)} style={{ marginTop: 4 }}>
          <Icon.Refresh />
          {t("wifi_scan_retry_btn")}
        </button>
      </div>
    );
  }

  // ── No adapter ──
  if (result && !result.interface_found) {
    return (
      <div className="card" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 12, padding: "48px 32px", textAlign: "center" }}>
        <div style={{ opacity: 0.5, width: 40, height: 40 }}><Icon.Wifi style={{ width: 40, height: 40 }} /></div>
        <div style={{ fontSize: 16, fontWeight: 700, color: "var(--text)" }}>{t("wifi_scan_no_adapter")}</div>
        <div style={{ fontSize: 13, color: "var(--text-muted)" }}>{t("wifi_scan_no_adapter_desc")}</div>
      </div>
    );
  }

  if (!result) return null;

  // ── Sonuçlar ──
  const bands = ["All", "2.4 GHz", "5 GHz", "6 GHz"];
  const activeBands = new Set(result.access_points.map((ap) => ap.band));

  const filteredAps = bandFilter === "All"
    ? result.access_points
    : result.access_points.filter((ap) => ap.band === bandFilter);

  // Harita için hangi band: filtre seçildiyse o, yoksa bağlı AP'nin bandı, yoksa ilk aktif band
  const mapBand = bandFilter !== "All"
    ? bandFilter
    : result.current_band || (activeBands.has("2.4 GHz") ? "2.4 GHz" : activeBands.has("5 GHz") ? "5 GHz" : "6 GHz");

  const mapAps = result.access_points.filter((ap) => ap.band === mapBand);

  const channelsForBand = (b: string): number[] => {
    if (b === "5 GHz") return CHANNELS_5;
    if (b === "6 GHz") return CHANNELS_6_PSC;
    return CHANNELS_24;
  };
  const nonOverlappingForBand = (b: string): number[] => {
    if (b === "5 GHz") return NON_DFS_5;
    if (b === "6 GHz") return CHANNELS_6_PSC;
    return NON_OVERLAPPING_24;
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {/* Wi-Fi Kalitesi (Gosterge Paneli'nden tasindi) — modulun en ustunde */}
      <WifiQualityCard wifi={wifi} />

      {/* Smart Grade */}
      {verdict && verdict.available && (
        <GradeCard
          grade={verdict.grade}
          comment={t(verdict.detail_key as Parameters<typeof t>[0])}
        />
      )}

      {/* ── Band Filtresi + Rescan ── */}
      <div className="card" style={{ padding: "10px 14px", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
        {bands.map((b) => {
          const isAll = b === "All";
          const count = isAll ? result.access_points.length : result.access_points.filter((ap) => ap.band === b).length;
          const disabled = !isAll && count === 0;
          const active = bandFilter === b;
          return (
            <button
              key={b}
              disabled={disabled}
              onClick={() => setBandFilter(b)}
              style={{
                padding: "4px 12px", borderRadius: 6, fontSize: 12,
                fontWeight: active ? 700 : 500,
                cursor: disabled ? "not-allowed" : "pointer",
                opacity: disabled ? 0.35 : 1,
                background: active ? "var(--accent)" : "rgba(255,255,255,0.06)",
                border: active ? "1px solid transparent" : "1px solid var(--border)",
                color: active ? "#fff" : "var(--text-dim)",
                transition: "all 0.15s",
              }}
            >
              {isAll ? t("wifi_scan_band_filter_all") : b}
              {!disabled && <span style={{ marginLeft: 5, opacity: 0.65, fontSize: 10 }}>({count})</span>}
            </button>
          );
        })}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 12, color: "var(--text-muted)" }}>
            {filteredAps.length} {t("wifi_scan_networks_count")}
          </span>
          {/* Auto-scan toggle - 30s aralık, IEEE 802.11k pasif scan budget */}
          <button
            onClick={() => toggleAutoScan()}
            style={{
              display: "flex", alignItems: "center", gap: 6,
              padding: "4px 12px", borderRadius: 6, fontSize: 12,
              fontWeight: autoScan ? 700 : 500,
              cursor: loading ? "not-allowed" : "pointer",
              opacity: loading && !autoScan ? 0.6 : 1,
              background: autoScan ? "rgba(16,185,129,0.15)" : "rgba(255,255,255,0.06)",
              border: autoScan ? "1px solid rgba(16,185,129,0.4)" : "1px solid var(--border)",
              color: autoScan ? "var(--success)" : "var(--text-dim)",
              transition: "all 0.2s",
            }}
          >
            {/* Toggle pill */}
            <span style={{
              display: "inline-flex", width: 28, height: 15, borderRadius: 8,
              background: autoScan ? "rgba(16,185,129,0.6)" : "rgba(255,255,255,0.15)",
              alignItems: "center", padding: "0 2px",
              transition: "background 0.2s",
            }}>
              <span style={{
                width: 11, height: 11, borderRadius: "50%", background: "#fff",
                transition: "transform 0.2s",
                transform: autoScan ? "translateX(13px)" : "translateX(0px)",
              }} />
            </span>
            {autoScan ? (
              <span>{t("wifi_scan_auto_active").replace("{countdown}", String(countdown))}</span>
            ) : t("wifi_scan_auto_scan_label")}
          </button>
          {/* Manuel rescan - silent mod, sayfa yüklenmiyor */}
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            {scanStatus === "scanning" && (
              <span style={{ fontSize: 11, color: "var(--text-muted)", display: "flex", alignItems: "center", gap: 4 }}>
                <span style={{ width: 8, height: 8, borderRadius: "50%", border: "1.5px solid rgba(255,255,255,0.15)", borderTop: "1.5px solid var(--accent)", animation: "spin 0.8s linear infinite", display: "inline-block" }} />
                {t("wifi_scan_status_scanning")}
              </span>
            )}
            {scanStatus === "done" && (
              <span style={{ fontSize: 11, color: "var(--success)", display: "flex", alignItems: "center", gap: 3 }}>
                {t("wifi_scan_status_updated")}
              </span>
            )}
            {scanNote && (
              <span title={scanNote} style={{ fontSize: 11, color: "#fca5a5", maxWidth: 260, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {t("wifi_scan_failed")}: {scanNote}
              </span>
            )}
            <button
              className="btn btn-secondary"
              onClick={() => handleScan(true)}
              disabled={scanStatus === "scanning"}
              style={{ padding: "4px 12px", fontSize: 12, opacity: scanStatus === "scanning" ? 0.5 : 1 }}
            >
              <Icon.Refresh style={{ width: 12, height: 12 }} />
              {t("wifi_scan_rescan_btn")}
            </button>
          </div>
        </div>
      </div>

      {/* ── Kanal Haritası ── */}
      <div className="card" style={{ padding: "14px 16px" }}>
        <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-muted)", letterSpacing: "0.6px", textTransform: "uppercase", marginBottom: 4 }}>
          {t("wifi_scan_channel_map_title")}
          <span style={{ marginLeft: 8, fontWeight: 400, textTransform: "none", opacity: 0.7 }}>{mapBand}</span>
        </div>
        {mapAps.length === 0 ? (
          <div style={{ textAlign: "center", padding: "32px 0", color: "var(--text-muted)", fontSize: 13 }}>
            {t("wifi_scan_no_networks")}
          </div>
        ) : (
          <GaussianChannelMap
            aps={mapAps}
            channels={channelsForBand(mapBand)}
            nonOverlapping={nonOverlappingForBand(mapBand)}
            connectedSsid={result.current_ssid}
            band={mapBand}
            staleBssids={staleBssids}
          />
        )}
      </div>

      {/* ── Tespit Edilen Ağlar ── */}
      <div className="card" style={{ padding: "14px 16px" }}>
        <div style={{ fontSize: 12, fontWeight: 700, color: "var(--text-muted)", letterSpacing: "0.6px", textTransform: "uppercase", marginBottom: 12 }}>
          {t("wifi_scan_networks_title")}
        </div>
        {filteredAps.length === 0 ? (
          <div style={{ textAlign: "center", padding: "24px 0", color: "var(--text-muted)", fontSize: 13 }}>
            {t("wifi_scan_no_networks")}
          </div>
        ) : (
          <NetworkTable aps={filteredAps} connectedSsid={result.current_ssid} staleBssids={staleBssids} />
        )}
      </div>

      {/* ── Kanal Optimizasyonu ── */}
      {result.recommendations.length > 0 && (
        <ChannelOptimizationCard result={result} bandFilter={bandFilter} />
      )}
    </div>
  );
}

/**
 * PDF Rapor Export — jsPDF (MIT) + Inter font (OFL, Turkce/Latin-ext tam).
 * Koyu tema (uygulama paleti). 3 bolumlu yapi:
 *   Sayfa 1  : Modul harf puanlama tablosu (ozet)
 *   Sayfa 2  : Genel sentez (pozitif / notr / negatif)
 *   Sayfa 3+ : Her olculen modul icin 1 sayfa (ham veri + yorum)
 *
 * Faz 3a: iskelet + ham veri + mevcut (grade-bazli) yorum.
 * Faz 3b+: Smart Engine zengin yorumu (pozitif/notr/negatif + buyuk resim).
 */
import { jsPDF } from "jspdf";
import { save } from "@tauri-apps/plugin-dialog";
import { writeFile } from "@tauri-apps/plugin-fs";
import type { SmartDiagnosis, ModuleVerdict } from "../types";
import type { TestResults } from "../App";
import { t } from "./i18n";
import { INTER_400_B64, INTER_700_B64 } from "./interFont";

const tk = (key: string) => t(key as Parameters<typeof t>[0]);

type RGB = [number, number, number];
const C = {
  bg: [10, 10, 15] as RGB,
  card: [24, 24, 38] as RGB,
  cardDim: [16, 16, 26] as RGB,
  ink: [245, 247, 250] as RGB,
  text: [200, 208, 220] as RGB,
  muted: [150, 155, 170] as RGB,
  accent: [129, 132, 245] as RGB,
  accent2: [159, 122, 255] as RGB,
  line: [58, 58, 84] as RGB,
  white: [255, 255, 255] as RGB,
  pos: [52, 211, 153] as RGB,
  neu: [250, 204, 21] as RGB,
  neg: [248, 113, 113] as RGB,
};

const GRADE_RGB: Record<string, RGB> = {
  "A+": [16, 185, 129], A: [34, 197, 94], B: [59, 130, 246],
  C: [202, 138, 4], D: [234, 88, 12], F: [239, 68, 68], "-": [150, 150, 160],
};
const STATUS_RGB: Record<string, RGB> = {
  excellent: [16, 185, 129], good: [34, 197, 94], fair: [202, 138, 4],
  poor: [234, 88, 12], critical: [239, 68, 68],
};

/** Grade -> pozitif / notr / negatif sinifi. */
function sentiment(grade: string): "pos" | "neu" | "neg" {
  if (grade === "A+" || grade === "A" || grade === "B") return "pos";
  if (grade === "C" || grade === "D") return "neu";
  return "neg"; // F veya "-"
}

let fontLoaded = false;
function ensureFont(doc: jsPDF) {
  if (fontLoaded) return;
  doc.addFileToVFS("Inter-400.ttf", INTER_400_B64);
  doc.addFont("Inter-400.ttf", "Inter", "normal");
  doc.addFileToVFS("Inter-700.ttf", INTER_700_B64);
  doc.addFont("Inter-700.ttf", "Inter", "bold");
  fontLoaded = true;
}

function moduleName(m: ModuleVerdict): string {
  return tk(`diag_mod_${m.module}`);
}

// ── Cizim yardimcilari ──────────────────────────────────────────────────
function pageBg(doc: jsPDF, pageW: number, pageH: number) {
  doc.setFillColor(...C.bg);
  doc.rect(0, 0, pageW, pageH, "F");
}

function header(doc: jsPDF, pageW: number, subtitle: string) {
  doc.setFillColor(...C.accent);
  doc.rect(0, 0, pageW, 84, "F");
  doc.setFillColor(...C.accent2);
  doc.rect(0, 80, pageW, 4, "F");
  doc.setFont("Inter", "bold"); doc.setFontSize(20); doc.setTextColor(...C.white);
  doc.text("NetVeil", 44, 40);
  doc.setFont("Inter", "normal"); doc.setFontSize(11);
  doc.text(subtitle, 44, 60);
  doc.setFontSize(9);
  doc.text(new Date().toLocaleString(), pageW - 44, 40, { align: "right" });
}

function footer(doc: jsPDF, pageW: number, pageH: number) {
  const n = doc.getNumberOfPages();
  for (let i = 1; i <= n; i++) {
    doc.setPage(i);
    doc.setDrawColor(...C.line);
    doc.line(44, pageH - 34, pageW - 44, pageH - 34);
    doc.setFont("Inter", "normal"); doc.setFontSize(8); doc.setTextColor(...C.muted);
    doc.text("netveil.net · NetVeil", 44, pageH - 20);
    doc.text(`${i} / ${n}`, pageW - 44, pageH - 20, { align: "right" });
  }
}

/** Ping hedef tablosu (latency/jitter/loss sayfalari icin). Yeni y doner. */
function renderPingTargets(
  doc: jsPDF, pings: TestResults["pings"], M: number, pageW: number, startY: number,
): number {
  let y = startY;
  if (!pings || pings.length === 0) return y;
  // Tum hedefleri basma — her kategoriden ulasilabilir en iyi (en dusuk avg) birkac tane.
  const PER_CAT = 5;
  const pick = (cat: string) =>
    pings
      .filter(p => p.category === cat && p.reachable)
      .sort((a, b) => a.avg_ms - b.avg_ms)
      .slice(0, PER_CAT);
  const selected = [...pick("services"), ...pick("games")];
  // Hicbiri ulasilamadiysa (hepsi unreachable) ilk birkacini yine de goster.
  const shown = selected.length > 0 ? selected : pings.slice(0, 6);

  doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
  doc.text(t("pdf_lbl_targets").toUpperCase(), M, y);
  y += 16;
  // Baslik satiri
  doc.setFont("Inter", "bold"); doc.setFontSize(9); doc.setTextColor(...C.muted);
  const colName = M + 12, colAvg = M + 210, colJit = M + 290, colLoss = M + 370, colGrade = pageW - M - 14;
  doc.text(t("pdf_lbl_targets"), colName, y);
  doc.text("avg", colAvg, y); doc.text("jitter", colJit, y); doc.text("loss", colLoss, y);
  doc.text(t("pdf_lbl_grade"), colGrade, y, { align: "right" });
  y += 6;
  doc.setDrawColor(...C.line); doc.setLineWidth(0.8); doc.line(M, y, pageW - M, y);
  y += 14;
  doc.setFont("Inter", "normal"); doc.setFontSize(9.5);
  for (const p of shown) {
    const g = GRADE_RGB[p.grade] ?? C.muted;
    doc.setTextColor(...C.ink);
    doc.text(`${p.name}`, colName, y);
    doc.setTextColor(...C.muted); doc.setFontSize(8);
    doc.text(`${p.region || p.host}`, colName, y + 10);
    doc.setFontSize(9.5); doc.setTextColor(...C.text);
    if (p.reachable) {
      doc.text(`${p.avg_ms.toFixed(0)} ms`, colAvg, y);
      doc.text(`${p.jitter_ms.toFixed(1)}`, colJit, y);
      doc.text(`${p.packet_loss.toFixed(1)}%`, colLoss, y);
    } else {
      doc.setTextColor(...C.muted);
      doc.text(t("pdf_unreachable"), colAvg, y);
    }
    doc.setTextColor(...g); doc.setFont("Inter", "bold");
    doc.text(p.reachable ? p.grade : "—", colGrade, y, { align: "right" });
    doc.setFont("Inter", "normal");
    y += 22;
  }
  return y + 8;
}

/** Bufferbloat segment tablosu + idle/loaded karsilastirma grafigi. Yeni y doner. */
function renderBufferbloat(
  doc: jsPDF, bb: NonNullable<TestResults["bufferbloat"]>, M: number, contentW: number, pageW: number, startY: number,
): number {
  let y = startY;
  // Segment tablosu
  doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
  doc.text(t("pdf_lbl_segments").toUpperCase(), M, y);
  y += 16;
  doc.setFont("Inter", "bold"); doc.setFontSize(9); doc.setTextColor(...C.muted);
  const c0 = M + 12, c1 = M + 150, c2 = M + 250, c3 = M + 350, c4 = pageW - M - 14;
  doc.text(t("pdf_lbl_segment"), c0, y);
  doc.text(t("pdf_lbl_idle"), c1, y); doc.text(t("pdf_lbl_loaded"), c2, y);
  doc.text("Δ", c3, y); doc.text(t("pdf_lbl_grade"), c4, y, { align: "right" });
  y += 6; doc.setDrawColor(...C.line); doc.setLineWidth(0.8); doc.line(M, y, pageW - M, y); y += 14;
  doc.setFont("Inter", "normal"); doc.setFontSize(9.5);
  for (const s of bb.segments) {
    const g = GRADE_RGB[s.grade] ?? C.muted;
    const label = s.target === "gateway" ? "PC ↔ Router" : s.target === "wan" ? "PC ↔ WAN" : s.target;
    doc.setTextColor(...C.ink); doc.text(label, c0, y);
    doc.setTextColor(...C.text);
    doc.text(`${s.idle_p95_ms.toFixed(0)} ms`, c1, y);
    doc.text(`${s.loaded_p95_ms.toFixed(0)} ms`, c2, y);
    doc.text(`+${s.delta_ms.toFixed(0)}`, c3, y);
    doc.setTextColor(...g); doc.setFont("Inter", "bold");
    doc.text(s.grade, c4, y, { align: "right" }); doc.setFont("Inter", "normal");
    y += 22;
  }
  y += 14;

  // Idle vs Loaded karsilastirma grafigi (bar)
  doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
  doc.text(t("pdf_lbl_latency_chart").toUpperCase(), M, y);
  y += 18;
  const maxV = Math.max(bb.idle_latency_ms, bb.loaded_latency_ms, 1);
  const barMaxW = contentW - 130;
  const bars: [string, number, RGB][] = [
    [t("pdf_lbl_idle_lat"), bb.idle_latency_ms, C.accent],
    [t("pdf_lbl_loaded_lat"), bb.loaded_latency_ms, [249, 115, 22]],
  ];
  for (const [lbl, val, col] of bars) {
    doc.setFont("Inter", "normal"); doc.setFontSize(9); doc.setTextColor(...C.text);
    doc.text(lbl, M, y + 9);
    doc.setFillColor(...C.cardDim);
    doc.roundedRect(M + 110, y, barMaxW, 14, 3, 3, "F");
    const w = Math.max(2, (val / maxV) * barMaxW);
    doc.setFillColor(...col);
    doc.roundedRect(M + 110, y, w, 14, 3, 3, "F");
    doc.setTextColor(...C.ink); doc.setFont("Inter", "bold"); doc.setFontSize(9);
    doc.text(`${val.toFixed(0)} ms`, pageW - M - 4, y + 10, { align: "right" });
    y += 22;
  }
  return y + 10;
}

/** Ortak yatay-bar grafik. items: [etiket, deger, renk, gosterim-metni]. Yeni y doner. */
function hBars(
  doc: jsPDF, title: string, items: [string, number, RGB, string][],
  M: number, contentW: number, pageW: number, startY: number,
): number {
  let y = startY;
  doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
  doc.text(title.toUpperCase(), M, y);
  y += 18;
  const maxV = Math.max(...items.map(i => i[1]), 1);
  const labelW = 120;
  const barMaxW = contentW - labelW - 70;
  for (const [lbl, val, col, disp] of items) {
    doc.setFont("Inter", "normal"); doc.setFontSize(9); doc.setTextColor(...C.text);
    doc.text(lbl, M, y + 10);
    doc.setFillColor(...C.cardDim);
    doc.roundedRect(M + labelW, y, barMaxW, 14, 3, 3, "F");
    const w = Math.max(2, (val / maxV) * barMaxW);
    doc.setFillColor(...col);
    doc.roundedRect(M + labelW, y, w, 14, 3, 3, "F");
    doc.setFont("Inter", "bold"); doc.setFontSize(9); doc.setTextColor(...C.ink);
    doc.text(disp, pageW - M - 4, y + 10, { align: "right" });
    y += 22;
  }
  return y + 12;
}

/** ISP mudahale: alt-test durum listesi (renkli rozet). Yeni y doner. */
function renderInterventionList(
  doc: jsPDF, isp: NonNullable<TestResults["ispIntervention"]>, M: number, contentW: number, pageW: number, startY: number,
): number {
  let y = startY;
  doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
  doc.text(t("pdf_lbl_checks").toUpperCase(), M, y);
  y += 16;
  const items: [string, string][] = [
    ["DNS Hijack", isp.dns_hijack?.verdict ?? "—"],
    ["Transparent Proxy", isp.transparent_proxy?.verdict ?? "—"],
    ["SNI Filtering", isp.sni_filtering?.verdict ?? "—"],
    ["MITM (TLS)", isp.mitm?.verdict ?? "—"],
    ["DoH / DoT", isp.doh_dot?.verdict ?? "—"],
    ["Port Access", isp.port_test?.verdict ?? "—"],
    ["AS Blocking", isp.as_blocking?.verdict ?? "—"],
  ];
  for (const [name, verdict] of items) {
    // Normal = yesil, Inconclusive = notr, digerleri = kirmizi
    const col: RGB = verdict === "Normal" ? C.pos : verdict === "Inconclusive" ? C.neu : C.neg;
    doc.setFillColor(...C.card); doc.setDrawColor(...C.line); doc.setLineWidth(0.8);
    doc.roundedRect(M, y, contentW, 26, 4, 4, "FD");
    doc.setFillColor(...col); doc.circle(M + 16, y + 13, 4, "F");
    doc.setFont("Inter", "normal"); doc.setFontSize(10); doc.setTextColor(...C.ink);
    doc.text(name, M + 30, y + 17);
    doc.setFont("Inter", "bold"); doc.setTextColor(...col);
    doc.text(verdict, pageW - M - 14, y + 17, { align: "right" });
    y += 30;
  }
  return y + 10;
}

/**
 * Smart Diagnosis + test sonuclarindan 3 bolumlu PDF uretip diske kaydeder.
 */
export async function exportPdfReport(
  smart: SmartDiagnosis,
  results: TestResults,
): Promise<string | null> {
  const doc = new jsPDF({ unit: "pt", format: "a4" });
  ensureFont(doc);
  const pageW = doc.internal.pageSize.getWidth();
  const pageH = doc.internal.pageSize.getHeight();
  const M = 44;
  const contentW = pageW - M * 2;

  // ═══ SAYFA 1 — Harf Puanlama Tablosu ════════════════════════════════
  pageBg(doc, pageW, pageH);
  header(doc, pageW, t("pdf_report_title"));
  let y = 84 + 28;

  doc.setFont("Inter", "bold"); doc.setFontSize(14); doc.setTextColor(...C.ink);
  doc.text(t("diag_modules_title"), M, y);
  y += 24;

  // Tablo satirlari (her modul: ad + grade rozeti)
  for (const m of smart.modules) {
    const g = GRADE_RGB[m.grade] ?? C.muted;
    const dim = !m.available;
    doc.setFillColor(...(dim ? C.cardDim : C.card));
    doc.setDrawColor(...C.line); doc.setLineWidth(1);
    doc.roundedRect(M, y, contentW, 34, 6, 6, "FD");
    // ad
    doc.setFont("Inter", "bold"); doc.setFontSize(11);
    doc.setTextColor(...(dim ? C.muted : C.ink));
    doc.text(moduleName(m), M + 16, y + 22);
    // grade rozeti (sag)
    doc.setFillColor(...g);
    doc.roundedRect(pageW - M - 52, y + 7, 40, 20, 4, 4, "F");
    doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.white);
    doc.text(m.grade, pageW - M - 32, y + 21, { align: "center" });
    y += 40;
  }

  // ═══ SAYFA 2 — Genel Sentez (pozitif / notr / negatif) ══════════════
  doc.addPage();
  pageBg(doc, pageW, pageH);
  header(doc, pageW, t("pdf_synthesis_title"));
  y = 84 + 28;

  // Genel durum rozeti
  const sc = STATUS_RGB[smart.overall_status] ?? C.muted;
  doc.setFont("Inter", "bold"); doc.setFontSize(13); doc.setTextColor(...C.muted);
  doc.text(t("diag_overall").toUpperCase(), M, y);
  doc.setFontSize(16); doc.setTextColor(...sc);
  doc.text(tk(`diag_status_${smart.overall_status}`), M + 170, y);
  y += 14;
  doc.setFont("Inter", "normal"); doc.setFontSize(10); doc.setTextColor(...C.text);
  const ovLines: string[] = doc.splitTextToSize(tk(`diag_overall_${smart.overall_status}`), contentW);
  doc.text(ovLines, M, y + 16, { lineHeightFactor: 1.35 });
  y += 16 + ovLines.length * 14 + 18;

  // Pozitif / Notr / Negatif gruplari (grade-bazli; 3b'de engine metni gelecek)
  const groups: { key: "pos" | "neu" | "neg"; color: RGB; titleKey: string }[] = [
    { key: "pos", color: C.pos, titleKey: "pdf_group_positive" },
    { key: "neu", color: C.neu, titleKey: "pdf_group_neutral" },
    { key: "neg", color: C.neg, titleKey: "pdf_group_negative" },
  ];
  for (const grp of groups) {
    const mods = smart.modules.filter(m => m.available && sentiment(m.grade) === grp.key);
    if (mods.length === 0) continue;
    if (y > pageH - 90) { doc.addPage(); pageBg(doc, pageW, pageH); y = M; }
    // baslik
    doc.setFillColor(...grp.color);
    doc.circle(M + 4, y - 3, 4, "F");
    doc.setFont("Inter", "bold"); doc.setFontSize(12); doc.setTextColor(...grp.color);
    doc.text(t(grp.titleKey as Parameters<typeof t>[0]), M + 16, y);
    y += 18;
    for (const m of mods) {
      const line = `${moduleName(m)} (${m.grade}) — ${tk(m.detail_key)}`;
      const lines: string[] = doc.splitTextToSize(line, contentW - 16);
      if (y + lines.length * 13 > pageH - 50) { doc.addPage(); pageBg(doc, pageW, pageH); y = M; }
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.text);
      doc.text(lines, M + 16, y, { lineHeightFactor: 1.3 });
      y += lines.length * 13 + 8;
    }
    y += 12;
  }

  // ═══ SAYFA 3+ — Her olculen modul icin 1 sayfa ══════════════════════
  for (const m of smart.modules) {
    if (!m.available) continue;
    doc.addPage();
    pageBg(doc, pageW, pageH);
    header(doc, pageW, moduleName(m));
    y = 84 + 32;

    // Grade rozeti büyük
    const g = GRADE_RGB[m.grade] ?? C.muted;
    doc.setFillColor(...g);
    doc.roundedRect(M, y, 64, 48, 8, 8, "F");
    doc.setFont("Inter", "bold"); doc.setFontSize(26); doc.setTextColor(...C.white);
    doc.text(m.grade, M + 32, y + 33, { align: "center" });
    // Modul adi + sentiment
    doc.setFont("Inter", "bold"); doc.setFontSize(16); doc.setTextColor(...C.ink);
    doc.text(moduleName(m), M + 80, y + 22);
    const sKey = sentiment(m.grade);
    const sCol = sKey === "pos" ? C.pos : sKey === "neu" ? C.neu : C.neg;
    doc.setFont("Inter", "normal"); doc.setFontSize(10); doc.setTextColor(...sCol);
    doc.text(t(`pdf_group_${sKey === "pos" ? "positive" : sKey === "neu" ? "neutral" : "negative"}` as Parameters<typeof t>[0]), M + 80, y + 40);
    y += 48 + 28;

    // Modüle ozel detay / grafik
    if (m.module === "latency" || m.module === "jitter" || m.module === "loss") {
      y = renderPingTargets(doc, results.pings, M, pageW, y);
    } else if (m.module === "bufferbloat" && results.bufferbloat) {
      y = renderBufferbloat(doc, results.bufferbloat, M, contentW, pageW, y);
    } else if (m.module === "speed") {
      y = hBars(doc, t("diag_mod_speed"), [
        [t("pdf_lbl_download"), results.speedDl ?? 0, C.accent, `${(results.speedDl ?? 0).toFixed(1)} Mbps`],
        [t("pdf_lbl_upload"), results.speedUl ?? 0, [139, 92, 246], `${(results.speedUl ?? 0).toFixed(1)} Mbps`],
      ], M, contentW, pageW, y);
      if (results.speedPing != null || results.speedServer) {
        doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
        doc.text(`${t("pdf_lbl_ping")}: ${results.speedPing?.toFixed(0) ?? "—"} ms   ·   ${t("pdf_lbl_server")}: ${results.speedServer ?? "—"}`, M, y);
        y += 22;
      }
    } else if (m.module === "throttle" && results.throttle) {
      const th = results.throttle;
      y = hBars(doc, t("diag_mod_throttle"), [
        [t("pdf_lbl_baseline"), th.baseline_mbps, C.accent, `${th.baseline_mbps.toFixed(1)} Mbps`],
        ["BitTorrent", th.bittorrent_mbps, GRADE_RGB[th.bittorrent_verdict === "Normal" ? "A" : "F"], `${th.bittorrent_mbps.toFixed(1)} · ${th.bittorrent_verdict}`],
        ["QUIC", th.quic_mbps, GRADE_RGB[th.quic_verdict === "Normal" ? "A" : "F"], `${th.quic_mbps.toFixed(1)} · ${th.quic_verdict}`],
        ["WireGuard", th.wireguard_mbps, GRADE_RGB[th.wireguard_verdict === "Normal" ? "A" : "F"], `${th.wireguard_mbps.toFixed(1)} · ${th.wireguard_verdict}`],
        ["OpenVPN", th.openvpn_mbps, GRADE_RGB[th.openvpn_verdict === "Normal" ? "A" : "F"], `${th.openvpn_mbps.toFixed(1)} · ${th.openvpn_verdict}`],
      ], M, contentW, pageW, y);
    } else if (m.module === "isp_intervention" && results.ispIntervention) {
      y = renderInterventionList(doc, results.ispIntervention, M, contentW, pageW, y);
    } else if (m.module === "ipv6" && results.ipv6) {
      const q = results.ipv6.quality;
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
      doc.text(`${t("pdf_lbl_connectivity")}: ${results.ipv6.connectivity.connectivity_grade}   ·   ${t("pdf_lbl_ipv6_score")}: ${q.score}/100`, M, y);
      y += 22;
    } else if (m.module === "cgnat" && results.cgnat) {
      const cg = results.cgnat;
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
      const vlabel = cg.verdict === "public" ? t("cgnat_verdict_public")
        : cg.verdict === "cgnat" ? t("cgnat_verdict_cgnat")
        : cg.verdict === "double_nat" ? t("cgnat_verdict_double_nat")
        : t("cgnat_verdict_unknown");
      doc.text(`${vlabel}`, M, y); y += 16;
      doc.text(`${t("cgnat_lbl_router_wan")}: ${cg.router_wan_ip ?? "—"}   ·   ${t("cgnat_lbl_web")}: ${cg.web_public_ip ?? "—"}`, M, y); y += 16;
    } else if (m.module === "mtu" && results.mtu) {
      const mt = results.mtu;
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
      doc.text(`${t("mtu_col_mtu")}: ${mt.primary_mtu ?? "—"} B   ·   ${t("mtu_col_mss")}: ${mt.primary_mtu != null ? mt.primary_mtu - 40 : "—"} B`, M, y); y += 16;
    } else if (m.module === "voip" && results.voip) {
      const vr = results.voip.voice;
      const mosStr = vr.mos != null ? vr.mos.toFixed(2) : "-";
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
      doc.text(`MOS: ${mosStr}   RTT: ${vr.rtt_avg_ms}ms   Jitter: ${vr.jitter_ms}ms   Loss: ${vr.loss_pct}%`, M, y); y += 16;
    } else if (m.module === "wifi" && results.wifi?.connected) {
      const w = results.wifi;
      const sigCol: RGB = w.signal_percent >= 70 ? C.pos : w.signal_percent >= 45 ? C.neu : C.neg;
      y = hBars(doc, t("pdf_lbl_signal"), [
        [w.ssid, w.signal_percent, sigCol, `${w.signal_percent}% (${w.signal_dbm} dBm)`],
      ], M, contentW, pageW, y);
      doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.muted);
      doc.text(`${t("pdf_lbl_band")}: ${w.band}   ·   ${w.phy_type}`, M, y);
      y += 16;
      // Co-channel neighbor count from wifi_scan (graceful: falls back to 0)
      const coChannelCount = (() => {
        const aps = results.wifiScan?.access_points;
        if (!aps) return 0;
        const connected = aps.find(ap => ap.is_connected);
        if (!connected) return 0;
        const myOui = connected.bssid.length >= 8 ? connected.bssid.slice(0, 8) : "";
        return aps.filter(ap =>
          !ap.is_connected &&
          ap.channel === connected.channel &&
          (myOui === "" || ap.bssid.slice(0, 8) !== myOui) &&
          ap.ssid !== connected.ssid &&
          ap.rssi > -85
        ).length;
      })();
      doc.text(`Co-channel neighbors: ${coChannelCount}   ·   Ch ${results.wifiScan?.current_channel ?? w.channel}`, M, y);
      y += 14;
    }

    // Degerlendirme: ozet cumle + (varsa) zengin engine icgoruleri (pozitif/notr/negatif)
    doc.setFont("Inter", "bold"); doc.setFontSize(11); doc.setTextColor(...C.muted);
    doc.text(t("pdf_lbl_assessment").toUpperCase(), M, y);
    y += 16;
    doc.setFillColor(...C.card); doc.setDrawColor(...C.line); doc.setLineWidth(1);
    const cLines: string[] = doc.splitTextToSize(tk(m.detail_key), contentW - 28);
    const cH = 20 + cLines.length * 14;
    doc.roundedRect(M, y, contentW, cH, 6, 6, "FD");
    doc.setFont("Inter", "normal"); doc.setFontSize(10); doc.setTextColor(...C.text);
    doc.text(cLines, M + 14, y + 18, { lineHeightFactor: 1.4 });
    y += cH + 16;

    // Zengin icgoruler (Faz 3b) — her madde renkli sembol + metin
    if (m.insights && m.insights.length > 0) {
      for (const it of m.insights) {
        const col: RGB = it.kind === "positive" ? C.pos : it.kind === "neutral" ? C.neu : C.neg;
        const sym = it.kind === "positive" ? "+" : it.kind === "neutral" ? "~" : "!";
        const iLines: string[] = doc.splitTextToSize(tk(it.text_key), contentW - 28);
        const iH = iLines.length * 13 + 4;
        if (y + iH > pageH - 50) { doc.addPage(); pageBg(doc, pageW, pageH); y = M; }
        // renkli sembol kutusu
        doc.setFillColor(...col);
        doc.roundedRect(M, y - 9, 14, 14, 3, 3, "F");
        doc.setFont("Inter", "bold"); doc.setFontSize(10); doc.setTextColor(...C.bg);
        doc.text(sym, M + 7, y + 1, { align: "center" });
        // metin
        doc.setFont("Inter", "normal"); doc.setFontSize(9.5); doc.setTextColor(...C.text);
        doc.text(iLines, M + 22, y, { lineHeightFactor: 1.32 });
        y += iH + 6;
      }
    }
  }

  // ── Footer (tum sayfalar) + kaydet ──────────────────────────────────
  footer(doc, pageW, pageH);
  const stamp = new Date().toISOString().slice(0, 10);
  const path = await save({
    defaultPath: `netveil-report-${stamp}.pdf`,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!path) return null;
  await writeFile(path, new Uint8Array(doc.output("arraybuffer")));
  return path;
}

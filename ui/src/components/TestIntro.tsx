import type { CSSProperties } from "react";
import { Icon, type IconKey } from "../lib/icons";
import { t } from "../lib/i18n";

/**
 * Ortak bos-durum kalibi (pat1): Icon + Baslik + Aciklama + buton.
 * Test-baslatmali modullerin henuz sonuc gelmemis halinde gosterilir.
 * Sonuc gelince modul kendi sonuc UI'sini render eder.
 *
 * Iki mod:
 *  - Varsayilan (onRun): "Baslat" butonu testi DOGRUDAN calistirir (girdi gerektirmeyen
 *    moduller: Bufferbloat, IPv6, vb.).
 *  - onNext verilirse: "Next" butonu testin ayar/girdi ekranina goturur (girdi gerektiren
 *    moduller: Twitch stream key gibi). Bu modda onRun yok sayilir, isRunning gosterilmez.
 */
interface Props {
  iconKey: IconKey;
  title: string;
  description: string;
  buttonLabel: string;
  onRun?: () => void;
  isRunning?: boolean;
  /** Verilirse "Next" akisi: butona basinca ayar ekranina gecer (testi baslatmaz). */
  onNext?: () => void;
  /** Next butonu etiketi (varsayilan: btn_next i18n). */
  nextLabel?: string;
  /** Dis-trafik uyarisi (kirmizi kutu, description altinda). */
  warning?: string;
  /** PRO kilit: true ise baslat butonu pasif + gold 'Aktive Et' seridi gosterilir. */
  locked?: boolean;
  /** Kilitli seritteki butona basilinca (Settings'e yonlendirir). */
  onActivate?: () => void;
}

const wrap: CSSProperties = {
  display: "flex", flexDirection: "column", alignItems: "center",
  justifyContent: "center", gap: 16, padding: "56px 32px", textAlign: "center",
};

export function TestIntro({
  iconKey,
  title,
  description,
  buttonLabel,
  onRun,
  isRunning,
  onNext,
  nextLabel,
  warning,
  locked,
  onActivate,
}: Props) {
  const Ico = Icon[iconKey];
  const isNextMode = typeof onNext === "function";
  return (
    <div className="card" style={wrap}>
      <div style={{ opacity: 0.45, color: "var(--text-muted)" }}>
        <Ico style={{ width: 46, height: 46 }} />
      </div>
      <div style={{ fontSize: 18, fontWeight: 700, color: "var(--text)" }}>{title}</div>
      <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 460, lineHeight: 1.65, whiteSpace: "pre-line" }}>
        {description}
      </div>
      {warning && (
        <div style={{ fontSize: 12, color: "#ef4444", maxWidth: 460, lineHeight: 1.6, padding: "10px 14px", borderRadius: 8, background: "rgba(239,68,68,0.08)", border: "1px solid rgba(239,68,68,0.25)" }}>
          {warning}
        </div>
      )}
      {isNextMode ? (
        <button className="btn" onClick={onNext} type="button" disabled={locked} style={{ marginTop: 4, ...(locked ? { opacity: 0.4, cursor: "not-allowed" } : {}) }}>
          {nextLabel ?? t("btn_next")}
          <Icon.ChevronRight />
        </button>
      ) : (
        <button
          className="btn"
          onClick={onRun}
          disabled={isRunning || locked}
          type="button"
          style={{ marginTop: 4, ...(locked ? { opacity: 0.4, cursor: "not-allowed" } : {}) }}
        >
          {isRunning
            ? <Icon.Refresh style={{ animation: "spin 1s linear infinite" }} />
            : <Icon.Play />}
          {isRunning ? t("btn_running") : buttonLabel}
        </button>
      )}
      {locked && <ProActivateBar onActivate={onActivate} />}
    </div>
  );
}

/** Intro altinda gosterilen ince gold 'PRO - Aktive Et' seridi (kilitli premium araclar). */
export function ProActivateBar({ onActivate }: { onActivate?: () => void }) {
  return (
    <div style={{
      marginTop: 8,
      display: "flex", flexDirection: "column", alignItems: "center", gap: 10,
      padding: "12px 18px", borderRadius: 10, maxWidth: 460, width: "100%",
      background: "linear-gradient(180deg, rgba(212,175,55,0.10) 0%, rgba(212,175,55,0.03) 100%)",
      border: "1px solid rgba(212,175,55,0.28)",
    }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
        <Icon.Lock style={{ width: 15, height: 15, color: "#d4af37", flexShrink: 0 }} />
        <span style={{ fontSize: 12.5, color: "#e8d9a0" }}>
          {t("license_pro_tool_note")}
        </span>
      </div>
      <button
        onClick={onActivate}
        type="button"
        style={{
          padding: "6px 16px", borderRadius: 7, fontSize: 12.5, fontWeight: 600,
          cursor: "pointer", border: "none", whiteSpace: "nowrap",
          background: "linear-gradient(135deg, #d4af37, #b8860b)", color: "#1a1500",
        }}
      >
        {t("license_gate_upgrade_btn")}
      </button>
    </div>
  );
}

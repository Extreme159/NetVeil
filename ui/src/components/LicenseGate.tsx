import { Icon } from "../lib/icons";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";

interface Props {
  /** Settings sekmesine yönlendirir (lisans aktivasyon paneli oradadır). */
  onActivate: () => void;
}

/**
 * Premium araç / Advanced Diagnosis kilitliyken tab içeriği yerine gösterilen ekran.
 * Lisans yoksa "PRO" kilidi + aktivasyona yönlendiren CTA. Free araçlar bunu kullanmaz.
 */
export function LicenseGate({ onActivate }: Props) {
  useLang(); // dil değişince re-render
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        textAlign: "center",
        minHeight: "60vh",
        gap: 18,
        padding: 32,
      }}
    >
      <div
        style={{
          width: 64,
          height: 64,
          borderRadius: 16,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          background: "rgba(139,92,246,0.12)",
          border: "1px solid rgba(139,92,246,0.3)",
          color: "#a78bfa",
        }}
      >
        <Icon.Lock />
      </div>

      <span
        style={{
          fontSize: 9,
          fontWeight: 700,
          letterSpacing: "0.08em",
          padding: "2px 8px",
          borderRadius: 4,
          background: "rgba(139,92,246,0.2)",
          color: "#a78bfa",
          border: "1px solid rgba(139,92,246,0.3)",
        }}
      >
        PRO
      </span>

      <h2 style={{ fontSize: 20, fontWeight: 700, margin: 0 }}>
        {t("license_gate_title")}
      </h2>

      <p
        style={{
          maxWidth: 440,
          color: "var(--text-muted)",
          fontSize: 14,
          lineHeight: 1.6,
          margin: 0,
        }}
      >
        {t("license_gate_desc")}
      </p>

      <button
        onClick={onActivate}
        style={{
          marginTop: 6,
          padding: "10px 22px",
          borderRadius: 8,
          fontSize: 14,
          fontWeight: 600,
          cursor: "pointer",
          border: "none",
          background: "linear-gradient(135deg, #8b5cf6, #6366f1)",
          color: "#fff",
        }}
      >
        {t("license_gate_upgrade_btn")}
      </button>
    </div>
  );
}

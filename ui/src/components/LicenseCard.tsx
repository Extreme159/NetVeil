import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";
import { Icon } from "../lib/icons";
import type { LicenseStatus } from "../types";

interface Props {
  licenseStatus: LicenseStatus | null;
  setLicenseStatus: (s: LicenseStatus | null) => void;
}

const rowStyle: React.CSSProperties = {
  padding: "12px 18px",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 16,
  flexWrap: "wrap",
};

const labelStyle: React.CSSProperties = {
  fontSize: 14,
  fontWeight: 600,
  color: "var(--text)",
};

/** Durum metnini i18n anahtarına eşler. */
function statusLabel(status: string): string {
  switch (status) {
    case "active": return t("settings_license_status_active");
    case "grace": return t("settings_license_status_active");
    case "inactive": return t("settings_license_status_inactive");
    case "expired": return t("settings_license_status_expired");
    case "disabled": return t("settings_license_status_invalid");
    default: return t("settings_license_status_inactive");
  }
}

function typeLabel(type: string): string {
  if (type === "monthly") return t("settings_license_type_monthly");
  if (type === "lifetime") return t("settings_license_type_lifetime");
  return "";
}

/**
 * Settings içindeki lisans yönetim kartı.
 * Lisanssız: key girişi + Activate. Lisanslı: durum + tier + Deactivate.
 */
export function LicenseCard({ licenseStatus, setLicenseStatus }: Props) {
  useLang();
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isValid = licenseStatus?.valid ?? false;

  const onActivate = async () => {
    if (!key.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const s = await invoke<LicenseStatus>("activate_license", { key: key.trim() });
      setLicenseStatus(s);
      setKey("");
    } catch (e: unknown) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setBusy(false);
    }
  };

  const onDeactivate = async () => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await invoke("deactivate_license");
      setLicenseStatus(null);
    } catch (e: unknown) {
      setError(typeof e === "string" ? e : String(e));
    } finally {
      setBusy(false);
    }
  };

  // Lisanslı görünüm: durum + tier rozeti + deactivate
  if (isValid) {
    return (
      <div className="card" style={{ flexDirection: "column", alignItems: "stretch", gap: 0 }}>
        <div style={rowStyle}>
          <span style={{ ...labelStyle, display: "flex", alignItems: "center", gap: 8 }}>
            <Icon.Lock style={{ width: 16, height: 16, color: "#a78bfa" }} />
            {t("settings_license_title")}
          </span>
          <span style={{
            fontSize: 11, fontWeight: 700, letterSpacing: "0.05em",
            padding: "3px 10px", borderRadius: 5,
            background: "rgba(139,92,246,0.18)", color: "#a78bfa",
            border: "1px solid rgba(139,92,246,0.3)",
          }}>
            {typeLabel(licenseStatus!.license_type)} · {statusLabel(licenseStatus!.status)}
          </span>
        </div>
        <div style={{ ...rowStyle, borderTop: "1px solid var(--border)" }}>
          <span style={{ fontSize: 12, color: "var(--text-muted)", fontFamily: "var(--mono)" }}>
            {licenseStatus!.license_key ?? ""}
            {licenseStatus!.expires_at && ` · ${licenseStatus!.expires_at.slice(0, 10)}`}
          </span>
          <button
            onClick={onDeactivate}
            disabled={busy}
            style={{
              padding: "7px 16px", borderRadius: 7, fontSize: 13, fontWeight: 600,
              cursor: busy ? "default" : "pointer", opacity: busy ? 0.6 : 1,
              border: "1px solid var(--border)", background: "rgba(255,255,255,0.04)",
              color: "var(--text)",
            }}
          >
            {t("settings_license_deactivate_btn")}
          </button>
        </div>
        {error && (
          <div style={{ padding: "8px 18px", color: "#f87171", fontSize: 12 }}>{error}</div>
        )}
      </div>
    );
  }

  // Lisanssız görünüm: key girişi + activate
  return (
    <div className="card" style={{ flexDirection: "column", alignItems: "stretch", gap: 0 }}>
      <div style={rowStyle}>
        <span style={{ ...labelStyle, display: "flex", alignItems: "center", gap: 8 }}>
          <Icon.Lock style={{ width: 16, height: 16, color: "var(--text-muted)" }} />
          {t("settings_license_title")}
        </span>
      </div>
      <div style={{ ...rowStyle, borderTop: "1px solid var(--border)", flexWrap: "nowrap" }}>
        <input
          type="text"
          value={key}
          onChange={e => setKey(e.target.value)}
          onKeyDown={e => { if (e.key === "Enter") onActivate(); }}
          placeholder={t("settings_license_key_placeholder")}
          spellCheck={false}
          style={{
            flex: 1, minWidth: 0,
            background: "rgba(255,255,255,0.04)", border: "1px solid var(--border)",
            borderRadius: 8, color: "var(--text)", padding: "8px 12px",
            fontSize: 13, fontFamily: "var(--mono)", outline: "none",
          }}
        />
        <button
          onClick={onActivate}
          disabled={busy || !key.trim()}
          style={{
            padding: "8px 18px", borderRadius: 8, fontSize: 13, fontWeight: 600,
            cursor: (busy || !key.trim()) ? "default" : "pointer",
            opacity: (busy || !key.trim()) ? 0.6 : 1, border: "none",
            background: "linear-gradient(135deg, #8b5cf6, #6366f1)", color: "#fff",
            whiteSpace: "nowrap",
          }}
        >
          {busy ? "…" : t("settings_license_activate_btn")}
        </button>
      </div>
      {error && (
        <div style={{ padding: "8px 18px", color: "#f87171", fontSize: 12 }}>{error}</div>
      )}
    </div>
  );
}

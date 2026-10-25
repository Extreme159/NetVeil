import { useState } from "react";
import { setLang, type Lang } from "../../lib/i18n";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { getScale, setScale, type UiScale } from "../../lib/uiScale";
import { getContentScale, setContentScale, CONTENT_SCALE_OPTIONS, type ContentScale } from "../../lib/contentScale";
import { Icon } from "../../lib/icons";
import { LicenseCard } from "../LicenseCard";
import type { LicenseStatus } from "../../types";
import pkg from "../../../package.json";

// Versiyon tek kaynak: ui/package.json (bump-version.mjs bunu gunceller).
const APP_VERSION = pkg.version;

const selectStyle: React.CSSProperties = {
  width: 200,
  background: "rgba(255,255,255,0.04)",
  border: "1px solid var(--border)",
  borderRadius: 8,
  color: "var(--text)",
  padding: "8px 12px",
  fontSize: 13,
  cursor: "pointer",
  outline: "none",
};

const rowStyle: React.CSSProperties = {
  padding: "12px 18px",
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: 16,
};

const labelStyle: React.CSSProperties = {
  fontSize: 14,
  fontWeight: 600,
  color: "var(--text)",
};

/**
 * Ayarlar — tam sayfa tab (eski mini SettingsModal'in yerini aldi).
 * Dil + UI Scale; ileride yeni ayar gruplari kart olarak eklenir.
 */
export function SettingsTab({ updateInfo, licenseStatus, setLicenseStatus }: {
  updateInfo: import("../../types").UpdateInfo | null;
  licenseStatus?: LicenseStatus | null;
  setLicenseStatus?: (s: LicenseStatus | null) => void;
}) {
  const lang = useLang();
  const [scale, setScaleState] = useState<UiScale>(getScale());
  const [contentScale, setContentScaleState] = useState<ContentScale>(getContentScale());

  const onScale = (v: UiScale) => {
    setScale(v);
    setScaleState(v);
  };

  const onContentScale = (v: ContentScale) => {
    setContentScale(v);
    setContentScaleState(v);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
      {/* Yeni sürüm bildirimi — sadece guncelleme varsa (Version satırının ustunde) */}
      {updateInfo && (
        <div className="card" style={{ ...rowStyle, background: "rgba(34,197,94,0.10)", borderColor: "rgba(34,197,94,0.35)" }}>
          <span style={{ ...labelStyle, color: "#86efac", display: "flex", alignItems: "center", gap: 8 }}>
            <Icon.Rocket style={{ width: 16, height: 16 }} />
            {t("dash_update_available")}v{updateInfo.latest_version}
          </span>
          <a
            href={updateInfo.download_url}
            target="_blank"
            rel="noreferrer"
            style={{ background: "#22c55e", color: "white", borderRadius: 6, padding: "4px 12px", fontSize: 12, fontWeight: 600, textDecoration: "none" }}
          >
            {t("dash_update_download")}
          </a>
        </div>
      )}

      {/* Lisans yonetimi (key girisi / durum + deactivate) */}
      {setLicenseStatus && (
        <LicenseCard licenseStatus={licenseStatus ?? null} setLicenseStatus={setLicenseStatus} />
      )}

      {/* Versiyon — bilgi satırı (select değil) */}
      <div className="card" style={rowStyle}>
        <span style={labelStyle}>{t("settings_version")}</span>
        <span style={{ fontSize: 13, fontWeight: 600, color: "var(--text-dim)", fontFamily: "var(--mono)" }}>
          v{APP_VERSION} · Beta
        </span>
      </div>

      {/* Dil grubu — tek satır */}
      <div className="card" style={rowStyle}>
        <span style={labelStyle}>{t("settings_language")}</span>
        <select
          value={lang}
          onChange={e => setLang(e.target.value as Lang)}
          style={selectStyle}
        >
          <option value="en">🇬🇧 English</option>
          <option value="tr">🇹🇷 Türkçe</option>
        </select>
      </div>

      {/* UI Scale — tek satır (pencere + zoom birlikte) */}
      <div className="card" style={rowStyle}>
        <span style={labelStyle}>{t("settings_ui_scale")}</span>
        <select
          value={scale}
          onChange={e => onScale(parseFloat(e.target.value) as UiScale)}
          style={selectStyle}
        >
          <option value={0.75}>%75</option>
          <option value={1.0}>%100</option>
          <option value={1.25}>%125</option>
        </select>
      </div>

      {/* İçerik Boyutu — pencere SABİT, sadece iç içerik (font + boşluklar) ölçeklenir */}
      <div className="card" style={rowStyle}>
        <span style={labelStyle}>{t("settings_content_scale")}</span>
        <select
          value={contentScale}
          onChange={e => onContentScale(parseFloat(e.target.value) as ContentScale)}
          style={selectStyle}
        >
          {CONTENT_SCALE_OPTIONS.map(v => (
            <option key={v} value={v}>%{Math.round(v * 100)}</option>
          ))}
        </select>
      </div>

      {/* İleride başka ayar grupları buraya kart olarak eklenir */}

      {/* Ticari marka bildirimi */}
      <div className="card" style={{ padding: "14px 18px" }}>
        <p style={{ margin: 0, fontSize: 11, color: "var(--text-muted)", lineHeight: 1.6, opacity: 0.75 }}>
          {t("trademark_full")}
        </p>
      </div>
    </div>
  );
}

import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import type { TabId } from "../../App";

const CHECKOUT_30D = "https://shop.netveil.net/checkout/buy/27285cc3-ccba-4fb5-aff5-f8e65676b889";
const CHECKOUT_LIFETIME = "https://shop.netveil.net/checkout/buy/9ba2ea85-902c-49e2-a8dc-f331b16596eb";

interface Props {
  setActiveTab: (t: TabId) => void;
}

/** Karşılaştırma tablosu satırı: ozellik adı + free/pro işaretleri. */
interface FeatureRow {
  labelKey: string;
  free: boolean;
}

const FEATURES: FeatureRow[] = [
  { labelKey: "getpro_feat_quick_gaming", free: true },
  { labelKey: "getpro_feat_ping", free: true },
  { labelKey: "getpro_feat_speed_bloat", free: true },
  { labelKey: "getpro_feat_wifi_dns", free: true },
  { labelKey: "getpro_feat_traceroute_history", free: true },
  { labelKey: "getpro_feat_advanced", free: false },
  { labelKey: "getpro_feat_isp_throttle", free: false },
  { labelKey: "getpro_feat_isp_interference", free: false },
  { labelKey: "getpro_feat_stability", free: false },
  { labelKey: "getpro_feat_voip", free: false },
  { labelKey: "getpro_feat_cgnat_leak", free: false },
  { labelKey: "getpro_feat_lan", free: false },
  { labelKey: "getpro_feat_twitch", free: false },
];

const GOLD = "#d4af37";

function Check({ on, gold }: { on: boolean; gold?: boolean }) {
  if (!on) return <span style={{ color: "var(--text-muted)", opacity: 0.5 }}>—</span>;
  return <span style={{ fontSize: 15, fontWeight: 700, color: gold ? GOLD : "var(--accent)" }}>✓</span>;
}

/** Fiyat kartı (checkout dış tarayıcıda açılır). */
function PriceCard({
  title, price, period, href, ctaLabel, highlight, badge,
}: { title: string; price: string; period: string; href: string; ctaLabel: string; highlight?: boolean; badge?: string }) {
  return (
    <div
      className="card"
      style={{
        flex: 1, minWidth: 200, padding: "14px 20px",
        display: "flex", flexDirection: "column", gap: 7,
        position: "relative",
        ...(highlight
          ? { border: `1px solid ${GOLD}66`, background: "linear-gradient(180deg, rgba(212,175,55,0.08) 0%, rgba(212,175,55,0.02) 100%)" }
          : {}),
      }}
    >
      {badge && (
        <span style={{
          position: "absolute", top: -10, left: 22,
          fontSize: 10, fontWeight: 700, letterSpacing: "0.05em",
          padding: "2px 10px", borderRadius: 5,
          background: `linear-gradient(135deg, ${GOLD}, #b8860b)`, color: "#1a1500",
        }}>{badge}</span>
      )}
      <div style={{ fontSize: 14, fontWeight: 700, color: "var(--text)" }}>{title}</div>
      <div style={{ display: "flex", alignItems: "baseline", gap: 4 }}>
        <span style={{ fontSize: 26, fontWeight: 800, color: highlight ? GOLD : "var(--text)" }}>{price}</span>
        <span style={{ fontSize: 12, color: "var(--text-muted)" }}>{period}</span>
      </div>
      <a
        href={href}
        target="_blank"
        rel="noreferrer noopener"
        style={{
          marginTop: 2, textDecoration: "none",
          display: "inline-flex", alignItems: "center", justifyContent: "center",
          padding: "8px 16px", borderRadius: 8, fontSize: 13.5, fontWeight: 600,
          border: "none", whiteSpace: "nowrap",
          ...(highlight
            ? { background: `linear-gradient(135deg, ${GOLD}, #b8860b)`, color: "#1a1500" }
            : { background: "linear-gradient(135deg, #8b5cf6, #6366f1)", color: "#fff" }),
        }}
      >
        {ctaLabel}
      </a>
    </div>
  );
}

/**
 * Get Pro — FREE vs PRO karşılaştırma + fiyat kartları.
 * Sidebar'da ve Dashboard ProTeaser'dan açılır. SADECE lisanssız kullanıcıya gösterilir.
 */
export function GetProTab({ setActiveTab }: Props) {
  useLang();
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 10, maxWidth: 980 }}>
      {/* Başlık TopBar'da (çift metin kaldırıldı); doğrudan tablo ile başlar. */}

      {/* Karşılaştırma tablosu */}
      <div className="card" style={{ padding: 0, overflow: "hidden" }}>
        {/* Başlık satırı */}
        <div style={{
          display: "grid", gridTemplateColumns: "1fr 80px 80px",
          padding: "6px 18px", borderBottom: "1px solid var(--border)",
          fontSize: 12, fontWeight: 700, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.04em",
        }}>
          <span>{t("getpro_col_feature")}</span>
          <span style={{ textAlign: "center" }}>{t("getpro_col_free")}</span>
          <span style={{ textAlign: "center", color: GOLD }}>{t("getpro_col_pro")}</span>
        </div>
        {/* Satırlar */}
        {FEATURES.map((f, i) => (
          <div
            key={f.labelKey}
            style={{
              display: "grid", gridTemplateColumns: "1fr 80px 80px", alignItems: "center",
              padding: "5px 18px",
              borderBottom: i < FEATURES.length - 1 ? "1px solid rgba(255,255,255,0.04)" : "none",
              background: !f.free ? "rgba(212,175,55,0.03)" : undefined,
            }}
          >
            <span style={{ fontSize: 13, color: "var(--text)" }}>{t(f.labelKey as Parameters<typeof t>[0])}</span>
            <span style={{ textAlign: "center" }}><Check on={f.free} /></span>
            <span style={{ textAlign: "center" }}><Check on gold /></span>
          </div>
        ))}
      </div>

      {/* Fiyat kartları */}
      <div style={{ display: "flex", gap: 14, flexWrap: "wrap" }}>
        <PriceCard
          title={t("getpro_30d_title")}
          price="$2.99"
          period={t("getpro_30d_period")}
          href={CHECKOUT_30D}
          ctaLabel={t("getpro_buy_30d")}
        />
        <PriceCard
          title={t("getpro_lifetime_title")}
          price="$19.99"
          period={t("getpro_lifetime_period")}
          href={CHECKOUT_LIFETIME}
          ctaLabel={t("getpro_buy_lifetime")}
          highlight
          badge={t("getpro_best_value")}
        />
      </div>

      {/* Zaten key'i olan -> activate */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: 8, fontSize: 12.5, color: "var(--text-muted)", marginTop: -2, marginBottom: -14 }}>
        <span>{t("getpro_have_key")}</span>
        <button
          type="button"
          onClick={() => setActiveTab("settings")}
          style={{ background: "none", border: "none", color: GOLD, fontSize: 13, fontWeight: 600, cursor: "pointer", padding: 0, textDecoration: "underline" }}
        >
          {t("getpro_activate_link")}
        </button>
      </div>
    </div>
  );
}

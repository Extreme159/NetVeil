import type { TopologyInfo, NicInfo, WifiInfo } from "../types";
import { t } from "../lib/i18n";

interface Props {
  topology: TopologyInfo | null;
  loading: boolean;
  gatewayPing: number | null;
  cfPing: number | null;
  nics: NicInfo[] | null;
  wifi: WifiInfo | null;
}

function pingColor(ms: number): string {
  if (ms < 50)  return "#a5b4fc"; // iyi — soluk mor/beyaz
  if (ms < 100) return "#fbbf24"; // orta — sarı
  return "#f87171";               // kötü — kırmızı
}

function ispShort(isp: string): string {
  if (!isp || isp === "—") return "ISP";
  const words = isp.split(" ").slice(0, 2).join(" ");
  return words.length > 18 ? words.slice(0, 18) + "…" : words;
}

/** Aktif NIC'in bağlantı hızını döndürür */
function linkSpeed(nics: NicInfo[] | null, wifi: WifiInfo | null): string {
  if (!nics) return "";
  const active = nics.find(n => n.is_active);
  if (!active) return "";
  // Wi-Fi: tx_rate kullan
  if (active.adapter_type === "Wi-Fi" && wifi?.tx_rate) return wifi.tx_rate;
  // Ethernet: PowerShell'den gelen gerçek link speed
  if (active.link_speed) return active.link_speed;
  return "";
}



/** Animasyonlu bağlantı çizgisi — üstte label, altta sublabel */
function Line({
  active,
  topLabel,
  bottomLabel,
  bottomColor,
}: {
  active: boolean;
  topLabel?: string;
  bottomLabel?: string;
  bottomColor?: string;
}) {
  return (
    <div className="cbar-line-wrap">
      {topLabel && (
        <span className="cbar-line-label top">{topLabel}</span>
      )}
      <div className="cbar-line">
        <div className="cbar-line-track" />
        {active && <div className="cbar-line-dot" />}
      </div>
      {bottomLabel && (
        <span className="cbar-line-label bottom" style={{ color: bottomColor }}>
          {bottomLabel}
        </span>
      )}
    </div>
  );
}

/** Cloudflare ikonu — turuncu bulut */
function CfLogo() {
  return (
    <svg viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg" style={{ width: 20, height: 20 }}>
      <path d="M16.5 10.5C16.5 10.5 16 7 12 7C8.5 7 7 9.5 7 9.5C7 9.5 4 9.8 4 13C4 16 6.5 16.5 8 16.5H16C17.9 16.5 20 15.2 20 13C20 10.8 18 10.5 16.5 10.5Z" fill="#F6821F" />
    </svg>
  );
}

export function ConnectionBar({ topology, loading, gatewayPing, cfPing, nics, wifi }: Props) {
  const topo = topology;
  const ready = !!topo && !loading;

  const localIp  = topo?.local_ip   ?? "—";
  const gwIp     = topo?.gateway_ip ?? "—";
  const wanIp    = topo?.wan_ip     ?? "—";
  const isp      = topo?.isp        ?? "—";

  const link  = linkSpeed(nics, wifi);

  return (
    <div className="cbar">

      {/* PC */}
      <div className="cbar-node">
        <div className="cbar-node-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8}
            strokeLinecap="round" strokeLinejoin="round">
            <rect x="2" y="3" width="20" height="14" rx="2" />
            <path d="M8 21h8M12 17v4" />
          </svg>
        </div>
        <div className="cbar-node-text">
          <span className="cbar-node-label">{t("cbar_this_pc")}</span>
          <span className="cbar-node-sub">{loading ? t("cbar_detecting") : localIp}</span>
        </div>
      </div>

      {/* PC → Gateway çizgisi */}
      <Line
        active={ready}
        topLabel={link !== "—" ? link : undefined}
        bottomLabel={gatewayPing != null ? `${Math.round(gatewayPing)} ms` : undefined}
        bottomColor={gatewayPing != null ? pingColor(gatewayPing) : undefined}
      />

      {/* Gateway */}
      <div className="cbar-node">
        <div className="cbar-node-icon">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8}
            strokeLinecap="round" strokeLinejoin="round">
            <path d="M12 3L4 7v5c0 4.4 3.4 8.5 8 9.5 4.6-1 8-5.1 8-9.5V7L12 3z" />
            <path d="M9 12l2 2 4-4" />
          </svg>
        </div>
        <div className="cbar-node-text">
          <span className="cbar-node-label">{t("cbar_gateway")}</span>
          <span className="cbar-node-sub">{loading ? t("cbar_detecting") : gwIp}</span>
          <span className="cbar-node-sub" style={{ color: "var(--text-muted)", fontSize: 9 }}>
            {loading ? "" : wanIp}
          </span>
        </div>
      </div>

      {/* Gateway → Cloudflare çizgisi */}
      <Line
        active={ready}
        topLabel={isp !== "—" ? ispShort(isp) : undefined}
        bottomLabel={cfPing != null ? `${Math.round(cfPing)} ms` : undefined}
        bottomColor={cfPing != null ? pingColor(cfPing) : undefined}
      />

      {/* Cloudflare */}
      <div className="cbar-node">
        <div className="cbar-node-icon accent">
          <CfLogo />
        </div>
        <div className="cbar-node-text">
          <span className="cbar-node-label">Cloudflare</span>
          <span className="cbar-node-sub">1.1.1.1</span>
        </div>
      </div>

    </div>
  );
}

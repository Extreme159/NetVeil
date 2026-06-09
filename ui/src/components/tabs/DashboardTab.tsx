import { useState } from "react";
import type { TabProps } from "./types";
import { Icon } from "../../lib/icons";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import type { UpdateInfo } from "../../types";
import { ConnectionBar } from "../ConnectionBar";

type IcoComp = typeof Icon[keyof typeof Icon];
const TYPE_ICONS: Record<string, IcoComp> = {
  "Wi-Fi": Icon.Wifi,
  Ethernet: Icon.Cable,
  VPN: Icon.Lock,
  Virtual: Icon.Monitor,
  Bluetooth: Icon.Bluetooth,
  Loopback: Icon.Refresh,
};

function fmtBytes(b: number): string {
  if (b === 0) return "0 B";
  const i = Math.floor(Math.log(b) / Math.log(1024));
  return `${(b / Math.pow(1024, i)).toFixed(1)} ${["B", "KB", "MB", "GB", "TB"][i]}`;
}

/* ============================================================
   Yeni versiyon bildirimi
   ============================================================ */
function UpdateBanner({ info }: { info: UpdateInfo | null }) {
  const [dismissed, setDismissed] = useState(false);

  if (!info || dismissed) return null;

  return (
    <div style={{
      display: "flex", alignItems: "center", gap: 12,
      background: "rgba(34,197,94,0.10)",
      border: "1px solid rgba(34,197,94,0.35)",
      borderRadius: 10, padding: "10px 16px", marginBottom: 16,
    }}>
      <Icon.Rocket style={{ width: 18, height: 18, color: "#86efac" }} />
      <div style={{ flex: 1 }}>
        <span style={{ fontWeight: 600, color: "#86efac", fontSize: 13 }}>
          {t("dash_update_available")}v{info.latest_version}
        </span>
      </div>
      <a
        href={info.download_url}
        target="_blank"
        rel="noreferrer"
        style={{
          background: "#22c55e", color: "white", borderRadius: 6,
          padding: "4px 12px", fontSize: 12, fontWeight: 600,
          textDecoration: "none", flexShrink: 0,
        }}
      >
        {t("dash_update_download")}
      </a>
      <button
        onClick={() => setDismissed(true)}
        style={{ background: "none", border: "none", color: "var(--text-muted)", cursor: "pointer", fontSize: 16, padding: 0, flexShrink: 0 }}
      >
        ✕
      </button>
    </div>
  );
}

/* ============================================================
   Hero — Ana Sayfa karsilama + One-Click Test CTA.
   ============================================================ */
function HomeHero({ onRunQuick, onRunGaming, onRunAdvanced }: { onRunQuick: () => void; onRunGaming: () => void; onRunAdvanced: () => void }) {
  return (
    <div
      className="card"
      style={{
        position: "relative",
        overflow: "hidden",
        padding: "32px 28px",
        marginBottom: 20,
        background:
          "radial-gradient(ellipse at 100% 0%, rgba(99,102,241,0.16), transparent 60%), radial-gradient(ellipse at 0% 100%, rgba(168,85,247,0.10), transparent 55%), var(--bg-card)",
        borderColor: "rgba(99,102,241,0.28)",
      }}
    >
      <div style={{ display: "flex", alignItems: "center", gap: 24, flexWrap: "wrap" }}>
        <div style={{ flex: 1, minWidth: 260 }}>
          <h1 style={{ fontSize: 24, fontWeight: 800, color: "var(--text)", margin: 0, lineHeight: 1.2 }}>
            {t("home_hero_title")}
          </h1>
          <p style={{ fontSize: 13.5, color: "var(--text-dim)", lineHeight: 1.6, margin: "10px 0 20px", maxWidth: 520 }}>
            {t("home_hero_desc")}
          </p>
          <div style={{ display: "flex", gap: 12, flexWrap: "wrap" }}>
            <button
              className="btn"
              onClick={onRunQuick}
              type="button"
              style={{ fontSize: 14, padding: "11px 22px", fontWeight: 700 }}
            >
              <Icon.Brain />
              {t("nav_quick_diagnosis")}
            </button>
            <button
              className="btn btn-secondary"
              onClick={onRunGaming}
              type="button"
              style={{ fontSize: 14, padding: "11px 22px", fontWeight: 700 }}
            >
              <Icon.Crosshair />
              {t("nav_gaming_diagnosis")}
            </button>
            <button
              className="btn btn-secondary"
              onClick={onRunAdvanced}
              type="button"
              style={{
                fontSize: 14, padding: "11px 22px", fontWeight: 700,
                color: "#fcd34d", borderColor: "rgba(234,179,8,0.4)",
                background: "rgba(234,179,8,0.10)",
              }}
            >
              <Icon.Search />
              {t("nav_smart")}
            </button>
          </div>
        </div>
        <div style={{ flexShrink: 0, opacity: 0.18, color: "var(--accent)" }}>
          <Icon.Brain style={{ width: 110, height: 110 }} />
        </div>
      </div>
    </div>
  );
}

type HomeTabId = Parameters<TabProps["setActiveTab"]>[0];

/* ============================================================
   Hizli araclar — sik kullanilan modullere kisayol kartlari.
   ============================================================ */
function QuickTools({ go }: { go: (t: HomeTabId) => void }) {
  // Lite araclar (Sidebar 'Araclar' grubu). Servisler/Oyun Ping'in alt-araclari -> burada yok.
  const items: { id: HomeTabId; icon: IcoComp; label: string }[] = [
    { id: "ping",          icon: Icon.PingTool,    label: t("nav_ping_standalone") },
    { id: "traceroute",    icon: Icon.Route,       label: t("nav_traceroute") },
    { id: "speedtest",     icon: Icon.Speed,       label: t("nav_speedtest") },
    { id: "bufferbloat",   icon: Icon.Buffer,      label: t("nav_bufferbloat") },
    { id: "ipv6",          icon: Icon.Globe,       label: "IPv6" },
    { id: "wifi-scan",     icon: Icon.WifiChannel, label: t("nav_wifi_scan") },
    { id: "dns-benchmark", icon: Icon.Dns,         label: t("nav_dns_benchmark") },
    { id: "mtu",           icon: Icon.Mtu,         label: t("nav_mtu") },
  ];
  return (
    <div className="card" style={{ padding: "16px 18px" }}>
      <div className="card-head" style={{ marginBottom: 12 }}>
        <div className="ico-wrap"><Icon.Bolt /></div>
        <h3>{t("home_quick_title")}</h3>
      </div>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 10 }}>
        {items.map(({ id, icon: Ico, label }) => (
          <button
            key={id}
            type="button"
            onClick={() => go(id)}
            style={{
              display: "flex", alignItems: "center", gap: 10,
              padding: "12px 14px", borderRadius: 10, cursor: "pointer",
              background: "rgba(255,255,255,0.02)", border: "1px solid var(--border)",
              color: "var(--text)", textAlign: "left", transition: "border-color 150ms, background 150ms",
            }}
            onMouseEnter={(e) => { e.currentTarget.style.borderColor = "var(--accent)"; e.currentTarget.style.background = "rgba(99,102,241,0.06)"; }}
            onMouseLeave={(e) => { e.currentTarget.style.borderColor = "var(--border)"; e.currentTarget.style.background = "rgba(255,255,255,0.02)"; }}
          >
            <span style={{ display: "grid", placeItems: "center", color: "var(--accent)", flexShrink: 0 }}><Ico style={{ width: 18, height: 18 }} /></span>
            <span style={{ fontSize: 13, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{label}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

/* ============================================================
   Ana Sayfa (eski Gösterge Paneli) — landing.
   Hero (One-Click CTA) + Bağlantı şeridi + Hızlı araçlar + Pro teaser + IP/NIC bilgileri.
   ============================================================ */
export function DashboardTab({ results, topology, topoLoading, gatewayPing, cfPing, setActiveTab, updateInfo }: TabProps) {
  useLang();
  const { nics, wifi } = results;

  return (
    <div>
      <UpdateBanner info={updateInfo} />

      <HomeHero
        onRunQuick={() => setActiveTab("quick")}
        onRunGaming={() => setActiveTab("gaming-diagnosis")}
        onRunAdvanced={() => setActiveTab("smart")}
      />

      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        <QuickTools go={setActiveTab} />



        {/* Wi-Fi Kalitesi karti "Wi-Fi Kanal Analizi" modulune tasindi (Premium). */}

        {/* IP Özet Kartı — topology gelir gelmez göster, nics'ten bağımsız */}
        {topology && (
          <div className="card" style={{ padding: 0, overflow: "hidden" }}>
            <div style={{
              display: "flex",
              alignItems: "center",
              gap: 10,
              padding: "12px 18px",
              borderBottom: "1px solid var(--border)",
              background: "rgba(10,10,15,0.4)",
            }}>
              <div className="ico-wrap"><Icon.Server /></div>
              <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
                {t("net_ip_info")}
              </h3>
            </div>

            {/* Bağlantı şeridi — IP Bilgileri kartına gömülü (Bu Bilgisayar → Ağ Geçidi → Cloudflare) */}
            <div className="cbar-embedded" style={{ padding: "10px 18px", borderBottom: "1px solid var(--border)" }}>
              <ConnectionBar
                topology={topology}
                loading={topoLoading}
                gatewayPing={gatewayPing}
                nics={nics}
                wifi={wifi}
                cfPing={cfPing}
              />
            </div>

            <div style={{
              display: "grid",
              gridTemplateColumns: "repeat(auto-fit, minmax(160px, 1fr))",
              gap: 10,
              padding: "14px 18px",
            }}>
              {[
                { label: t("net_lan_ip"),  value: topology.local_ip },
                { label: t("net_gateway"), value: topology.gateway_ip },
                { label: t("net_wan_ip"),  value: topology.wan_ip },
                { label: t("net_isp"),     value: topology.isp },
              ].map(({ label, value }) => (
                <div key={label} style={{
                  padding: "8px 12px",
                  borderRadius: 8,
                  background: "rgba(255,255,255,0.02)",
                  border: "1px solid var(--border)",
                }}>
                  <p style={{ fontSize: 10, color: "var(--text-muted)", textTransform: "uppercase", letterSpacing: "0.1em", margin: 0 }}>
                    {label}
                  </p>
                  <p className="mono" style={{ fontSize: 12, fontWeight: 500, color: "var(--text)", marginTop: 3, marginBottom: 0, wordBreak: "break-all" }}>
                    {value || "—"}
                  </p>
                </div>
              ))}
            </div>
          </div>
        )}

        {nics && (
          <div className="card" style={{ padding: 0, overflow: "hidden" }}>
            <div
              style={{
                display: "flex",
                alignItems: "center",
                gap: 10,
                padding: "12px 18px",
                borderBottom: "1px solid var(--border)",
                background: "rgba(10,10,15,0.4)",
              }}
            >
              <div className="ico-wrap"><Icon.Server /></div>
              <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
                {t("net_adapters")}
              </h3>
              <span style={{ marginLeft: "auto", fontFamily: "var(--mono)", fontSize: 11, color: "var(--text-muted)" }}>
                {nics.length}
              </span>
            </div>

            <div>
              {nics.map((nic, i) => (
                <div
                  key={i}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 14,
                    padding: "14px 18px",
                    borderBottom: i < nics.length - 1 ? "1px solid rgba(26,26,46,0.7)" : "none",
                  }}
                >
                  <span style={{ display: "grid", placeItems: "center", color: "var(--text-muted)" }}>{(() => { const I = TYPE_ICONS[nic.adapter_type] ?? Icon.Radio; return <I style={{ width: 20, height: 20 }} />; })()}</span>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                      <span style={{ fontSize: 13, fontWeight: 500, color: "var(--text)" }}>{nic.name}</span>
                      <span
                        style={{
                          fontFamily: "var(--mono)",
                          fontSize: 10,
                          padding: "2px 7px",
                          borderRadius: 4,
                          background: "var(--border)",
                          color: "var(--text-muted)",
                        }}
                      >
                        {nic.adapter_type}
                      </span>
                      {nic.is_active && (
                        <span
                          className="grade-badge A"
                          style={{ fontSize: 10, padding: "1px 8px" }}
                        >
                          {t("dash_nic_active")}
                        </span>
                      )}
                    </div>
                    <div style={{ display: "flex", gap: 16, marginTop: 4, flexWrap: "wrap" }}>
                      <span className="mono" style={{ fontSize: 11, color: "var(--text-muted)" }}>MAC: {nic.mac}</span>
                      {nic.ip_addresses
                        .filter(ip => {
                          if (ip.startsWith("fe80:") || ip === "::1") return false;
                          return true;
                        })
                        .slice(0, 2)
                        .map((ip, j) => {
                          const isIPv6 = ip.includes(":");
                          return (
                            <span key={j} className="mono" style={{ fontSize: 11, color: "var(--text-dim)" }}>
                              <span style={{ color: "var(--text-muted)" }}>{isIPv6 ? "IPv6" : "IPv4"}: </span>{ip}
                            </span>
                          );
                        })}
                      {nic.link_speed && (
                        <span className="mono" style={{ fontSize: 11, color: "var(--text-dim)" }}>
                          <span style={{ color: "var(--text-muted)" }}>Link: </span>{nic.link_speed}
                        </span>
                      )}
                    </div>
                  </div>
                  {nic.is_active && (
                    <div style={{ textAlign: "right" }}>
                      <p className="mono" style={{ fontSize: 11, color: "var(--text-muted)", margin: 0 }}>
                        ↓ {fmtBytes(nic.received_bytes)}
                      </p>
                      <p className="mono" style={{ fontSize: 11, color: "var(--text-muted)", margin: 0 }}>
                        ↑ {fmtBytes(nic.transmitted_bytes)}
                      </p>
                    </div>
                  )}
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

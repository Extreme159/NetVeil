import { useState, useEffect } from "react";
import type { TabId } from "../App";
import { Icon, type IconKey } from "../lib/icons";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";

// Sol alttaki durum satırı — targets yüklenince kısa süre gösterir
function StatusLine({ targetsLoaded }: { targetsLoaded: boolean }) {
  useLang();
  const [visible, setVisible] = useState(false);

  useEffect(() => {
    if (!targetsLoaded) return;
    setVisible(true);
    const id = setTimeout(() => setVisible(false), 4000);
    return () => clearTimeout(id);
  }, [targetsLoaded]);

  return (
    <div style={{ flex: 1, minWidth: 0, fontSize: 11, color: "var(--text-muted)", overflow: "hidden" }}>
      {visible && (
        <span style={{ display: "flex", alignItems: "center", gap: 5 }}>
          <span style={{ color: "var(--success)", fontSize: 14, lineHeight: 1 }}>✓</span>
          {t("status_targets_loaded")}
        </span>
      )}
    </div>
  );
}

type LeafIconKey = Exclude<
  IconKey,
  "Settings" | "Play" | "Logo" | "Bell" | "Refresh" | "Down" | "Up"
  | "Robot" | "Wave" | "Shield" | "Bolt" | "Drop" | "Eye" | "Wifi"
>;

interface NavLeaf {
  kind: "leaf";
  id: TabId;
  iconKey: LeafIconKey;
  label: string;
  badge?: string;
  disabled?: boolean;
}

interface NavGroup {
  kind: "group";
  id: string;
  iconKey: LeafIconKey;
  label: string;
  badge?: string;
  children: NavLeaf[];
}

type NavEntry = NavLeaf | NavGroup;

function buildOneClickNav(): NavEntry[] {
  return [
    { kind: "leaf", id: "quick",            iconKey: "Brain",     label: t("nav_quick_diagnosis") },
    { kind: "leaf", id: "gaming-diagnosis", iconKey: "Crosshair", label: t("nav_gaming_diagnosis") },
    { kind: "leaf", id: "smart",            iconKey: "Search",    label: t("nav_smart") },
  ];
}


// Araçlar: bağımsız test araçları (Tanılama'dan ayrı grup). Sıra korunur.
// "Ping" artık grup: saf ping aracı + Servisler + Oyun Sunucuları (eski 'latency' grubu buraya taşındı).
// NOT: tab ID'leri (ping/latency-services/latency-games) DEĞİŞMEDİ — Smart Engine akışı korunur.
function buildToolsNav(): NavEntry[] {
  return [
    {
      kind: "group",
      id: "ping-tools",
      iconKey: "PingTool",
      label: t("nav_ping"),
      children: [
        { kind: "leaf", id: "ping",             iconKey: "PingTool", label: t("nav_ping_standalone") },
        { kind: "leaf", id: "latency-services", iconKey: "Server",   label: t("nav_ping_services") },
        { kind: "leaf", id: "latency-games",    iconKey: "Game",     label: t("nav_ping_games") },
      ],
    },
    { kind: "leaf", id: "traceroute",  iconKey: "Route",       label: t("nav_traceroute") },
    { kind: "leaf", id: "speedtest",   iconKey: "Speed",       label: t("nav_speedtest") },
    { kind: "leaf", id: "bufferbloat", iconKey: "Buffer",      label: t("nav_bufferbloat") },
    { kind: "leaf", id: "ipv6",        iconKey: "Globe",       label: "IPv6" },
    { kind: "leaf", id: "wifi-scan",   iconKey: "WifiChannel", label: t("nav_wifi_scan") },
    { kind: "leaf", id: "dns-benchmark", iconKey: "Dns",       label: t("nav_dns_benchmark") },
    { kind: "leaf", id: "mtu",         iconKey: "Mtu",         label: t("nav_mtu") },
  ];
}

function buildHistoryNav(): NavLeaf[] {
  return [
    { kind: "leaf", id: "history", iconKey: "Chart", label: t("nav_history") },
  ];
}

/** Community (open-core) build'de premium nav/arac gizlenir. Default (resmi premium build) = true. */
const PREMIUM_BUILD = import.meta.env.VITE_NETVEIL_PREMIUM !== "false";

function buildPremiumNav(): NavEntry[] {
  return [
    { kind: "leaf", id: "throttle",          iconKey: "Throttle",     label: t("nav_throttle") },
    { kind: "leaf", id: "isp-intervention",  iconKey: "Intervention", label: t("nav_isp_intervention") },
    { kind: "leaf", id: "network-stability",  iconKey: "Stability",    label: t("nav_stability") },
    { kind: "leaf", id: "twitch-test",         iconKey: "TwitchTest",   label: t("nav_twitch") },
    { kind: "leaf", id: "cgnat",                iconKey: "Cgnat",        label: t("nav_cgnat") },
    { kind: "leaf", id: "dns-leak",             iconKey: "DnsLeak",      label: t("nav_dns_leak") },
    { kind: "leaf", id: "voip-quality",          iconKey: "VoipQuality",  label: t("nav_voip_quality") },
    { kind: "leaf", id: "lan-scanner",            iconKey: "Search",       label: t("nav_lan_scanner") },
    { kind: "leaf", id: "lan-speed",              iconKey: "Speed",        label: t("nav_lan_speed") },
  ];
}

interface Props {
  activeTab: TabId;
  onTabChange: (t: TabId) => void;
  targetsLoaded: boolean;
  /** Yeni surum var mi — dissli ikonu yesil pulse yapar. */
  hasUpdate: boolean;
  /** Lisans gecerli mi — Premium kilit (FAZ3'te kullanilacak). */
  isLicensed: boolean;
}

export function Sidebar({ activeTab, onTabChange, targetsLoaded, hasUpdate, isLicensed }: Props) {
  useLang(); // dil değişince re-render
  const initialOpen: Record<string, boolean> = {};
  for (const entry of buildToolsNav()) {
    if (entry.kind === "group") {
      // Varsayilan KAPALI; yalnizca aktif sekme bu grubun cocuguysa acik baslar.
      initialOpen[entry.id] = entry.children.some(c => c.id === activeTab);
    }
  }
  const [openGroups, setOpenGroups] = useState<Record<string, boolean>>(initialOpen);

  const toggleGroup = (id: string) =>
    setOpenGroups(prev => ({ ...prev, [id]: !prev[id] }));

  return (
    <aside className="sidebar">
      <div className="brand">
        <img src="/netveil-app-icon.png" alt="" className="brand-logo" />
        <div className="brand-text">
          <span className="brand-word"><span className="brand-net">Net</span><span className="brand-veil">Veil</span></span>
          <span className="brand-tagline">Lift the Veil</span>
        </div>
      </div>

      {/* Gösterge Paneli — en üstte, başlıksız tek öğe (Tanılama bölümü kaldırıldı) */}
      <div
        className={"nav-item" + (activeTab === "dashboard" ? " active" : "")}
        onClick={() => onTabChange("dashboard")}
      >
        <span className="ico"><Icon.Dashboard /></span>
        <span>{t("nav_dashboard")}</span>
      </div>

      {/* Get Pro — yalnizca lisanssiz kullaniciya + premium build'de gosterilir (community open-core'da Pro kavrami yok) */}
      {!isLicensed && PREMIUM_BUILD && (
        <div
          className={"nav-item" + (activeTab === "get-pro" ? " active premium-active" : "")}
          onClick={() => onTabChange("get-pro")}
          style={{
            background: activeTab === "get-pro" ? undefined : "linear-gradient(180deg, rgba(212,175,55,0.08) 0%, rgba(212,175,55,0.02) 100%)",
            border: "1px solid rgba(212,175,55,0.20)", borderRadius: 10,
          }}
        >
          <span className="ico" style={{ color: "#d4af37" }}><Icon.Throttle /></span>
          <span style={{ flex: 1 }}>{t("nav_get_pro")}</span>
        </div>
      )}

      <div className="nav-section-label" style={{ marginTop: 8 }}>{t("nav_oneclick")}</div>
      {buildOneClickNav().map((entry) => {
        if (entry.kind !== "leaf") return null;
        const Ico = Icon[entry.iconKey];
        const active = activeTab === entry.id;
        // Advanced Diagnosis ("smart") Pro; pasif gold gradient arka plan ile belli edilir (serit/PRO yok).
        const isProTool = entry.id === "smart";
        return (
          <div
            key={entry.id}
            className={"nav-item" + (active ? (isProTool ? " active premium-active" : " active") : "")}
            onClick={() => onTabChange(entry.id)}
            style={isProTool ? {
              background: "linear-gradient(180deg, rgba(212,175,55,0.07) 0%, rgba(212,175,55,0.02) 100%)",
              border: "1px solid rgba(212,175,55,0.13)",
              borderRadius: 10,
            } : undefined}
          >
            <span className="ico"><Ico /></span>
            <span style={{ flex: 1 }}>{entry.label}</span>
            {entry.badge && (
              <span style={{
                fontSize: 8, fontWeight: 700, letterSpacing: "0.05em",
                padding: "1px 5px", borderRadius: 3,
                background: "rgba(139,92,246,0.2)", color: "#a78bfa",
                border: "1px solid rgba(139,92,246,0.3)", flexShrink: 0,
              }}>{entry.badge}</span>
            )}
          </div>
        );
      })}

      {/* Kayan bölge: Logo+Dashboard+One-Click üstte SABİT, footer altta SABİT;
          yalnızca Araçlar/Premium/Geçmiş buradan itibaren kayar. */}
      <div className="sidebar-scroll">
      <div className="nav-section-label" style={{ marginTop: 0 }}>{t("nav_tools")}</div>
      {buildToolsNav().map((entry) => {
        if (entry.kind === "leaf") {
          const Ico = Icon[entry.iconKey];
          const active = activeTab === entry.id;
          return (
            <div
              key={entry.id}
              className={"nav-item" + (active ? " active" : "")}
              onClick={() => onTabChange(entry.id)}
            >
              <span className="ico"><Ico /></span>
              <span>{entry.label}</span>
            </div>
          );
        }

        // group ("Ping" → saf ping aracı + Servisler + Oyun Sunucuları)
        const Ico = Icon[entry.iconKey];
        const isOpen = openGroups[entry.id] ?? false;
        const childActive = entry.children.some(c => c.id === activeTab);
        return (
          <div key={entry.id}>
            <div
              className={"nav-item nav-group" + (childActive ? " group-active" : "")}
              onClick={() => toggleGroup(entry.id)}
            >
              <span className="ico"><Ico /></span>
              <span>{entry.label}</span>
              <span
                className="mono"
                style={{
                  marginLeft: "auto",
                  fontSize: 10,
                  color: "var(--text-muted)",
                  transition: "transform 150ms ease",
                  transform: isOpen ? "rotate(90deg)" : "rotate(0deg)",
                }}
              >
                ▶
              </span>
            </div>
            {isOpen && (
              <div className="nav-sub">
                {entry.children.map((c) => {
                  const CIco = Icon[c.iconKey];
                  const active = activeTab === c.id;
                  return (
                    <div
                      key={c.id}
                      className={"nav-item nav-sub-item" + (active ? " active" : "")}
                      onClick={() => onTabChange(c.id)}
                    >
                      <span className="ico"><CIco /></span>
                      <span>{c.label}</span>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        );
      })}

      {PREMIUM_BUILD && <>
      <div className="nav-section-label" style={{ marginTop: 8 }}>{t("nav_premium")}</div>
      {/* Premium grup: hafif gold gradient arka plan (pasif premium bolge isareti, PRO spam yok) */}
      <div style={{
        background: "linear-gradient(180deg, rgba(212,175,55,0.07) 0%, rgba(212,175,55,0.02) 100%)",
        border: "1px solid rgba(212,175,55,0.13)",
        borderRadius: 10,
        padding: "4px 2px",
        marginTop: 2,
      }}>
      {buildPremiumNav().map((entry) => {
        if (entry.kind === "leaf") {
          const Ico = Icon[entry.iconKey];
          const active = activeTab === entry.id;
          // Gold gradient arka plan grup wrapper'da; her arac normal (serit yok, PRO yazisi yok).
          return (
            <div
              key={entry.id}
              className={"nav-item" + (active ? " active premium-active" : "")}
              onClick={() => onTabChange(entry.id)}
            >
              <span className="ico"><Ico /></span>
              <span style={{ flex: 1 }}>{entry.label}</span>
            </div>
          );
        }
        return null;
      })}
      </div>
      </>}

      <div className="nav-section-label" style={{ marginTop: 8 }}>{t("nav_history")}</div>
      {buildHistoryNav().map((entry) => {
        const Ico = Icon[entry.iconKey];
        const active = activeTab === entry.id;
        return (
          <div
            key={entry.id}
            className={"nav-item" + (active ? " active" : "")}
            onClick={() => onTabChange(entry.id)}
          >
            <span className="ico"><Ico /></span>
            <span>{entry.label}</span>
          </div>
        );
      })}

      </div>{/* /.sidebar-scroll */}

      <div className="sidebar-foot">
        <StatusLine targetsLoaded={targetsLoaded} />
        <div
          className={"icon-btn" + (activeTab === "settings" ? " active" : "") + (hasUpdate ? " update-pulse" : "")}
          style={{
            width: 26, height: 26, borderRadius: 7, cursor: "pointer", flexShrink: 0,
            color: hasUpdate ? "#22c55e" : (activeTab === "settings" ? "var(--accent)" : undefined),
          }}
          title={hasUpdate ? t("settings_update_available") : t("settings_title")}
          onClick={() => onTabChange("settings")}
        >
          <Icon.Settings />
        </div>
      </div>
    </aside>
  );
}

import { useMemo, useState } from "react";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";
import { Icon } from "../../lib/icons";
import type { TabProps } from "./types";
import type { ServerPingResult, PingCategory } from "../../types";
import { GradeBox } from "../GradeBox";
import { gameGrade } from "../../lib/helpers";
import { HistoryView } from "../HistoryView";
import netflixLogo from "../../assets/brands/netflix.svg";
import youtubeLogo from "../../assets/brands/youtube.svg";
import discordLogo from "../../assets/brands/discord.svg";
import twitchLogo from "../../assets/brands/twitch.svg";
import spotifyLogo from "../../assets/brands/spotify.svg";
import cloudflareLogo from "../../assets/brands/cloudflare.svg";
import googleLogo from "../../assets/brands/google.svg";
import steamLogo from "../../assets/brands/steam.svg";
import epicgamesLogo from "../../assets/brands/epicgames.svg";
import riotgamesLogo from "../../assets/brands/riotgames.svg";
import playstationLogo from "../../assets/brands/playstation.svg";
import tiktokLogo from "../../assets/brands/tiktok.svg";
import redditLogo from "../../assets/brands/reddit.svg";
import githubLogo from "../../assets/brands/github.svg";
import telegramLogo from "../../assets/brands/telegram.svg";
import whatsappLogo from "../../assets/brands/whatsapp.svg";
import instagramLogo from "../../assets/brands/instagram.svg";
import facebookLogo from "../../assets/brands/facebook.svg";

type IcoComp = typeof Icon[keyof typeof Icon];

interface PingTabProps extends TabProps {
  /** Hangi alt sekme gösteriliyor: services veya games. */
  category: PingCategory;
  showHistory?: boolean;
}

type SortKey = "region" | "avg" | "min" | "max" | "jitter" | "loss" | "grade";
type SortDir = "asc" | "desc";

interface SortState {
  key: SortKey;
  dir: SortDir;
}

const GRADE_ORDER: Record<string, number> = {
  "A+": 0, A: 1, B: 2, C: 3, D: 4, F: 5,
};

function compareRows(a: ServerPingResult, b: ServerPingResult, key: SortKey, dir: SortDir): number {
  // Unreachable her zaman aşağıda kalsın
  if (a.reachable !== b.reachable) return a.reachable ? -1 : 1;

  let cmp = 0;
  switch (key) {
    case "region": cmp = a.region.localeCompare(b.region, "tr"); break;
    case "avg":    cmp = a.avg_ms - b.avg_ms;                    break;
    case "min":    cmp = a.min_ms - b.min_ms;                    break;
    case "max":    cmp = a.max_ms - b.max_ms;                    break;
    case "jitter": cmp = a.jitter_ms - b.jitter_ms;              break;
    case "loss":   cmp = a.packet_loss - b.packet_loss;          break;
    case "grade": {
      const ao = GRADE_ORDER[a.grade] ?? 99;
      const bo = GRADE_ORDER[b.grade] ?? 99;
      cmp = ao - bo;
      break;
    }
  }
  return dir === "asc" ? cmp : -cmp;
}

const FLAGS: Record<string, string> = {
  Istanbul: "🇹🇷", Frankfurt: "🇩🇪", Warsaw: "🇵🇱",
  London: "🇬🇧", Paris: "🇫🇷", Amsterdam: "🇳🇱",
  Stockholm: "🇸🇪", Madrid: "🇪🇸", Vienna: "🇦🇹",
  // GeForce NOW bölgeleri (önce spesifik EU ülkeleri, sonra genel US/CA — ilk-eşleşen kuralı)
  "EU United Kingdom": "🇬🇧", "EU Germany": "🇩🇪", "EU France": "🇫🇷",
  "EU Netherlands": "🇳🇱", "EU Sweden": "🇸🇪", "EU Poland": "🇵🇱",
  "EU Bulgaria": "🇧🇬",
  // Steam SDR şehirleri (global PoP'lar)
  "Sao Paulo": "🇧🇷", Ezeiza: "🇦🇷", "Santiago de Chile": "🇨🇱",
  Tokyo: "🇯🇵", Seoul: "🇰🇷", Singapore: "🇸🇬", "Hong Kong": "🇭🇰",
  Sydney: "🇦🇺", Johannesburg: "🇿🇦", Dubai: "🇦🇪",
  Atlanta: "🇺🇸", Chicago: "🇺🇸", Dallas: "🇺🇸",
  "Los Angeles": "🇺🇸", Seattle: "🇺🇸", Washington: "🇺🇸",
  // GeForce NOW genel (en sonda — spesifik şehir/ülkeler önce eşleşsin)
  "CA ": "🇨🇦", "US ": "🇺🇸",
};

/** Services kategori grup sırası (sabit). */
const GROUP_ORDER = ["social", "comm", "streaming", "gaming", "tech"] as const;
type GroupKey = typeof GROUP_ORDER[number];

/** Grup i18n anahtarı → translation key. */
const GROUP_I18N_KEY: Record<GroupKey, string> = {
  social:    "ping_group_social",
  comm:      "ping_group_comm",
  streaming: "ping_group_streaming",
  gaming:    "ping_group_gaming",
  tech:      "ping_group_tech",
};

/** Grup başlığı ikonları. */
const GROUP_ICONS: Record<GroupKey, IcoComp> = {
  social:    Icon.Social,
  comm:      Icon.Chat,
  streaming: Icon.Movie,
  gaming:    Icon.Game,
  tech:      Icon.Cloud,
};

/** Brand logos — service name → imported SVG URL (renkli, filtre yok). */
const BRAND_LOGOS: Record<string, string> = {
  "Cloudflare": cloudflareLogo,
  "Google": googleLogo,
  "YouTube": youtubeLogo,
  "Facebook": facebookLogo,
  "Instagram": instagramLogo,
  "WhatsApp": whatsappLogo,
  "Netflix": netflixLogo,
  "Discord": discordLogo,
  "Twitch": twitchLogo,
  "Spotify": spotifyLogo,
  "TikTok": tiktokLogo,
  "Reddit": redditLogo,
  "GitHub": githubLogo,
  "Epic Games": epicgamesLogo,
  "Riot Games": riotgamesLogo,
  "PlayStation": playstationLogo,
  "Telegram": telegramLogo,
  "Steam": steamLogo,
  "Steam / Valve (CS2 - DOTA2)": steamLogo,
};

/** Service adına göre ikon — group başlığı için (monochrome lucide). */
const SERVICE_ICONS: Record<string, IcoComp> = {
  Cloudflare:     Icon.Cloud,
  Google:         Icon.Search,
  Meta:           Icon.Social,
  Netflix:        Icon.Movie,
  Microsoft:      Icon.Window,
  Apple:          Icon.Apple,
  Amazon:         Icon.Cart,
  Discord:        Icon.Chat,
  Twitch:         Icon.Game,
  Spotify:        Icon.Music,
  "Steam / Valve (CS2 - DOTA2)": Icon.Crosshair,
  "GeForce NOW":  Icon.Cloud,
};

function flagFor(region: string): string {
  for (const key of Object.keys(FLAGS)) {
    if (region.toLowerCase().includes(key.toLowerCase())) return FLAGS[key];
  }
  return "🌐";
}

function iconFor(game: string) {
  const logo = BRAND_LOGOS[game];
  if (logo) {
    return (
      <img
        src={logo}
        alt={game}
        style={{ width: 16, height: 16, objectFit: "contain", flexShrink: 0, display: "block" }}
      />
    );
  }
  const Ico = SERVICE_ICONS[game] ?? Icon.Globe;
  return <Ico style={{ width: 14, height: 14, color: "var(--text-muted)" }} />;
}

/** Sortable column header. */
function SortHeader({
  label, sortKey, current, onSort, align = "left", style,
}: {
  label: string;
  sortKey: SortKey;
  current: SortState;
  onSort: (k: SortKey) => void;
  align?: "left" | "right";
  style?: React.CSSProperties;
}) {
  const active = current.key === sortKey;
  const arrow = active ? (current.dir === "asc" ? "▲" : "▼") : "↕";
  return (
    <th
      onClick={() => onSort(sortKey)}
      style={{
        cursor: "pointer",
        userSelect: "none",
        color: active ? "#c7d2fe" : undefined,
        transition: "color 120ms",
        ...style,
      }}
      onMouseEnter={(e) => {
        if (!active) (e.currentTarget as HTMLElement).style.color = "var(--text-dim)";
      }}
      onMouseLeave={(e) => {
        if (!active) (e.currentTarget as HTMLElement).style.color = "";
      }}
    >
      <span style={{ display: "inline-flex", alignItems: "center", gap: 5 }}>
        {align === "right" && (
          <span
            className="mono"
            style={{
              fontSize: 9,
              opacity: active ? 1 : 0.45,
              color: active ? "var(--accent)" : undefined,
            }}
          >
            {arrow}
          </span>
        )}
        {label}
        {align === "left" && (
          <span
            className="mono"
            style={{
              fontSize: 9,
              opacity: active ? 1 : 0.45,
              color: active ? "var(--accent)" : undefined,
            }}
          >
            {arrow}
          </span>
        )}
      </span>
    </th>
  );
}

/** Tek bir oyun grubu — kendi sort state'i ile. */
function GameTable({ game, rows, lossCapEnabled = true }: { game: string; rows: ServerPingResult[]; lossCapEnabled?: boolean }) {
  useLang();
  const [sort, setSort] = useState<SortState>({ key: "min", dir: "asc" });

  const handleSort = (key: SortKey) => {
    setSort(prev => {
      if (prev.key === key) {
        return { key, dir: prev.dir === "asc" ? "desc" : "asc" };
      }
      // Yeni kolon: numeric değerler için varsayılan asc (düşük=iyi),
      // grade için asc (A+ önce), region için asc
      return { key, dir: "asc" };
    });
  };

  /** Computed grade: reachable satırlar için gameGrade, unreachable için r.grade. */
  const computeGrade = (r: ServerPingResult): string =>
    r.reachable ? gameGrade(r.avg_ms, r.jitter_ms, r.packet_loss, lossCapEnabled) : r.grade;

  // Top-5: çok-sunuculu oyun gruplarında (5+ region) reachable'ları avg_ms'e göre
  // sıralayıp en iyi 5'i göster (GeForce NOW + Steam/Valve + ileride eklenecekler).
  // 5 veya daha az region'lı gruplar tümü gösterilir.
  // Grade sort: computed grade kullanılır (toggle'a duyarlı).
  const TOP5_THRESHOLD = 5;
  const displayRows = useMemo(() => {
    // Computed grade'i sort için row'a yansıt (shallow override — orijinal row bozulmaz)
    const withComputed = rows.map(r => ({ ...r, grade: computeGrade(r) }));
    const sorted = [...withComputed].sort((a, b) => compareRows(a, b, sort.key, sort.dir));
    if (rows.length <= TOP5_THRESHOLD) return sorted;
    const reachable = sorted.filter(r => r.reachable);
    const unreachable = sorted.filter(r => !r.reachable);
    // reachable'lar avg_ms'e göre sırala, top 5 al
    const top5 = [...reachable].sort((a, b) => a.avg_ms - b.avg_ms).slice(0, 5);
    // sort state'e göre top5'i yeniden sırala (kullanıcı sıralamasına saygı)
    top5.sort((a, b) => compareRows(a, b, sort.key, sort.dir));
    return [...top5, ...unreachable.slice(0, Math.max(0, 5 - top5.length))];
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rows, sort, game, lossCapEnabled]);

  const isGfn = game === "GeForce NOW";

  return (
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
        <div className="ico-wrap">
          <span style={{ fontSize: 14 }}>{iconFor(game)}</span>
        </div>
        <h3
          style={{
            margin: 0,
            fontSize: 13,
            fontWeight: 600,
            letterSpacing: "0.02em",
            textTransform: "uppercase",
          }}
        >
          {game}
        </h3>
        {isGfn && (
          <span
            style={{
              marginLeft: 8,
              fontSize: 10,
              color: "var(--text-muted)",
              background: "rgba(99,102,241,0.15)",
              border: "1px solid rgba(99,102,241,0.3)",
              borderRadius: 4,
              padding: "1px 6px",
              fontFamily: "var(--mono)",
            }}
            title={t("gfn_tcp_note")}
          >
            {t("gfn_cloud_edge")}
          </span>
        )}
        <span
          style={{
            marginLeft: "auto",
            fontFamily: "var(--mono)",
            fontSize: 11,
            color: "var(--text-muted)",
          }}
        >
          {displayRows.length < rows.length ? `top ${displayRows.length} / ${rows.length}` : rows.length} {t("ping_servers")} · {t("ping_sorted_by")} {sort.key} {sort.dir === "asc" ? "↑" : "↓"}
        </span>
      </div>

      <table className="servers-table" style={{ padding: "0 8px" }}>
        <thead>
          <tr>
            <SortHeader label={t("col_region")} sortKey="region" current={sort} onSort={handleSort} style={{ paddingLeft: 18 }} />
            <SortHeader label="Min"    sortKey="min"    current={sort} onSort={handleSort} />
            <SortHeader label="Avg"    sortKey="avg"    current={sort} onSort={handleSort} />
            <SortHeader label="Max"    sortKey="max"    current={sort} onSort={handleSort} />
            <SortHeader label="Jitter" sortKey="jitter" current={sort} onSort={handleSort} />
            <SortHeader label="Loss"   sortKey="loss"   current={sort} onSort={handleSort} />
            <SortHeader label="Grade"  sortKey="grade"  current={sort} onSort={handleSort} align="right" style={{ paddingRight: 18 }} />
          </tr>
        </thead>
        <tbody>
          {displayRows.map((r, i) => {
            return (
              <tr key={i}>
                <td style={{ paddingLeft: 18 }}>
                  <div className="server-cell">
                    <div className="flag">{flagFor(r.region)}</div>
                    <div>
                      <div className="region-name">{r.region}</div>
                      <div className="region-city">{r.display}</div>
                    </div>
                  </div>
                </td>
                {r.reachable ? (
                  <>
                    <td>
                      <span className="mono" style={{ color: "var(--text-dim)" }}>
                        {r.min_ms.toFixed(0)}ms
                      </span>
                    </td>
                    <td>
                      <span className="mono">
                        {r.avg_ms.toFixed(0)}
                        <span style={{ color: "var(--text-muted)" }}>ms</span>
                      </span>
                    </td>
                    <td>
                      <span className="mono" style={{ color: "var(--text-dim)" }}>
                        {r.max_ms.toFixed(0)}ms
                      </span>
                    </td>
                    <td>
                      <span className="mono" style={{ color: "var(--text-dim)" }}>
                        {r.jitter_ms.toFixed(1)}ms
                      </span>
                    </td>
                    <td>
                      <span
                        className="mono"
                        style={{ color: r.packet_loss > 0 ? "var(--danger)" : "var(--text-muted)" }}
                      >
                        {r.packet_loss > 0 ? `${r.packet_loss.toFixed(1)}%` : "—"}
                      </span>
                    </td>
                    <td style={{ paddingRight: 18 }}>
                      <span style={{ display: "inline-flex", justifyContent: "flex-end" }}>
                        <GradeBox grade={r.grade} size="sm" />
                      </span>
                    </td>
                  </>
                ) : (
                  <td
                    colSpan={6}
                    style={{ textAlign: "center", color: "var(--text-muted)", fontSize: 12, paddingRight: 18 }}
                  >
                    {t("ping_unreachable")}
                  </td>
                )}
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

/** Services kategorisinde tek bir grup bölümü (Sosyal Medya, İletişim vb.). */
function ServiceGroupTable({ groupKey, rows, running }: {
  groupKey: GroupKey;
  rows: ServerPingResult[];
  running: boolean;
}) {
  useLang();
  const [sort, setSort] = useState<SortState>({ key: "avg", dir: "asc" });

  const handleSort = (key: SortKey) => {
    setSort(prev =>
      prev.key === key
        ? { key, dir: prev.dir === "asc" ? "desc" : "asc" }
        : { key, dir: "asc" }
    );
  };

  const displayRows = useMemo(
    () => [...rows].sort((a, b) => compareRows(a, b, sort.key, sort.dir)),
    [rows, sort]
  );

  const Ico = GROUP_ICONS[groupKey] ?? Icon.Globe;
  const title = t(GROUP_I18N_KEY[groupKey] as Parameters<typeof t>[0]);

  return (
    <div className="card" style={{ padding: 0, overflow: "hidden" }}>
      <div style={{
        display: "flex", alignItems: "center", gap: 10,
        padding: "12px 18px", borderBottom: "1px solid var(--border)",
        background: "rgba(10,10,15,0.4)",
      }}>
        <div className="ico-wrap">
          <Ico style={{ width: 14, height: 14, color: "var(--text-muted)" }} />
        </div>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
          {title}
        </h3>
        {running && (
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--accent)", animation: "pulse 1.2s infinite" }}>
            ● {t("ping_measuring")}
          </span>
        )}
        {!running && (
          <span style={{ marginLeft: "auto", fontFamily: "var(--mono)", fontSize: 11, color: "var(--text-muted)" }}>
            {rows.length} {t("ping_servers")}
          </span>
        )}
      </div>
      <table className="servers-table" style={{ padding: "0 8px" }}>
        <thead>
          <tr>
            <SortHeader label={t("col_region")} sortKey="region" current={sort} onSort={handleSort} style={{ paddingLeft: 18 }} />
            <SortHeader label="Min"    sortKey="min"    current={sort} onSort={handleSort} />
            <SortHeader label="Avg"    sortKey="avg"    current={sort} onSort={handleSort} />
            <SortHeader label="Max"    sortKey="max"    current={sort} onSort={handleSort} />
            <SortHeader label="Jitter" sortKey="jitter" current={sort} onSort={handleSort} />
            <SortHeader label="Loss"   sortKey="loss"   current={sort} onSort={handleSort} />
            <SortHeader label="Grade"  sortKey="grade"  current={sort} onSort={handleSort} align="right" style={{ paddingRight: 18 }} />
          </tr>
        </thead>
        <tbody>
          {displayRows.map((r, i) => (
            <tr key={i}>
              <td style={{ paddingLeft: 18 }}>
                <div className="server-cell">
                  <div className="flag">{iconFor(r.name)}</div>
                  <div>
                    <div className="region-name">{r.name}</div>
                    <div className="region-city">{r.display}</div>
                  </div>
                </div>
              </td>
              {r.reachable ? (
                <>
                  <td><span className="mono" style={{ color: "var(--text-dim)" }}>{r.min_ms.toFixed(0)}ms</span></td>
                  <td><span className="mono">{r.avg_ms.toFixed(0)}<span style={{ color: "var(--text-muted)" }}>ms</span></span></td>
                  <td><span className="mono" style={{ color: "var(--text-dim)" }}>{r.max_ms.toFixed(0)}ms</span></td>
                  <td><span className="mono" style={{ color: "var(--text-dim)" }}>{r.jitter_ms.toFixed(1)}ms</span></td>
                  <td>
                    <span className="mono" style={{ color: r.packet_loss > 0 ? "var(--danger)" : "var(--text-muted)" }}>
                      {r.packet_loss > 0 ? `${r.packet_loss.toFixed(1)}%` : "—"}
                    </span>
                  </td>
                  <td style={{ paddingRight: 18 }}>
                    <span style={{ display: "inline-flex", justifyContent: "flex-end" }}>
                      <GradeBox grade={r.grade} size="sm" />
                    </span>
                  </td>
                </>
              ) : (
                <td colSpan={6} style={{ textAlign: "center", color: "var(--text-muted)", fontSize: 12, paddingRight: 18 }}>
                  {t("ping_unreachable")}
                </td>
              )}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** Skeleton row for services group table. */
function ServiceGroupSkeleton({ groupKey, names, running }: {
  groupKey: GroupKey;
  names: string[];
  running: boolean;
}) {
  const Ico = GROUP_ICONS[groupKey] ?? Icon.Globe;
  const title = t(GROUP_I18N_KEY[groupKey] as Parameters<typeof t>[0]);
  return (
    <div className="card" style={{ padding: 0, overflow: "hidden" }}>
      <div style={{
        display: "flex", alignItems: "center", gap: 10,
        padding: "12px 18px", borderBottom: "1px solid var(--border)",
        background: "rgba(10,10,15,0.4)",
      }}>
        <div className="ico-wrap">
          <Ico style={{ width: 14, height: 14, color: "var(--text-muted)" }} />
        </div>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
          {title}
        </h3>
        {running && (
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--accent)", animation: "pulse 1.2s infinite" }}>
            ● {t("ping_measuring")}
          </span>
        )}
      </div>
      <table className="servers-table" style={{ padding: "0 8px" }}>
        <thead>
          <tr>
            <th style={{ paddingLeft: 18 }}>{t("col_region")}</th>
            <th>{t("col_min")}</th><th>{t("col_avg")}</th><th>{t("col_max")}</th><th>{t("col_jitter")}</th><th>{t("col_loss")}</th>
            <th style={{ paddingRight: 18 }}>{t("col_grade")}</th>
          </tr>
        </thead>
        <tbody>
          {names.map(n => <SkeletonRow key={n} region={n} />)}
        </tbody>
      </table>
    </div>
  );
}

/** Skeleton services group map — group → isim listesi (fallback). */
const SKELETON_SERVICES_BY_GROUP: Record<GroupKey, string[]> = {
  social:    ["Facebook", "Instagram", "YouTube", "TikTok", "Reddit", "Twitch"],
  comm:      ["WhatsApp", "Discord", "Telegram"],
  streaming: ["Netflix", "Netflix (fast.com)", "Spotify"],
  gaming:    ["Xbox", "Steam", "Epic Games", "Riot Games", "PlayStation"],
  tech:      ["Cloudflare", "Google", "Microsoft", "Apple", "iCloud", "Amazon", "Amazon Web Services", "GitHub", "Fastly"],
};

const SKELETON_GAMES: Record<string, string[]> = {
  "Steam / Valve (CS2 - DOTA2)": ["Vienna", "Frankfurt", "Warsaw", "Madrid", "Amsterdam"],
  Valorant:     ["TR", "EU West", "EU North", "EU East"],
  "GeForce NOW": ["EU Germany", "EU France 1", "EU Netherlands North", "EU United Kingdom 1", "EU Sweden"],
};

function SkeletonRow({ region }: { region: string }) {
  return (
    <tr style={{ opacity: 0.45 }}>
      <td style={{ paddingLeft: 18 }}>
        <div className="server-cell">
          <div className="flag">•</div>
          <div>
            <div className="region-name">{region}</div>
          </div>
        </div>
      </td>
      {[0,1,2,3,4].map(i => (
        <td key={i}>
          <span className="mono" style={{ color: "var(--text-muted)" }}>—</span>
        </td>
      ))}
      <td style={{ paddingRight: 18 }}>
        <span style={{ display: "inline-flex", justifyContent: "flex-end", opacity: 0.4 }}>
          <GradeBox size="sm" />
        </span>
      </td>
    </tr>
  );
}

// Çok-sunuculu gruplar (GeForce NOW / Steam / ...) test edilmemişken:
// liste yerine ortalı uyarı metni. hintKey = gruba özel i18n anahtarı.
function EmptyHintCard({ game, hintKey, running }: { game: string; hintKey: string; running: boolean }) {
  return (
    <div className="card" style={{ padding: 0, overflow: "hidden" }}>
      <div
        style={{
          display: "flex", alignItems: "center", gap: 10,
          padding: "12px 18px", borderBottom: "1px solid var(--border)",
          background: "rgba(10,10,15,0.4)",
        }}
      >
        <div className="ico-wrap"><span style={{ fontSize: 14 }}>{iconFor(game)}</span></div>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
          {game}
        </h3>
        {running && (
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--accent)", animation: "pulse 1.2s infinite" }}>
            ● {t("ping_measuring")}
          </span>
        )}
      </div>
      <div style={{ padding: "28px 18px", textAlign: "center", color: "var(--text-muted)", fontSize: 13, lineHeight: 1.6 }}>
        {t(hintKey as Parameters<typeof t>[0])}
      </div>
    </div>
  );
}

// Test öncesi liste yerine uyarı metni gösterecek çok-sunuculu gruplar (top-5 mantığıyla uyumlu).
// İleride yeni oyun grubu eklenince buraya hint anahtarı eklenir.
const EMPTY_HINT_GROUPS: Record<string, string> = {
  "GeForce NOW": "gfn_empty_hint",
  "Steam / Valve (CS2 - DOTA2)": "steam_empty_hint",
};

function SkeletonTable({ game, regions, running }: { game: string; regions: string[]; running: boolean }) {
  return (
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
        <div className="ico-wrap">
          <span style={{ fontSize: 14 }}>{iconFor(game)}</span>
        </div>
        <h3 style={{ margin: 0, fontSize: 13, fontWeight: 600, letterSpacing: "0.02em", textTransform: "uppercase" }}>
          {game}
        </h3>
        {running && (
          <span style={{ marginLeft: "auto", fontSize: 11, color: "var(--accent)", animation: "pulse 1.2s infinite" }}>
            ● {t("ping_measuring")}
          </span>
        )}
      </div>
      <table className="servers-table" style={{ padding: "0 8px" }}>
        <thead>
          <tr>
            <th style={{ paddingLeft: 18 }}>{t("col_region")}</th>
            <th>{t("col_min")}</th><th>{t("col_avg")}</th><th>{t("col_max")}</th><th>{t("col_jitter")}</th><th>{t("col_loss")}</th>
            <th style={{ paddingRight: 18 }}>{t("col_grade")}</th>
          </tr>
        </thead>
        <tbody>
          {regions.map(r => <SkeletonRow key={r} region={r} />)}
        </tbody>
      </table>
    </div>
  );
}

export function PingTab({ results, phase, category, pingTargets, showHistory }: PingTabProps) {
  useLang();
  const { pings } = results;
  const isRunning = phase === "running_ping";
  const [lossCapEnabled, setLossCapEnabled] = useState(true);

  if (showHistory) {
    return <HistoryView module="ping" title={t("history_title")} />;
  }

  // ── SERVICES: group-based rendering ─────────────────────────────────────────
  if (category === "services") {
    const serviceTargets = pingTargets.filter(pt => pt.category === "services");

    // Build dynamic skeleton group map from server targets
    const dynamicGroupMap: Partial<Record<GroupKey, string[]>> = {};
    for (const pt of serviceTargets) {
      const raw = (pt.group ?? "tech") as string;
      const gk: GroupKey = (GROUP_ORDER as readonly string[]).includes(raw) ? raw as GroupKey : "tech";
      if (!dynamicGroupMap[gk]) dynamicGroupMap[gk] = [];
      dynamicGroupMap[gk]!.push(pt.name);
    }
    const skeletonGroupMap: Record<GroupKey, string[]> =
      Object.keys(dynamicGroupMap).length > 0
        ? (dynamicGroupMap as Record<GroupKey, string[]>)
        : SKELETON_SERVICES_BY_GROUP;

    if (!pings) {
      return (
        <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
          {GROUP_ORDER.map(gk => {
            const names = skeletonGroupMap[gk];
            if (!names || names.length === 0) return null;
            return <ServiceGroupSkeleton key={gk} groupKey={gk} names={names} running={isRunning} />;
          })}
        </div>
      );
    }

    // Results available: group by `group` field from pingTargets
    const filtered = pings.filter(p => p.category === "services");
    const groupResultMap: Partial<Record<GroupKey, ServerPingResult[]>> = {};
    for (const r of filtered) {
      const target = serviceTargets.find(pt => pt.host === r.host);
      const raw = (target?.group ?? "tech") as string;
      const gk: GroupKey = (GROUP_ORDER as readonly string[]).includes(raw) ? raw as GroupKey : "tech";
      if (!groupResultMap[gk]) groupResultMap[gk] = [];
      groupResultMap[gk]!.push(r);
    }

    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {GROUP_ORDER.map(gk => {
          const rows = groupResultMap[gk];
          if (rows && rows.length > 0) {
            return <ServiceGroupTable key={gk} groupKey={gk} rows={rows} running={isRunning} />;
          }
          const names = skeletonGroupMap[gk];
          if (!names || names.length === 0) return null;
          return <ServiceGroupSkeleton key={gk} groupKey={gk} names={names} running={isRunning} />;
        })}
        <p style={{ margin: 0, fontSize: 10, color: "var(--text-muted)", opacity: 0.6, lineHeight: 1.5 }}>
          {t("trademark_short")}
        </p>
      </div>
    );
  }

  // ── GAMES: original name-based rendering (unchanged) ───────────────────────
  const serverTargets = pingTargets.filter(pt => pt.category === category);
  const dynamicSkeletonMap: Record<string, string[]> = {};

  if (serverTargets.length > 0) {
    for (const pt of serverTargets) {
      if (!dynamicSkeletonMap[pt.name]) dynamicSkeletonMap[pt.name] = [];
      // GeForce NOW: skeleton'da en fazla 5 region (Yol A top-5; gercek en-yakin 5
      // olcum sonucu GameTable'da RTT'ye gore secilir). 21 region listesi sismesin.
      if (pt.name === "GeForce NOW" && dynamicSkeletonMap[pt.name].length >= 5) continue;
      dynamicSkeletonMap[pt.name].push(pt.region);
    }
  }

  const skeletonMap = Object.keys(dynamicSkeletonMap).length > 0 ? dynamicSkeletonMap : SKELETON_GAMES;

  /** Loss cap toggle bar — yalnızca category="games" iken gösterilir. */
  const lossCapBar = (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 10,
        padding: "10px 14px",
        background: "rgba(10,10,15,0.55)",
        border: "1px solid var(--border)",
        borderRadius: 8,
      }}
    >
      <input
        id="loss-cap-toggle"
        type="checkbox"
        checked={lossCapEnabled}
        onChange={e => setLossCapEnabled(e.target.checked)}
        style={{ cursor: "pointer", accentColor: "var(--accent)", width: 14, height: 14 }}
      />
      <label
        htmlFor="loss-cap-toggle"
        style={{ cursor: "pointer", fontSize: 13, fontWeight: 500, userSelect: "none" }}
      >
        {t("games_loss_cap_toggle")}
      </label>
      <span style={{ fontSize: 11, color: "var(--text-muted)", marginLeft: 4 }}>
        {t("games_loss_cap_hint")}
      </span>
    </div>
  );

  if (!pings) {
    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
        {lossCapBar}
        {Object.entries(skeletonMap).map(([game, regions]) =>
          EMPTY_HINT_GROUPS[game]
            ? <EmptyHintCard key={game} game={game} hintKey={EMPTY_HINT_GROUPS[game]} running={isRunning} />
            : <SkeletonTable key={game} game={game} regions={regions} running={isRunning} />
        )}
      </div>
    );
  }

  const filtered = pings.filter(p => p.category === category);
  const games = Array.from(new Set(filtered.map(p => p.name)));

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 20 }}>
      {lossCapBar}
      {Object.keys(skeletonMap).map(game => {
        const rows = filtered.filter(p => p.name === game);
        if (rows.length > 0) {
          return <GameTable key={game} game={game} rows={rows} lossCapEnabled={lossCapEnabled} />;
        }
        if (EMPTY_HINT_GROUPS[game]) {
          return <EmptyHintCard key={game} game={game} hintKey={EMPTY_HINT_GROUPS[game]} running={isRunning} />;
        }
        return <SkeletonTable key={game} game={game} regions={skeletonMap[game]} running={isRunning} />;
      })}
      {/* Skeleton'da olmayan ama sonuç gelen extra game'ler */}
      {games.filter(g => !skeletonMap[g]).map(game => (
        <GameTable key={game} game={game} rows={filtered.filter(p => p.name === game)} lossCapEnabled={lossCapEnabled} />
      ))}
    </div>
  );
}

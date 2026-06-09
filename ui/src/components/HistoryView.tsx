import { useEffect, useState, Fragment } from "react";
import type { ReactNode, CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../lib/i18n";
import { useLang } from "../lib/useLang";
import { Sparkline } from "../lib/sparkline";
import type { TestRunRow } from "../types";

interface Props {
  module?: string;
  title?: string;
  /** default true. Ana TabPane hep-mount olduğu için active geçince yeniden yükler. */
  active?: boolean;
}

function parseMetrics(raw: string): Record<string, unknown> {
  try { return JSON.parse(raw) as Record<string, unknown>; }
  catch { return {}; }
}

function num(v: unknown): string {
  if (v === null || v === undefined) return "—";
  const n = Number(v);
  return isNaN(n) ? "—" : n.toFixed(1);
}

function VerdictBadge({ v }: { v: string }) {
  const colors: Record<string, string> = { Normal: "#22c55e", Throttled: "#f97316", Blocked: "#ef4444" };
  const c = colors[v] ?? "var(--text-muted)";
  return <span style={{ fontFamily: "var(--mono)", fontWeight: 700, fontSize: 11, color: c, border: `1px solid ${c}44`, borderRadius: 4, padding: "1px 5px", background: `${c}11` }}>{v || "—"}</span>;
}

function GradeBadge({ grade }: { grade: string }) {
  const colors: Record<string, string> = {
    "A+": "#22c55e", A: "#4ade80", B: "#86efac",
    C: "#fbbf24", D: "#f97316", F: "#ef4444",
  };
  const color = colors[grade] ?? "var(--text-muted)";
  return (
    <span style={{
      fontFamily: "var(--mono)", fontWeight: 700, fontSize: 13,
      color, border: `1px solid ${color}44`,
      borderRadius: 5, padding: "1px 6px",
      background: `${color}11`,
    }}>
      {grade}
    </span>
  );
}

/** Mono-font wrapper for numeric cells */
function Mono({ children }: { children: ReactNode }) {
  return <span style={{ fontFamily: "var(--mono)" }}>{children}</span>;
}

type SortVal = number | string;

interface Col {
  key: string;
  label: () => string;
  align?: "left" | "right";
  sortValue: (row: TestRunRow, m: Record<string, unknown>) => SortVal;
  render: (row: TestRunRow, m: Record<string, unknown>) => ReactNode;
}

const DATE_COL: Col = {
  key: "date",
  label: () => t("hist_col_date"),
  align: "left",
  sortValue: (row) => row.ts,
  render: (row) => new Date(row.ts).toLocaleString(),
};

const COLS: Record<string, Col[]> = {
  speed: [
    DATE_COL,
    { key: "dl",     label: () => t("hist_col_dl"),     align: "right", sortValue: (_, m) => Number(m.dl)     || 0, render: (_, m) => <Mono>{num(m.dl)}</Mono> },
    { key: "ul",     label: () => t("hist_col_ul"),     align: "right", sortValue: (_, m) => Number(m.ul)     || 0, render: (_, m) => <Mono>{num(m.ul)}</Mono> },
    { key: "ping",   label: () => t("hist_col_ping"),   align: "right", sortValue: (_, m) => Number(m.ping)   || 0, render: (_, m) => <Mono>{num(m.ping)}</Mono> },
    { key: "jitter", label: () => t("hist_col_jitter"), align: "right", sortValue: (_, m) => Number(m.jitter) || 0, render: (_, m) => <Mono>{num(m.jitter)}</Mono> },
    { key: "server", label: () => t("hist_col_server"), align: "left",  sortValue: (_, m) => String(m.server ?? ""), render: (_, m) => String(m.server ?? "—") },
  ],
  bufferbloat: [
    DATE_COL,
    { key: "idle",   label: () => t("hist_col_idle"),   align: "right", sortValue: (_, m) => Number(m.idle)   || 0, render: (_, m) => <Mono>{num(m.idle)}</Mono> },
    { key: "loaded", label: () => t("hist_col_loaded"), align: "right", sortValue: (_, m) => Number(m.loaded) || 0, render: (_, m) => <Mono>{num(m.loaded)}</Mono> },
    { key: "delta",  label: () => t("hist_col_delta"),  align: "right", sortValue: (_, m) => Number(m.delta)  || 0, render: (_, m) => <Mono>{num(m.delta)}</Mono> },
    {
      key: "grade", label: () => t("hist_col_grade"), align: "left",
      sortValue: (_, m) => String(m.grade ?? ""),
      render: (_, m) => m.grade ? <GradeBadge grade={String(m.grade)} /> : "—",
    },
  ],
  ping: [
    DATE_COL,
    { key: "best",   label: () => t("hist_col_best"),   align: "right", sortValue: (_, m) => Number(m.best)   || 0, render: (_, m) => <Mono>{num(m.best)}</Mono> },
    { key: "jitter", label: () => t("hist_col_jitter"), align: "right", sortValue: (_, m) => Number(m.jitter) || 0, render: (_, m) => <Mono>{num(m.jitter)}</Mono> },
    { key: "loss",   label: () => t("hist_col_loss"),   align: "right", sortValue: (_, m) => Number(m.loss)   || 0, render: (_, m) => <Mono>{num(m.loss)}</Mono> },
  ],
  voip: [
    DATE_COL,
    { key: "rtt",    label: () => t("hist_col_rtt"),    align: "right", sortValue: (_, m) => Number(m.rtt)    || 0, render: (_, m) => <Mono>{num(m.rtt)}</Mono> },
    { key: "jitter", label: () => t("hist_col_jitter"), align: "right", sortValue: (_, m) => Number(m.jitter) || 0, render: (_, m) => <Mono>{num(m.jitter)}</Mono> },
    { key: "loss",   label: () => t("hist_col_loss"),   align: "right", sortValue: (_, m) => Number(m.loss)   || 0, render: (_, m) => <Mono>{num(m.loss)}</Mono> },
    { key: "mos",    label: () => t("hist_col_mos"),    align: "right", sortValue: (_, m) => Number(m.mos)    || 0, render: (_, m) => m.mos != null ? <Mono>{num(m.mos)}</Mono> : "—" },
    { key: "grade",  label: () => t("hist_col_grade"),  align: "left",  sortValue: (_, m) => String(m.grade ?? ""), render: (_, m) => m.grade ? <GradeBadge grade={String(m.grade)} /> : "—" },
  ],
  throttle: [
    DATE_COL,
    { key: "baseline", label: () => t("hist_col_baseline"), align: "right", sortValue: (_, m) => Number(m.baseline) || 0, render: (_, m) => <Mono>{num(m.baseline)}</Mono> },
    { key: "bt_v",     label: () => t("hist_col_bt"),       align: "left",  sortValue: (_, m) => String(m.bt_v ?? ""),    render: (_, m) => m.bt_v   ? <VerdictBadge v={String(m.bt_v)} />   : "—" },
    { key: "quic_v",   label: () => t("hist_col_quic"),     align: "left",  sortValue: (_, m) => String(m.quic_v ?? ""),  render: (_, m) => m.quic_v ? <VerdictBadge v={String(m.quic_v)} /> : "—" },
    { key: "wg_v",     label: () => t("hist_col_wg"),       align: "left",  sortValue: (_, m) => String(m.wg_v ?? ""),    render: (_, m) => m.wg_v   ? <VerdictBadge v={String(m.wg_v)} />   : "—" },
    { key: "ovpn_v",   label: () => t("hist_col_ovpn"),     align: "left",  sortValue: (_, m) => String(m.ovpn_v ?? ""),  render: (_, m) => m.ovpn_v ? <VerdictBadge v={String(m.ovpn_v)} /> : "—" },
  ],
  lan_speed: [
    DATE_COL,
    { key: "dl",      label: () => t("hist_col_dl"),       align: "right", sortValue: (_, m) => Number(m.dl)      || 0, render: (_, m) => <Mono>{num(m.dl)}</Mono> },
    { key: "ul",      label: () => t("hist_col_ul"),       align: "right", sortValue: (_, m) => Number(m.ul)      || 0, render: (_, m) => <Mono>{num(m.ul)}</Mono> },
    { key: "dl_peak", label: () => t("hist_col_dl_peak"),  align: "right", sortValue: (_, m) => Number(m.dl_peak) || 0, render: (_, m) => <Mono>{num(m.dl_peak)}</Mono> },
    { key: "ul_peak", label: () => t("hist_col_ul_peak"),  align: "right", sortValue: (_, m) => Number(m.ul_peak) || 0, render: (_, m) => <Mono>{num(m.ul_peak)}</Mono> },
    { key: "streams", label: () => t("hist_col_streams"),  align: "right", sortValue: (_, m) => Number(m.streams) || 0, render: (_, m) => m.streams != null ? <Mono>{String(Math.round(Number(m.streams)))}</Mono> : "—" },
    { key: "peer",    label: () => t("hist_col_peer"),     align: "left",  sortValue: (_, m) => String(m.peer ?? ""), render: (_, m) => String(m.peer ?? "—") },
  ],
  _main: [
    DATE_COL,
    { key: "module", label: () => t("hist_col_module"), align: "left",  sortValue: (row) => row.module, render: (row) => row.module },
    {
      key: "summary_score", label: () => t("hist_col_score"), align: "right",
      sortValue: (row) => row.summary_score ?? -Infinity,
      render: (row) => row.summary_score !== null ? <Mono>{String(row.summary_score)}</Mono> : "—",
    },
    {
      key: "summary_grade", label: () => t("hist_col_grade"), align: "left",
      sortValue: (row) => row.summary_grade ?? "",
      render: (row) => row.summary_grade ? <GradeBadge grade={row.summary_grade} /> : "—",
    },
  ],
};

function compareSortVal(a: SortVal, b: SortVal): number {
  if (typeof a === "number" && typeof b === "number") return a - b;
  return String(a).localeCompare(String(b));
}

/** Maps module slug to a readable label using i18n */
function moduleLabel(mod: string): string {
  const map: Record<string, string> = {
    speed:       t("nav_speedtest"),
    bufferbloat: t("nav_bufferbloat"),
    ping:        t("hist_mod_ping"),
    throttle:    t("nav_throttle"),
    voip:        t("nav_voip_quality"),
    lan_speed:   t("nav_lan_speed"),
    full:        t("diag_full_test"),
  };
  return map[mod] ?? mod;
}

// ─── Main-screen accordion + filter render ───────────────────────────────────

const inputStyle: CSSProperties = {
  background: "rgba(255,255,255,0.05)",
  border: "1px solid var(--border)",
  color: "var(--text)",
  borderRadius: 6,
  padding: "4px 8px",
  fontSize: 12,
  colorScheme: "dark" as CSSProperties["colorScheme"],
};

const selectStyle: CSSProperties = {
  ...inputStyle,
  cursor: "pointer",
};

interface MainHistoryProps {
  rows: TestRunRow[];
}

function MainHistoryView({ rows }: MainHistoryProps) {
  useLang();

  const [filterModule, setFilterModule] = useState<string>("");
  const [dateFrom, setDateFrom]         = useState<string>("");
  const [dateTo, setDateTo]             = useState<string>("");
  const [sortKey, setSortKey]           = useState<string>("date");
  const [sortDir, setSortDir]           = useState<"asc" | "desc">("desc");
  const [openRows, setOpenRows]         = useState<Set<number>>(new Set());

  function toggleRow(id: number) {
    setOpenRows(prev => {
      const next = new Set(prev);
      next.has(id) ? next.delete(id) : next.add(id);
      return next;
    });
  }

  function handleSort(key: string) {
    if (key === sortKey) {
      setSortDir(d => d === "asc" ? "desc" : "asc");
    } else {
      setSortKey(key);
      setSortDir("desc");
    }
  }

  function clearFilters() {
    setFilterModule("");
    setDateFrom("");
    setDateTo("");
  }

  const hasFilter = filterModule !== "" || dateFrom !== "" || dateTo !== "";

  const filtered = rows.filter(r => {
    if (filterModule && r.module !== filterModule) return false;
    if (dateFrom) {
      const from = new Date(dateFrom + "T00:00:00").getTime();
      if (r.ts < from) return false;
    }
    if (dateTo) {
      const to = new Date(dateTo + "T23:59:59").getTime();
      if (r.ts > to) return false;
    }
    return true;
  });

  // Trend sparkline from filtered rows (scored, oldest→newest)
  const scored = [...filtered].filter(r => r.summary_score !== null).reverse();
  const sparkData = scored.map(r => r.summary_score as number);

  // Sort columns for the main 3-column view
  const mainCols: Array<{ key: string; label: () => string; align: "left" | "right"; sortValue: (r: TestRunRow) => SortVal }> = [
    { key: "date",   label: () => t("hist_col_date"),  align: "left",  sortValue: r => r.ts },
    { key: "module", label: () => t("hist_filter_module"), align: "left",  sortValue: r => r.module },
    { key: "grade",  label: () => t("hist_col_grade"), align: "left",  sortValue: r => r.summary_grade ?? "" },
  ];

  const activeCol = mainCols.find(c => c.key === sortKey) ?? mainCols[0];

  const sorted = [...filtered].sort((a, b) => {
    const va = activeCol.sortValue(a);
    const vb = activeCol.sortValue(b);
    const aIsBottom = va === -Infinity || va === "";
    const bIsBottom = vb === -Infinity || vb === "";
    if (aIsBottom && bIsBottom) return 0;
    if (aIsBottom) return 1;
    if (bIsBottom) return -1;
    const cmp = compareSortVal(va, vb);
    return sortDir === "asc" ? cmp : -cmp;
  });

  const thStyle: CSSProperties = {
    padding: "6px 10px",
    fontSize: 11,
    fontWeight: 600,
    color: "var(--text-muted)",
    textTransform: "uppercase",
    letterSpacing: "0.08em",
    borderBottom: "1px solid var(--border)",
    cursor: "pointer",
    userSelect: "none",
    whiteSpace: "nowrap",
  };

  const tdStyle: CSSProperties = {
    padding: "7px 10px",
    fontSize: 12,
    borderBottom: "1px solid var(--border)",
    color: "var(--text)",
  };

  return (
    <>
      {/* ── Filter bar ──────────────────────────────────────────────────── */}
      <div className="card" style={{ padding: "10px 14px" }}>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 10, alignItems: "center" }}>
          {/* Module filter */}
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}>
              {t("hist_filter_module")}
            </span>
            <select
              style={selectStyle}
              value={filterModule}
              onChange={e => setFilterModule(e.target.value)}
            >
              <option value="">{t("hist_filter_all")}</option>
              <option value="speed">{t("nav_speedtest")}</option>
              <option value="bufferbloat">{t("nav_bufferbloat")}</option>
              <option value="ping">{t("hist_mod_ping")}</option>
              <option value="throttle">{t("nav_throttle")}</option>
              <option value="voip">{t("nav_voip_quality")}</option>
              <option value="lan_speed">{t("nav_lan_speed")}</option>
              <option value="full">{t("diag_full_test")}</option>
            </select>
          </div>

          {/* Date range */}
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <span style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}>
              {t("hist_filter_date")}
            </span>
            <input
              type="date"
              style={inputStyle}
              value={dateFrom}
              onChange={e => setDateFrom(e.target.value)}
            />
            <span style={{ fontSize: 11, color: "var(--text-muted)" }}>–</span>
            <input
              type="date"
              style={inputStyle}
              value={dateTo}
              onChange={e => setDateTo(e.target.value)}
            />
          </div>

          {/* Clear button — only visible when a filter is active */}
          {hasFilter && (
            <button
              onClick={clearFilters}
              style={{
                background: "rgba(255,255,255,0.06)",
                border: "1px solid var(--border)",
                color: "var(--text-muted)",
                borderRadius: 6,
                padding: "3px 10px",
                fontSize: 11,
                cursor: "pointer",
              }}
            >
              {t("hist_filter_clear")}
            </button>
          )}
        </div>
      </div>

      {/* ── Sparkline ───────────────────────────────────────────────────── */}
      {sparkData.length >= 2 && (
        <div className="card" style={{ padding: "12px 16px" }}>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 8, textTransform: "uppercase", letterSpacing: "0.1em" }}>
            {t("history_trend")}
          </div>
          <Sparkline data={sparkData} color="var(--accent)" width={200} height={36} />
        </div>
      )}

      {/* ── Table ───────────────────────────────────────────────────────── */}
      {sorted.length === 0 ? (
        <div className="card" style={{ padding: 28, textAlign: "center" }}>
          <p style={{ margin: 0, fontSize: 14, color: "var(--text-muted)" }}>
            {hasFilter ? t("hist_no_match") : t("history_empty")}
          </p>
        </div>
      ) : (
        <div className="card" style={{ padding: 0, overflowX: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                {mainCols.map(col => (
                  <th
                    key={col.key}
                    style={{ ...thStyle, textAlign: col.align }}
                    onClick={() => handleSort(col.key)}
                  >
                    {col.label()}
                    {sortKey === col.key && (sortDir === "asc" ? " ▲" : " ▼")}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map(row => {
                const isOpen = openRows.has(row.id);
                const metrics = parseMetrics(row.metrics);
                return (
                  <Fragment key={row.id}>
                    <tr
                      onClick={() => toggleRow(row.id)}
                      style={{ cursor: "pointer", transition: "background 0.1s" }}
                      onMouseEnter={e => (e.currentTarget.style.background = "var(--bg-hover, rgba(255,255,255,0.04))")}
                      onMouseLeave={e => (e.currentTarget.style.background = "")}
                    >
                      {/* Date with arrow indicator */}
                      <td style={{ ...tdStyle, textAlign: "left" }}>
                        <span style={{ marginRight: 6, fontSize: 10, color: "var(--text-muted)" }}>
                          {isOpen ? "▾" : "▸"}
                        </span>
                        {new Date(row.ts).toLocaleString()}
                      </td>
                      {/* Module label */}
                      <td style={{ ...tdStyle, textAlign: "left" }}>
                        {moduleLabel(row.module)}
                      </td>
                      {/* Grade */}
                      <td style={{ ...tdStyle, textAlign: "left" }}>
                        {row.summary_grade ? <GradeBadge grade={row.summary_grade} /> : "—"}
                      </td>
                    </tr>

                    {/* Accordion detail row */}
                    {isOpen && (
                      <tr>
                        <td
                          colSpan={3}
                          style={{
                            padding: "10px 20px 14px 32px",
                            background: "rgba(255,255,255,0.02)",
                            borderBottom: "1px solid var(--border)",
                          }}
                        >
                          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 6, textTransform: "uppercase", letterSpacing: "0.08em" }}>
                            {t("hist_detail")} · {moduleLabel(row.module)} · {new Date(row.ts).toLocaleString()}
                          </div>
                          <div style={{
                            display: "grid",
                            gridTemplateColumns: "repeat(auto-fill, minmax(180px, 1fr))",
                            gap: "4px 16px",
                          }}>
                            {Object.entries(metrics).map(([key, val]) => (
                              <div key={key} style={{ display: "flex", gap: 6, alignItems: "baseline" }}>
                                <span style={{ fontSize: 11, color: "var(--text-muted)", whiteSpace: "nowrap" }}>{key}:</span>
                                <span style={{ fontFamily: "var(--mono)", fontSize: 12, color: "var(--text)" }}>
                                  {val === null || val === undefined ? "—" : typeof val === "number" ? num(val) : String(val)}
                                </span>
                              </div>
                            ))}
                            {Object.keys(metrics).length === 0 && (
                              <span style={{ fontSize: 12, color: "var(--text-muted)" }}>—</span>
                            )}
                          </div>
                        </td>
                      </tr>
                    )}
                  </Fragment>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

// ─── Module-specific (unchanged) table render ────────────────────────────────

interface ModuleHistoryProps {
  module: string;
  rows: TestRunRow[];
}

function ModuleHistoryView({ module, rows }: ModuleHistoryProps) {
  useLang();
  const [sortKey, setSortKey] = useState<string>("date");
  const [sortDir, setSortDir] = useState<"asc" | "desc">("desc");

  const cols = COLS[module] ?? COLS["_main"];

  function handleSort(key: string) {
    if (key === sortKey) {
      setSortDir(d => d === "asc" ? "desc" : "asc");
    } else {
      setSortKey(key);
      setSortDir("desc");
    }
  }

  const activeCol = cols.find(c => c.key === sortKey) ?? cols[0];

  const sorted = [...rows].sort((a, b) => {
    const ma = parseMetrics(a.metrics);
    const mb = parseMetrics(b.metrics);
    const va = activeCol.sortValue(a, ma);
    const vb = activeCol.sortValue(b, mb);
    const aIsBottom = va === -Infinity || va === "";
    const bIsBottom = vb === -Infinity || vb === "";
    if (aIsBottom && bIsBottom) return 0;
    if (aIsBottom) return 1;
    if (bIsBottom) return -1;
    const cmp = compareSortVal(va, vb);
    return sortDir === "asc" ? cmp : -cmp;
  });

  const scored = [...rows].filter(r => r.summary_score !== null).reverse();
  const sparkData = scored.map(r => r.summary_score as number);

  const thStyle: CSSProperties = {
    padding: "6px 10px",
    fontSize: 11,
    fontWeight: 600,
    color: "var(--text-muted)",
    textTransform: "uppercase",
    letterSpacing: "0.08em",
    borderBottom: "1px solid var(--border)",
    cursor: "pointer",
    userSelect: "none",
    whiteSpace: "nowrap",
  };

  const tdStyle: CSSProperties = {
    padding: "7px 10px",
    fontSize: 12,
    borderBottom: "1px solid var(--border)",
    color: "var(--text)",
  };

  return (
    <>
      {sparkData.length >= 2 && (
        <div className="card" style={{ padding: "12px 16px" }}>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginBottom: 8, textTransform: "uppercase", letterSpacing: "0.1em" }}>
            {t("history_trend")}
          </div>
          <Sparkline data={sparkData} color="var(--accent)" width={200} height={36} />
        </div>
      )}

      {rows.length === 0 ? (
        <div className="card" style={{ padding: 28, textAlign: "center" }}>
          <p style={{ margin: 0, fontSize: 14, color: "var(--text-muted)" }}>{t("history_empty")}</p>
        </div>
      ) : (
        <div className="card" style={{ padding: 0, overflowX: "auto" }}>
          <table style={{ width: "100%", borderCollapse: "collapse" }}>
            <thead>
              <tr>
                {cols.map(col => (
                  <th
                    key={col.key}
                    style={{ ...thStyle, textAlign: col.align ?? "left" }}
                    onClick={() => handleSort(col.key)}
                  >
                    {col.label()}
                    {sortKey === col.key && (sortDir === "asc" ? " ▲" : " ▼")}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {sorted.map(row => {
                const m = parseMetrics(row.metrics);
                return (
                  <tr
                    key={row.id}
                    style={{ transition: "background 0.1s" }}
                    onMouseEnter={e => (e.currentTarget.style.background = "var(--bg-hover, rgba(255,255,255,0.04))")}
                    onMouseLeave={e => (e.currentTarget.style.background = "")}
                  >
                    {cols.map(col => (
                      <td
                        key={col.key}
                        style={{ ...tdStyle, textAlign: col.align ?? "left" }}
                      >
                        {col.render(row, m)}
                      </td>
                    ))}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </>
  );
}

// ─── Public component ────────────────────────────────────────────────────────

export function HistoryView({ module, title, active = true }: Props) {
  useLang();
  const [rows, setRows]       = useState<TestRunRow[]>([]);
  const [loading, setLoading] = useState(true);

  const isMain = !module;

  useEffect(() => {
    if (active === false) return;
    setLoading(true);
    const p = module
      ? invoke<TestRunRow[]>("list_test_runs_by_module", { module, limit: 100 })
      : invoke<TestRunRow[]>("list_test_runs", { limit: 100 });
    p.then(setRows).catch(() => setRows([])).finally(() => setLoading(false));
  }, [module, active]);

  if (loading) {
    return (
      <div style={{ padding: 24, textAlign: "center", color: "var(--text-muted)", fontSize: 13 }}>
        {t("history_title")}…
      </div>
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
      {title && (
        <div style={{ fontSize: 13, fontWeight: 600, color: "var(--text-dim)", textTransform: "uppercase", letterSpacing: "0.08em" }}>
          {title}
        </div>
      )}

      {isMain
        ? <MainHistoryView rows={rows} />
        : <ModuleHistoryView module={module} rows={rows} />
      }
    </div>
  );
}

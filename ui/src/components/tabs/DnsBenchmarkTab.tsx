import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { DnsBenchmarkResult, DnsProviderResult, DnsProbeResult, DnsBenchmarkProgress, ModuleVerdict } from "../../types";
import { Icon } from "../../lib/icons";
import { TestIntro } from "../TestIntro";
import { GradeCard } from "../GradeCard";
import { t } from "../../lib/i18n";
import { useLang } from "../../lib/useLang";

type IcoComp = typeof Icon[keyof typeof Icon];
import { gradeColor } from "../../lib/helpers";
import { GradeBox } from "../GradeBox";

// ─── Yardımcılar ──────────────────────────────────────────────────────────────

function fmtMs(ms: number, reachable: boolean): string {
  if (!reachable || ms === 0) return "—";
  return `${ms.toFixed(1)} ms`;
}

function ProbeCell({ probe, highlight }: { probe: DnsProbeResult; highlight?: boolean }) {
  if (!probe.reachable) {
    return (
      <div style={{ textAlign: "center" }}>
        <span style={{ fontSize: 12, color: "rgba(255,255,255,0.2)" }}>—</span>
      </div>
    );
  }
  const gc = gradeColor(probe.grade);
  return (
    <div style={{ textAlign: "center" }}>
      <div style={{
        fontFamily: "var(--mono)", fontSize: 13, fontWeight: highlight ? 700 : 500,
        color: highlight ? gc : "var(--text-dim)",
      }}>
        {fmtMs(probe.median_ms, probe.reachable)}
      </div>
      {probe.packet_loss > 0 && (
        <div style={{ fontSize: 10, color: "#ef4444", marginTop: 1 }}>
          {probe.packet_loss.toFixed(0)}% loss
        </div>
      )}
    </div>
  );
}

function GradeBadge({ grade }: { grade: string }) {
  // Ortak standart GradeBox (Smart Diagnosis kare bicimi). N/A -> gri "-" kutusu.
  return <GradeBox grade={grade === "N/A" ? null : grade} size="md" />;
}

// ─── Progress live satırı ────────────────────────────────────────────────────

function LiveProgress({ active }: { active: { provider: string; protocol: string } | null }) {
  useLang();
  if (!active) return null;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, color: "var(--accent)" }}>
      <span style={{ animation: "pulse 1s infinite" }}>●</span>
      <span>{active.provider}</span>
      <span style={{ color: "var(--text-muted)" }}>·</span>
      <span style={{ fontFamily: "var(--mono)", textTransform: "uppercase", fontSize: 11 }}>{active.protocol}</span>
      <span style={{ color: "var(--text-muted)" }}>
        {active.protocol === "udp" ? t("dns_proto_udp_desc") : active.protocol === "dot" ? t("dns_proto_dot_desc") : t("dns_proto_doh_desc")}
      </span>
    </div>
  );
}

// ─── Progress kartı ──────────────────────────────────────────────────────────

function ProgressCard({ done, total, active }: { done: number; total: number; active: { provider: string; protocol: string } | null }) {
  useLang();
  const pct = total > 0 ? Math.round((done / total) * 100) : 0;
  return (
    <div className="card" style={{
      padding: "16px 18px",
      background: "radial-gradient(ellipse at 0% 50%, rgba(99,102,241,0.10), transparent 55%), var(--bg-card-soft)",
      borderColor: "rgba(99,102,241,0.25)",
    }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, marginBottom: 10 }}>
        <div style={{
          width: 32, height: 32, borderRadius: 9,
          display: "grid", placeItems: "center",
          background: "linear-gradient(135deg, var(--accent), var(--accent-2))",
          flexShrink: 0,
        }}>
          <Icon.Settings style={{ width: 16, height: 16, animation: "spin 2s linear infinite" }} />
        </div>
        <div style={{ flex: 1 }}>
          <div style={{ fontWeight: 600, fontSize: 13 }}>{t("dns_running_title")}</div>
          <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
            {done}/{total} {t("dns_probes_done")}
          </div>
        </div>
        <div style={{ fontFamily: "var(--mono)", fontSize: 20, fontWeight: 700, color: "var(--accent)" }}>
          {pct}%
        </div>
      </div>
      <div style={{ height: 4, borderRadius: 999, background: "rgba(255,255,255,0.04)", overflow: "hidden", marginBottom: 10 }}>
        <div style={{
          width: `${pct}%`, height: "100%", borderRadius: 999,
          background: "linear-gradient(90deg, var(--accent), var(--accent-2))",
          transition: "width 400ms ease",
        }} />
      </div>
      <LiveProgress active={active} />
    </div>
  );
}

// ─── Idle kartı ───────────────────────────────────────────────────────────────


// ─── Plain DNS tablosu (:53 / UDP) ───────────────────────────────────────────

type PlainSortKey = "rank" | "udp";
type SortDir = "asc" | "desc";

// Top-level — render içinde tanımlanmamalı (React "Cannot create components during render")
function ColHead({ label, active, dir, onToggle }: {
  label: string; active: boolean; dir: SortDir; onToggle: () => void;
}) {
  return (
    <th onClick={onToggle} style={{
      cursor: "pointer", userSelect: "none", textAlign: "center",
      color: active ? "#c7d2fe" : "var(--text-muted)",
      fontSize: 11, fontWeight: 600, textTransform: "uppercase",
      letterSpacing: "0.05em", padding: "8px 8px",
      borderBottom: "1px solid var(--border)", whiteSpace: "nowrap",
    }}>
      {label} <span style={{ fontSize: 9, opacity: active ? 1 : 0.4 }}>{dir === "asc" ? "▲" : "▼"}</span>
    </th>
  );
}

function PlainTable({ providers }: { providers: DnsProviderResult[] }) {
  useLang();
  const [sort, setSort] = useState<{ key: PlainSortKey; dir: SortDir }>({ key: "rank", dir: "asc" });

  // UDP'ye göre rank hesapla
  const ranked = [...providers]
    .sort((a, b) => {
      const aMs = a.udp.reachable ? a.udp.median_ms : 99999;
      const bMs = b.udp.reachable ? b.udp.median_ms : 99999;
      return aMs - bMs;
    })
    .map((p, i) => ({ ...p, udpRank: i + 1 }));

  const sorted = [...ranked].sort((a, b) => {
    const aMs = a.udp.reachable ? a.udp.median_ms : 99999;
    const bMs = b.udp.reachable ? b.udp.median_ms : 99999;
    const cmp = sort.key === "rank" ? a.udpRank - b.udpRank : aMs - bMs;
    return sort.dir === "asc" ? cmp : -cmp;
  });

  const toggleSort = (key: PlainSortKey) =>
    setSort(prev => prev.key === key ? { key, dir: prev.dir === "asc" ? "desc" : "asc" } : { key, dir: "asc" });

  return (
    <div className="card" style={{ padding: 0, overflow: "hidden", flexShrink: 0, minWidth: 0 }}>
      {/* Başlık */}
      <div style={{
        padding: "11px 16px", borderBottom: "1px solid var(--border)",
        background: "rgba(10,10,15,0.4)",
        display: "flex", alignItems: "center", gap: 8,
      }}>
        <Icon.Circle style={{ width: 14, height: 14, color: "var(--text-muted)" }} />
        <span style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.05em", whiteSpace: "nowrap" }}>
          {t("dns_col_plain")}
        </span>
      </div>
      <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <ColHead label="#" active={sort.key === "rank"} dir={sort.dir} onToggle={() => toggleSort("rank")} />
              <th style={{ textAlign: "left", fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", padding: "8px 10px", color: "var(--text-muted)", borderBottom: "1px solid var(--border)", whiteSpace: "nowrap" }}>
                {t("dns_provider")}
              </th>
              <ColHead label="UDP :53" active={sort.key === "udp"} dir={sort.dir} onToggle={() => toggleSort("udp")} />
              <th style={{ textAlign: "center", fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", padding: "8px 10px", color: "var(--text-muted)", borderBottom: "1px solid var(--border)", whiteSpace: "nowrap" }}>
                {t("dns_grade")}
              </th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((p, i) => {
              const isFirst = i === 0 && sort.key === "rank" && sort.dir === "asc";
              const gc = gradeColor(p.udp.grade);
              return (
                <tr key={p.name} style={{
                  borderBottom: "1px solid rgba(255,255,255,0.04)",
                  background: isFirst ? `${gc}08` : undefined,
                }}>
                  <td style={{ textAlign: "center", padding: "10px 6px", width: 28 }}>
                    {isFirst
                      ? <Icon.Trophy style={{ width: 14, height: 14, color: "#eab308" }} />
                      : <span style={{ fontFamily: "var(--mono)", fontSize: 12, color: "var(--text-muted)" }}>{p.udpRank}</span>}
                  </td>
                  <td style={{ padding: "10px 10px" }}>
                    <div style={{ fontWeight: 600, fontSize: 13 }}>{p.name}</div>
                    <div style={{ fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--mono)" }}>{p.ip}</div>
                  </td>
                  <td style={{ padding: "10px 6px" }}>
                    <ProbeCell probe={p.udp} highlight />
                  </td>
                  <td style={{ textAlign: "center", padding: "10px 10px" }}>
                    <GradeBadge grade={p.udp.reachable ? p.udp.grade : "F"} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
    </div>
  );
}

// ─── Encrypted DNS tablosu (DoT + DoH) ───────────────────────────────────────

type EncSortKey = "rank" | "dot" | "doh" | "best";

function EncryptedTable({ providers }: { providers: DnsProviderResult[] }) {
  useLang();
  const [sort, setSort] = useState<{ key: EncSortKey; dir: SortDir }>({ key: "rank", dir: "asc" });

  const encBest = (p: DnsProviderResult): number => {
    const d = p.dot.reachable ? p.dot.median_ms : 99999;
    const h = p.doh.reachable ? p.doh.median_ms : 99999;
    return Math.min(d, h);
  };

  const ranked = [...providers]
    .sort((a, b) => encBest(a) - encBest(b))
    .map((p, i) => ({ ...p, encRank: i + 1 }));

  const safe = (ms: number, ok: boolean) => ok ? ms : 99999;

  const sorted = [...ranked].sort((a, b) => {
    let cmp = 0;
    switch (sort.key) {
      case "rank": cmp = a.encRank - b.encRank; break;
      case "dot":  cmp = safe(a.dot.median_ms, a.dot.reachable) - safe(b.dot.median_ms, b.dot.reachable); break;
      case "doh":  cmp = safe(a.doh.median_ms, a.doh.reachable) - safe(b.doh.median_ms, b.doh.reachable); break;
      case "best": cmp = encBest(a) - encBest(b); break;
    }
    return sort.dir === "asc" ? cmp : -cmp;
  });

  const toggleSort = (key: EncSortKey) =>
    setSort(prev => prev.key === key ? { key, dir: prev.dir === "asc" ? "desc" : "asc" } : { key, dir: "asc" });

  return (
    <div className="card" style={{ padding: 0, overflow: "hidden", flex: 1, minWidth: 0 }}>
      {/* Başlık */}
      <div style={{
        padding: "11px 16px", borderBottom: "1px solid var(--border)",
        background: "rgba(10,10,15,0.4)",
        display: "flex", alignItems: "center", gap: 8,
      }}>
        <Icon.Lock style={{ width: 14, height: 14, color: "var(--text-muted)" }} />
        <span style={{ fontSize: 12, fontWeight: 700, textTransform: "uppercase", letterSpacing: "0.05em", whiteSpace: "nowrap" }}>
          {t("dns_col_encrypted")}
        </span>
      </div>
      <div style={{ overflowX: "auto" }}>
        <table style={{ width: "100%", borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <ColHead label="#" active={sort.key === "rank"} dir={sort.dir} onToggle={() => toggleSort("rank")} />
              <th style={{ textAlign: "left", fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", padding: "8px 10px", color: "var(--text-muted)", borderBottom: "1px solid var(--border)", whiteSpace: "nowrap" }}>
                {t("dns_provider")}
              </th>
              <ColHead label="DoT :853" active={sort.key === "dot"} dir={sort.dir} onToggle={() => toggleSort("dot")} />
              <ColHead label="DoH HTTPS" active={sort.key === "doh"} dir={sort.dir} onToggle={() => toggleSort("doh")} />
              <ColHead label={t("dns_best")} active={sort.key === "best"} dir={sort.dir} onToggle={() => toggleSort("best")} />
              <th style={{ textAlign: "center", fontSize: 11, fontWeight: 600, textTransform: "uppercase", letterSpacing: "0.05em", padding: "8px 10px", color: "var(--text-muted)", borderBottom: "1px solid var(--border)", whiteSpace: "nowrap" }}>
                {t("dns_grade")}
              </th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((p, i) => {
              const isFirst = i === 0 && sort.key === "rank" && sort.dir === "asc";
              const bestMs  = encBest(p);
              const reachable = p.dot.reachable || p.doh.reachable;
              const bestProto = (p.dot.reachable && (!p.doh.reachable || p.dot.median_ms <= p.doh.median_ms)) ? "DoT" : "DoH";
              const bestGrade = bestProto === "DoT" ? p.dot.grade : p.doh.grade;
              const gc = gradeColor(reachable ? bestGrade : "F");
              return (
                <tr key={p.name} style={{
                  borderBottom: "1px solid rgba(255,255,255,0.04)",
                  background: isFirst ? `${gc}08` : undefined,
                }}>
                  <td style={{ textAlign: "center", padding: "10px 6px", width: 28 }}>
                    {isFirst
                      ? <Icon.Trophy style={{ width: 14, height: 14, color: "#eab308" }} />
                      : <span style={{ fontFamily: "var(--mono)", fontSize: 12, color: "var(--text-muted)" }}>{p.encRank}</span>}
                  </td>
                  <td style={{ padding: "10px 10px" }}>
                    <div style={{ fontWeight: 600, fontSize: 13 }}>{p.name}</div>
                    <div style={{ fontSize: 11, color: "var(--text-muted)", fontFamily: "var(--mono)" }}>{p.ip}</div>
                  </td>
                  <td style={{ padding: "10px 6px" }}>
                    <ProbeCell probe={p.dot} highlight={bestProto === "DoT"} />
                  </td>
                  <td style={{ padding: "10px 6px" }}>
                    <ProbeCell probe={p.doh} highlight={bestProto === "DoH"} />
                  </td>
                  <td style={{ textAlign: "center", padding: "10px 6px" }}>
                    <span style={{ fontFamily: "var(--mono)", fontWeight: 700, fontSize: 13, color: gc }}>
                      {reachable ? `${bestMs.toFixed(1)} ms` : "—"}
                    </span>
                  </td>
                  <td style={{ textAlign: "center", padding: "10px 10px" }}>
                    <GradeBadge grade={reachable ? bestGrade : "F"} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

// ─── Öneri pill'leri ──────────────────────────────────────────────────────────

// Top-level — render içinde tanımlanmamalı
function Pill({ color, icon, label, name, ms, proto }: {
  color: string; icon: IcoComp; label: string;
  name: string; ms: number; proto: string;
}) {
  const Ico = icon;
  return (
    <div style={{
      display: "inline-flex", alignItems: "center", gap: 6,
      padding: "6px 12px", borderRadius: 8,
      background: `${color}12`, border: `1px solid ${color}35`,
      fontSize: 12,
    }}>
      <Ico style={{ width: 14, height: 14, color }} />
      <span style={{ color: "var(--text-muted)" }}>{label}:</span>
      <span style={{ fontWeight: 700, color }}>{name}</span>
      <span style={{ color: "var(--text-muted)" }}>·</span>
      <span style={{ fontFamily: "var(--mono)", fontWeight: 600, color }}>{ms.toFixed(1)} ms</span>
      <span style={{ color: "var(--text-muted)" }}>·</span>
      <span style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--text-muted)" }}>{proto}</span>
    </div>
  );
}

function RecommendRow({ result }: { result: DnsBenchmarkResult }) {
  useLang();

  // En iyi UDP
  const bestPlain = [...result.providers]
    .filter(p => p.udp.reachable)
    .sort((a, b) => a.udp.median_ms - b.udp.median_ms)[0];

  // En iyi encrypted
  const encBest = (p: DnsProviderResult) => {
    const d = p.dot.reachable ? p.dot.median_ms : 99999;
    const h = p.doh.reachable ? p.doh.median_ms : 99999;
    return Math.min(d, h);
  };
  const bestEnc = [...result.providers]
    .filter(p => p.dot.reachable || p.doh.reachable)
    .sort((a, b) => encBest(a) - encBest(b))[0];

  const encProto = bestEnc
    ? (bestEnc.dot.reachable && (!bestEnc.doh.reachable || bestEnc.dot.median_ms <= bestEnc.doh.median_ms) ? "DoT" : "DoH")
    : null;
  const encMs = bestEnc ? encBest(bestEnc) : null;

  return (
    <div style={{ display: "flex", flexWrap: "wrap", gap: 10 }}>
      {bestPlain && (
        <Pill
          color="#6366f1" icon={Icon.Circle}
          label={t("dns_best_plain")}
          name={bestPlain.name}
          ms={bestPlain.udp.median_ms}
          proto="UDP :53"
        />
      )}
      {bestEnc && encProto && encMs !== null && (
        <Pill
          color="#22c55e" icon={Icon.Lock}
          label={t("dns_best_encrypted")}
          name={bestEnc.name}
          ms={encMs}
          proto={encProto}
        />
      )}
    </div>
  );
}

// ─── Ana bileşen ──────────────────────────────────────────────────────────────

export function DnsBenchmarkTab() {
  useLang();
  const [result,    setResult]    = useState<DnsBenchmarkResult | null>(null);
  const [isRunning, setIsRunning] = useState(false);
  const [error,     setError]     = useState<string | null>(null);
  const [active,    setActive]    = useState<{ provider: string; protocol: string } | null>(null);
  const [doneCount, setDoneCount] = useState(0);
  const [verdict,   setVerdict]   = useState<ModuleVerdict | null>(null);

  // 7 provider × 3 protokol = 21 toplam probe
  const TOTAL_PROBES = 21;

  useEffect(() => {
    const unsub = listen<DnsBenchmarkProgress>("dns_benchmark:progress", (ev) => {
      const p = ev.payload;
      if (p.status === "running") {
        setActive({ provider: p.provider, protocol: p.protocol });
      } else {
        setActive(null);
        setDoneCount(prev => prev + 1);
      }
    });
    return () => { void unsub.then(fn => fn()); };
  }, []);

  useEffect(() => {
    if (!result) { setVerdict(null); return; }
    invoke<ModuleVerdict>("run_module_verdict", {
      module: "dns_benchmark",
      input: {
        nic: {}, wifi: {}, ping_results: [], bufferbloat: {},
        speed_download: null, speed_upload: null,
        throttle: null, isp_intervention: null, ipv6: null, cgnat: null, mtu: null, voip: null, wifi_scan: null,
        dns_benchmark: result, twitch: null,
      },
    }).then(setVerdict).catch(() => setVerdict(null));
  }, [result]); // eslint-disable-line react-hooks/exhaustive-deps


  const run = async () => {
    if (isRunning) return;
    setIsRunning(true);
    setResult(null);
    setError(null);
    setDoneCount(0);
    setActive(null);
    try {
      const res = await invoke<DnsBenchmarkResult>("run_dns_benchmark");
      setResult(res);
    } catch (e: unknown) {
      setError(String(e));
    } finally {
      setIsRunning(false);
      setActive(null);
    }
  };

  // Bos durum (henuz sonuc yok, calismyor): ortak pat1 kalibi.
  if (!result && !isRunning) {
    return (
      <TestIntro
        iconKey="Dns"
        title={t("tab_dns_benchmark_title")}
        description={t("dns_intro_desc")}
        buttonLabel={t("dns_btn_run")}
        onRun={run}
        warning={t("test_warning_external_traffic")}
      />
    );
  }

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>

      {/* Header */}
      <div className="card" style={{ padding: "12px 16px", display: "flex", alignItems: "center", gap: 14 }}>
        <div style={{ flex: 1, fontSize: 12, color: "var(--text-muted)", lineHeight: 1.6 }}>
          {t("dns_header_desc")}
        </div>
        <button className="btn" onClick={run} disabled={isRunning} type="button" style={{ flexShrink: 0 }}>
          {isRunning ? t("btn_running") : t("dns_btn_run")}
        </button>
      </div>

      {/* Hata */}
      {error && (
        <div className="card" style={{ padding: "12px 16px", background: "rgba(239,68,68,0.08)", borderColor: "rgba(239,68,68,0.3)", color: "#fca5a5", fontSize: 13 }}>
          <strong>Hata:</strong> {error}
        </div>
      )}

      {/* Progress */}
      {isRunning && (
        <ProgressCard done={doneCount} total={TOTAL_PROBES} active={active} />
      )}

      {/* Sonuç — iki sütun */}
      {result && (
        <>
          {/* Smart Grade */}
          {verdict && verdict.available && (
            <GradeCard
              grade={verdict.grade}
              comment={t(verdict.detail_key as Parameters<typeof t>[0])}
            />
          )}

          {/* Öneri pill'leri */}
          <RecommendRow result={result} />

          {/* İki tablo yan yana */}
          <div style={{ display: "flex", gap: 16, alignItems: "stretch" }}>
            <PlainTable providers={result.providers} />
            <EncryptedTable providers={result.providers} />
          </div>

          {/* Metodoloji */}
          <div className="card" style={{ padding: "10px 16px" }}>
            <div style={{ fontSize: 11, color: "var(--text-muted)", lineHeight: 1.7 }}>
              <strong style={{ color: "var(--text-secondary)" }}>{t("dns_methodology_title")}</strong>
              {" "}{t("dns_methodology_desc")}
            </div>
          </div>
        </>
      )}

    </div>
  );
}

import { useEffect, useState } from "react";

/** 0-100 ölçeğinde değer → CSS rengi (yüksek = iyi). */
export function colorForValue(v: number): string {
  if (v >= 80) return "var(--success)";
  if (v >= 60) return "var(--warning)";
  return "var(--danger)";
}

/** Gerçek zamanlı saat hook'u. */
export function useClock(): Date {
  const [now, setNow] = useState<Date>(() => new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);
  return now;
}

export function formatTime(d: Date): string {
  const hh = String(d.getHours()).padStart(2, "0");
  const mm = String(d.getMinutes()).padStart(2, "0");
  return `${hh}:${mm}`;
}

export function formatDate(d: Date): string {
  return d.toLocaleDateString("en-US", { month: "short", day: "numeric" });
}

// 6 kademe harf notu renkleri: A+ A B C D F (akademik standart, E/S yok)
const GRADE_COLORS: Record<string, string> = {
  "A+": "#34d399", // parlak yesil (en ust)
  A: "#86efac",    // yesil
  B: "#93c5fd",    // mavi
  C: "#fde047",    // sari
  D: "#fdba74",    // turuncu
  F: "#fca5a5",    // kirmizi
};

export function gradeColor(grade: string): string {
  return GRADE_COLORS[grade] ?? "#9ca3af";
}

/**
 * 0-100 skor → harf notu (A+/A/B/C/D/F). Backend smart.rs::score_to_grade ile BİREBİR aynı
 * eşikler — Smart Engine'in puanladığı modüller (IPv6 quality.score gibi) için tutarlılık.
 */
export function gradeFromScore(score: number): string {
  if (score >= 95) return "A+";
  if (score >= 85) return "A";
  if (score >= 75) return "B";
  if (score >= 65) return "C";
  if (score >= 50) return "D";
  return "F";
}

/** ms cinsinden latency için heuristic grade (A+/A/B/C/D/F). */
export function gradeFromPing(avgMs: number): string {
  if (avgMs < 25) return "A+";
  if (avgMs < 40) return "A";
  if (avgMs < 60) return "B";
  if (avgMs < 90) return "C";
  if (avgMs < 130) return "D";
  return "F";
}

/** Saniye/ago format (örn. "18s ago"). */
export function formatAgo(date: Date | null, now: Date = new Date()): string {
  if (!date) return "—";
  const diff = Math.max(0, Math.floor((now.getTime() - date.getTime()) / 1000));
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  return `${Math.floor(diff / 3600)}h ago`;
}

// ─── Game Server Grade (ITU-T Y.1541 + gaming engineering) ──────────────────

const GAME_GRADE_RANK: Record<string, number> = {
  "A+": 0, "A": 1, "B": 2, "C": 3, "D": 4, "F": 5,
};

/** Daha kötü olanı (higher rank = worse) seçer. */
function minGrade(a: string, b: string): string {
  return (GAME_GRADE_RANK[a] ?? 5) >= (GAME_GRADE_RANK[b] ?? 5) ? a : b;
}

function rttGrade(rtt: number): string {
  if (rtt <= 15) return "A+";
  if (rtt <= 35) return "A";
  if (rtt <= 60) return "B";
  if (rtt <= 100) return "C";
  if (rtt <= 150) return "D";
  return "F";
}

function jitterGrade(j: number): string {
  if (j < 3) return "A+";
  if (j < 8) return "A";
  if (j < 15) return "B";
  if (j < 30) return "C";
  if (j < 50) return "D";
  return "F";
}

function lossCapGrade(loss: number): string {
  if (loss <= 0.1) return "A+";
  if (loss <= 0.5) return "A";
  if (loss <= 1.0) return "B";
  if (loss <= 2.0) return "C";
  if (loss <= 5.0) return "D";
  return "F";
}

/**
 * Games-only loss-capped grade (ITU-T Y.1541 + gaming).
 * lossEnabled=false → loss cap devre dışı, grade sadece rtt+jitter.
 * Yalnızca reachable satırlara uygula; unreachable için r.grade kullan.
 */
export function gameGrade(
  rtt: number,
  jitter: number,
  loss: number,
  lossEnabled: boolean,
): string {
  const base = minGrade(rttGrade(rtt), jitterGrade(jitter));
  return lossEnabled ? minGrade(lossCapGrade(loss), base) : base;
}

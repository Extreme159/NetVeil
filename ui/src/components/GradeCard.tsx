import type { ReactNode } from "react";
import { gradeColor } from "../lib/helpers";
import { t } from "../lib/i18n";

/**
 * Ortak grade özet kartı — SOL büyük grade damgası + SAĞ DURUM kelimesi + yorum.
 * CGNAT verdict kartı düzeni referans. Tüm grade'li modüller bu bileşeni kullanır.
 *
 * title verilmezse grade harfine karşılık gelen DURUM kelimesi (Mükemmel/İyi/...) gösterilir
 * — modül adını tekrar etmek yerine (zaten o modülün sayfasındayız) durumu belirtir.
 */
function gradeStatusKey(grade: string): string {
  const key = grade === "A+" ? "Aplus" : grade;
  return `grade_status_${key}`;
}

export function GradeCard({ grade, title, comment }: {
  grade: string;       // "A+".."F"
  title?: string;      // verilmezse grade durumu (Mükemmel/İyi/...) gösterilir
  comment: ReactNode;
}) {
  const color = gradeColor(grade);
  const heading = title ?? t(gradeStatusKey(grade) as Parameters<typeof t>[0]);
  return (
    <div className="card" style={{ display: "flex", alignItems: "center", gap: 18, borderColor: `${color}55` }}>
      <div style={{
        flexShrink: 0,
        width: 64,
        height: 64,
        borderRadius: 16,
        display: "grid",
        placeItems: "center",
        color,
        background: `${color}1a`,
        border: `1.5px solid ${color}55`,
        fontFamily: "var(--mono)",
        fontWeight: 800,
        fontSize: 30,
      }}>
        {grade}
      </div>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 17, fontWeight: 700, color }}>{heading}</div>
        <p style={{ margin: "6px 0 0", fontSize: 12.5, color: "var(--text-dim)", lineHeight: 1.55 }}>{comment}</p>
      </div>
    </div>
  );
}

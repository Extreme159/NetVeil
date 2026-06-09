import type { CSSProperties } from "react";
import { gradeColor } from "../lib/helpers";

/**
 * Ortak grade (harf notu) rozeti — TÜM uygulamada tek standart biçim.
 * Kaynak referans: Smart Diagnosis'in kare/rounded grade kutusu.
 *
 * Biçim sabittir (kare-rounded, renk = gradeColor'dan inline: dolgu/kenarlık/yazı),
 * yalnızca BOYUT `size` ile ayarlanır (sm/md/lg) ya da tek tek override edilebilir.
 * Renk inline geldiği için A+ dahil tüm notlar doğru renklenir (eski grade-badge CSS
 * class'ında A+ selector'ı çalışmıyordu — bu sorun burada kökten çözülür).
 *
 * Boş/ölçülemeyen durum: grade verilmezse gri "–" kutusu (Smart'taki "available değil" kalıbı).
 */
type GradeSize = "sm" | "md" | "lg";

interface Props {
  /** Harf notu: A+/A/B/C/D/F. null/undefined → gri "–" (ölçülemedi). */
  grade?: string | null;
  /** Hazır boyut. Varsayılan "md" (Smart'taki standart: 34×24, font 13). */
  size?: GradeSize;
  /** true → vurgu efekti: dış glow (box-shadow) + daha kalın kenarlık. Ana/hero
   * grade gösterimleri için (Bufferbloat/IPv6 gibi). Liste grade'lerinde kapalı (varsayılan). */
  glow?: boolean;
  /** Ek/override stil (ör. tablo hücresinde hizalama, özel boyut). */
  style?: CSSProperties;
}

interface SizeSpec {
  minWidth: number;
  height: number;
  padding: string;
  borderRadius: number;
  fontSize: number;
}

// Yumuşak köşeli (rounded) + kalın (2px) çerçeve — tek standart biçim.
// sm == md: ikisi de Smart Diagnosis MODÜL SATIRI biçimi (liste/tablo grade'leri).
// lg: Smart Diagnosis SENTEZ KARTI biçimi (büyük/hero grade, genelde glow ile).
const SIZES: Record<GradeSize, SizeSpec> = {
  sm: { minWidth: 34, height: 24, padding: "0 8px", borderRadius: 7, fontSize: 13 }, // Smart modul satiri
  md: { minWidth: 34, height: 24, padding: "0 8px", borderRadius: 7, fontSize: 13 }, // Smart modul satiri (== sm)
  lg: { minWidth: 88, height: 88, padding: "0 12px", borderRadius: 18, fontSize: 40 }, // Smart sentez karti
};

export function GradeBox({ grade, size = "md", glow = false, style }: Props) {
  const spec = SIZES[size];
  const base: CSSProperties = {
    flexShrink: 0,
    minWidth: spec.minWidth,
    height: spec.height,
    padding: spec.padding,
    borderRadius: spec.borderRadius,
    display: "grid",
    placeItems: "center",
    fontFamily: "var(--mono)",
    fontSize: spec.fontSize,
    fontWeight: 700,
    lineHeight: 1,
  };

  // Ölçülemeyen / boş durum: gri "–"
  if (!grade) {
    return (
      <span
        style={{
          ...base,
          color: "var(--text-muted)",
          background: "rgba(255,255,255,0.04)",
          border: "2px solid var(--border)",
          ...style,
        }}
      >
        –
      </span>
    );
  }

  const color = gradeColor(grade);
  return (
    <span
      style={{
        ...base,
        color,
        // Glow modunda gradient dolgu + dış parıltı; normalde düz ~10% dolgu (Smart ile aynı).
        background: glow
          ? `radial-gradient(ellipse at 30% 20%, ${color}33, ${color}14)`
          : `${color}1a`,
        border: glow ? `3px solid ${color}80` : `2px solid ${color}66`,
        boxShadow: glow ? `0 0 22px ${color}40` : undefined,
        ...style,
      }}
    >
      {grade}
    </span>
  );
}

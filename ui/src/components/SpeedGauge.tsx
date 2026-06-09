import { useId } from "react";

interface Props {
  value: number;
  /** Adaptif max — gauge ölçeği bu değere göre ayarlanır */
  maxValue?: number;
  /** "cw" = saat yönü (saat 9 → saat 6), "ccw" = saatin tersi (saat 3 → saat 6) */
  direction?: "cw" | "ccw";
  label?: string;
}

/** Adaptif max'e göre tick değerlerini üret */
function tickValues(max: number): number[] {
  if (max <= 100) return [0, 20, 40, 60, 80, 100];
  if (max <= 500) return [0, 50, 100, 200, 350, 500];
  return [0, 100, 250, 500, 750, 1000];
}

/** Hibrit logaritmik ölçek: pow(0.65) — 0.6'dan biraz daha doğrusal */
function toPct(v: number, max: number) {
  return Math.pow(Math.max(0, Math.min(1, v / max)), 0.65);
}

/**
 * CW  gauge: startA = -90° (saat 9),  endA = 180° (saat 6), saat yönü
 * CCW gauge: startA =  90° (saat 3),  endA = 180° (saat 6), saatin tersi
 * Her ikisi de 270° arc.
 */
export function SpeedGauge({ value, maxValue = 1000, direction = "cw", label }: Props) {
  // Benzersiz ID prefix — aynı sayfada birden fazla SpeedGauge (SpeedTest + LAN, TabPane hep-mount)
  // gradient/filter ID çakışmasını önler (SVG duplicate-ID dolgu bug'ı).
  const uid = useId().replace(/:/g, "");
  const cx = 130;
  const cy = 134;
  const r  = 91;

  const isCW   = direction === "cw";
  const startA = isCW ? -90 : 90;
  const span   = isCW ? 270 : -270;

  const TICKS  = tickValues(maxValue);
  const pct    = toPct(value, maxValue);
  const valueA = startA + span * pct;

  const polar = (deg: number, rr = r) => ({
    x: cx + rr * Math.sin((deg * Math.PI) / 180),
    y: cy - rr * Math.cos((deg * Math.PI) / 180),
  });

  const s = polar(startA);
  const endA = startA + span;  // her ikisi saat 6'da biter (180°)
  const e = polar(endA);
  const v = polar(valueA);

  // sweep açısı
  const sweepDeg = Math.abs(valueA - startA);
  const showValue = sweepDeg > 2;

  // CW: sweep=1 (saat yönü), CCW: sweep=0 (saatin tersi)
  const sweepFlag = isCW ? 1 : 0;
  // track: her zaman büyük arc (270°)
  const trackPath = `M ${s.x} ${s.y} A ${r} ${r} 0 1 ${sweepFlag} ${e.x} ${e.y}`;
  // value arc
  const largeArc  = sweepDeg > 180 ? 1 : 0;
  const valuePath = `M ${s.x} ${s.y} A ${r} ${r} 0 ${largeArc} ${sweepFlag} ${v.x} ${v.y}`;

  // Tick'ler — adaptif, uç noktalar için textAnchor ayarı
  const ticks = TICKS.map((val, i) => {
    const p   = toPct(val, maxValue);
    const a   = startA + span * p;
    const p1  = polar(a, r + 6);
    const p2  = polar(a, r + 18);
    const isFirst = i === 0;
    const lbl = polar(a, r + 32);
    const anchor: "start" | "middle" | "end" = isFirst
      ? (isCW ? "end" : "start")
      : "middle";
    return { p1, p2, lbl, val, anchor };
  });

  // İbre açısı
  const needleA = valueA;

  // Hız değeri konumu: CW için saat 7:30 (sol-alt iç), CCW için saat 4:30 (sağ-alt iç)
  const txtX = isCW ? cx - r * 0.5 : cx + r * 0.5;
  const txtY = cy + r * 0.45;

  return (
    <svg className="gauge-svg" viewBox="0 0 260 252" xmlns="http://www.w3.org/2000/svg">
      <defs>
        <linearGradient id={`gaugeGrad-${uid}`} x1={isCW ? "0" : "1"} y1="0" x2={isCW ? "1" : "0"} y2="0">
          <stop offset="0%"   stopColor="#ef4444" />
          <stop offset="18%"  stopColor="#f97316" />
          <stop offset="44%"  stopColor="#eab308" />
          <stop offset="55%"  stopColor="#84cc16" />
          <stop offset="100%" stopColor="#22c55e" />
        </linearGradient>
        <filter id={`glow-${uid}`}>
          <feGaussianBlur stdDeviation="4" result="b" />
          <feMerge>
            <feMergeNode in="b" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
        <radialGradient id={`hubGrad-${uid}`} cx="0.5" cy="0.5" r="0.5">
          <stop offset="0%"   stopColor="#a5b4fc" />
          <stop offset="100%" stopColor="#4f46e5" />
        </radialGradient>
      </defs>

      {/* Track */}
      <path d={trackPath} fill="none" stroke="#1a1a2e" strokeWidth="13" strokeLinecap="butt" />
      <path d={trackPath} fill="none" stroke={`url(#gaugeGrad-${uid})`} strokeOpacity="0.08" strokeWidth="13" strokeLinecap="butt" />

      {/* Değer yayı — track'ten ince, butt linecap — dışara taşmaz */}
      {showValue && (
        <path
          d={valuePath}
          fill="none"
          stroke={`url(#gaugeGrad-${uid})`}
          strokeWidth="10"
          strokeLinecap="butt"
          filter={`url(#glow-${uid})`}
          style={{ transition: "all 0.5s cubic-bezier(0.22,1,0.36,1)" }}
        />
      )}

      {/* Tick'ler */}
      {ticks.map((t, i) => (
        <g key={i}>
          <line
            x1={t.p1.x} y1={t.p1.y}
            x2={t.p2.x} y2={t.p2.y}
            stroke="#4b5563" strokeWidth="1.5" strokeLinecap="round"
          />
          <text
            x={t.lbl.x} y={t.lbl.y + 3}
            textAnchor={t.anchor} fontSize="8.5"
            fontFamily="JetBrains Mono, ui-monospace, monospace"
            fill="#6b7280"
          >
            {t.val}
          </text>
        </g>
      ))}

      {/* İğne */}
      <g style={{
        transform: `rotate(${needleA}deg)`,
        transformOrigin: `${cx}px ${cy}px`,
        transition: "transform 0.5s cubic-bezier(0.22,1,0.36,1)",
      }}>
        <line x1={cx} y1={cy} x2={cx} y2={cy - r + 20} stroke="#c7d2fe" strokeWidth="2" strokeLinecap="round" />
        <circle cx={cx} cy={cy - r + 20} r="3.5" fill="#a5b4fc" filter={`url(#glow-${uid})`} />
      </g>

      {/* Hub */}
      <circle cx={cx} cy={cy} r="12" fill={`url(#hubGrad-${uid})`} />
      <circle cx={cx} cy={cy} r="12" fill="none" stroke="rgba(255,255,255,0.2)" />
      <circle cx={cx} cy={cy} r="4"  fill="#0a0a0f" />

      {/* Hız değeri — boş 90°'lik iç alana */}
      <text
        x={txtX} y={txtY}
        textAnchor="middle" fontSize="34" fontWeight="700"
        fontFamily="JetBrains Mono, ui-monospace, monospace"
        fill="#e0e7ff"
        style={{ filter: "drop-shadow(0 0 16px rgba(99,102,241,0.5))" }}
      >
        {value.toFixed(1)}
      </text>
      <text
        x={txtX} y={txtY + 16}
        textAnchor="middle" fontSize="9"
        fontFamily="JetBrains Mono, ui-monospace, monospace"
        fill="#6b7280" letterSpacing="0.1em"
      >
        {label ?? "MBPS"}
      </text>
    </svg>
  );
}

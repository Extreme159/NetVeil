interface SparklineProps {
  data: number[];
  color?: string;
  width?: number;
  height?: number;
}

/**
 * Küçük sparkline çizgisi — son noktada bir dot bırakır.
 */
export function Sparkline({ data, color = "currentColor", width = 70, height = 28 }: SparklineProps) {
  if (data.length < 2) return null;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const span = Math.max(0.001, max - min);
  const stepX = width / (data.length - 1);
  const pts = data
    .map((v, i) => `${(i * stepX).toFixed(1)},${(height - ((v - min) / span) * height).toFixed(1)}`)
    .join(" ");
  const last = data[data.length - 1];
  const lastY = (height - ((last - min) / span) * height).toFixed(1);
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`}>
      <polyline
        points={pts}
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinejoin="round"
        strokeLinecap="round"
      />
      <circle cx={width.toFixed(1)} cy={lastY} r="2" fill={color} />
    </svg>
  );
}

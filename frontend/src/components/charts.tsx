import { useMemo, useState } from 'react';
import type React from 'react';

function join(...classes: Array<string | undefined>) {
  return classes.filter(Boolean).join(' ');
}

function niceCeil(value: number): number {
  if (value <= 0) return 1;
  const exp = Math.floor(Math.log10(value));
  const base = Math.pow(10, exp);
  const scaled = value / base;
  const factor = scaled <= 1 ? 1 : scaled <= 2 ? 2 : scaled <= 5 ? 5 : 10;
  return factor * base;
}

function shortNum(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 10_000 ? 0 : 1)}K`;
  return String(Math.round(n * 100) / 100);
}

const W = 600;
const H = 220;
const PAD = { top: 10, right: 12, bottom: 26, left: 44 };

const X0 = PAD.left;
const X1 = W - PAD.right;
const Y0 = H - PAD.bottom;
const Y1 = PAD.top;

function xFor(i: number, count: number): number {
  if (count <= 1) return (X0 + X1) / 2;
  return X0 + (i / (count - 1)) * (X1 - X0);
}

function yFor(v: number, max: number): number {
  return Y0 - (v / max) * (Y0 - Y1);
}

export interface Series {
  key: string;
  label: string;
  color: string;
}

/** Monotone-ish smooth area chart with hover tooltip and optional bar overlay series. */
export function AreaChart({ data, series, barSeries, height = 220, className }: {
  data: Array<Record<string, number | string>>;
  series: Series[];
  /** Optional series rendered as bars (e.g. latency alongside requests). */
  barSeries?: Series;
  height?: number;
  className?: string;
}) {
  const [hover, setHover] = useState<number | null>(null);

  const { paths, areaPaths, max, bars } = useMemo(() => {
    const seriesValues = data.flatMap((d) => series.map((s) => Number(d[s.key]) || 0));
    const barValues = barSeries ? data.map((d) => Number(d[barSeries.key]) || 0) : [];
    const maxVal = Math.max(1, ...seriesValues, ...barValues);
    const niceMax = niceCeil(maxVal);

    const lines = series.map((s) => {
      const points = data.map((d, i) => [xFor(i, data.length), yFor(Number(d[s.key]) || 0, niceMax)] as const);
      const path = points.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
      const area = `${path} L${points.length ? points[points.length - 1][0].toFixed(1) : X0},${Y0} L${points.length ? points[0][0].toFixed(1) : X0},${Y0} Z`;
      return { path, area };
    });

    const barWidth = barSeries && data.length > 0
      ? Math.min(24, Math.max(2, (X1 - X0) / data.length / 2))
      : 0;
    const barRects = barSeries
      ? data.map((d, i) => ({
          x: xFor(i, data.length) - barWidth / 2,
          y: yFor(Number(d[barSeries.key]) || 0, niceMax),
          h: Y0 - yFor(Number(d[barSeries.key]) || 0, niceMax),
          w: barWidth,
        }))
      : [];

    return { paths: lines.map((l) => l.path), areaPaths: lines.map((l) => l.area), max: niceMax, bars: barRects };
  }, [data, series, barSeries]);

  const yTicks = useMemo(() => [0, 0.25, 0.5, 0.75, 1].map((f) => f * max), [max]);
  const labelEvery = Math.max(1, Math.ceil(data.length / 8));

  return (
    <div className={join('relative', className)} style={{ height }}>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="h-full w-full select-none"
        preserveAspectRatio="none"
        onMouseLeave={() => setHover(null)}
        onMouseMove={(e) => {
          const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
          const relX = ((e.clientX - rect.left) / rect.width) * W;
          const idx = Math.round(((relX - X0) / (X1 - X0)) * (data.length - 1));
          setHover(idx >= 0 && idx < data.length ? idx : null);
        }}
      >
        <defs>
          {series.map((s, i) => (
            <linearGradient key={s.key} id={`grad-${i}`} x1="0" y1="0" x2="0" y2="1">
              <stop offset="0%" stopColor={s.color} stopOpacity={0.18} />
              <stop offset="100%" stopColor={s.color} stopOpacity={0} />
            </linearGradient>
          ))}
        </defs>

        {yTicks.map((tick) => (
          <g key={tick}>
            <line x1={X0} y1={yFor(tick, max)} x2={X1} y2={yFor(tick, max)} stroke="#d4d4d0" strokeDasharray="3 3" strokeWidth={1} />
            <text x={X0 - 6} y={yFor(tick, max) + 3.5} textAnchor="end" fontSize={9} fill="#8e877d">{shortNum(tick)}</text>
          </g>
        ))}

        {bars.map((bar, i) => (
          <rect key={i} x={bar.x} y={bar.y} width={bar.w} height={bar.h} rx={1.5} fill={barSeries?.color} opacity={0.55} />
        ))}

        {areaPaths.map((area, i) => (
          <path key={`a${i}`} d={area} fill={`url(#grad-${i})`} />
        ))}
        {paths.map((path, i) => (
          <path key={`l${i}`} d={path} fill="none" stroke={series[i].color} strokeWidth={2} strokeLinejoin="round" strokeLinecap="round" />
        ))}

        {data.map((_, i) => (
          i % labelEvery === 0
            ? <text key={`x${i}`} x={xFor(i, data.length)} y={Y0 + 15} textAnchor="middle" fontSize={9} fill="#8e877d">{String(data[i][series[0]?.key ?? 'label'] && typeof data[i].label === 'string' ? data[i].label : i)}</text>
            : null
        ))}

        {hover !== null && data[hover] && (
          <line x1={xFor(hover, data.length)} y1={Y1} x2={xFor(hover, data.length)} y2={Y0} stroke="#0a0a0a" strokeWidth={1} opacity={0.3} />
        )}
      </svg>

      {hover !== null && data[hover] && (
        <div
          className="pointer-events-none absolute z-10 rounded-lg border border-surface-200 bg-white px-3 py-2 text-xs shadow-[0_4px_16px_rgba(41,39,36,0.12)]"
          style={{
            left: `${Math.min(85, Math.max(0, ((xFor(hover, data.length)) / W) * 100))}%`,
            top: 4,
            transform: 'translateX(-50%)',
          }}
        >
          {typeof data[hover].label === 'string' && <div className="mb-1 font-operational text-[10px] uppercase tracking-wider text-surface-400">{data[hover].label}</div>}
          {barSeries && <div className="flex items-center gap-1.5 text-surface-600"><span className="h-2 w-2 rounded-sm" style={{ background: barSeries.color }} />{barSeries.label}: {Number(data[hover][barSeries.key]).toLocaleString()}</div>}
          {series.map((s) => (
            <div key={s.key} className="flex items-center gap-1.5 text-surface-600">
              <span className="h-2 w-2 rounded-full" style={{ background: s.color }} />
              {s.label}: {Number(data[hover][s.key]).toLocaleString()}
            </div>
          ))}
        </div>
      )}

      <div className="mt-1 flex gap-4 px-2 text-xs text-surface-400">
        {barSeries && <div className="flex items-center gap-1.5"><span className="h-3 w-3 rounded-sm" style={{ background: barSeries.color }} />{barSeries.label}</div>}
        {series.map((s) => (
          <div key={s.key} className="flex items-center gap-1.5"><span className="h-0.5 w-3 rounded" style={{ background: s.color }} />{s.label}</div>
        ))}
      </div>
    </div>
  );
}

/** Stacked horizontal bar (token composition / composition charts). */
export function StackBar({ segments, className }: {
  segments: Array<{ label: string; value: number; color: string }>;
  className?: string;
}) {
  const total = segments.reduce((s, seg) => s + seg.value, 0);
  if (total <= 0) return <div className={join('h-2.5 rounded-full bg-surface-100', className)} />;
  return (
    <div className={className}>
      <div className="flex h-2.5 w-full overflow-hidden rounded-full bg-surface-100">
        {segments.filter((s) => s.value > 0).map((seg) => (
          <div key={seg.label} title={`${seg.label}: ${seg.value.toLocaleString()}`} style={{ width: `${(seg.value / total) * 100}%`, background: seg.color }} />
        ))}
      </div>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-surface-500">
        {segments.filter((s) => s.value > 0).map((seg) => (
          <span key={seg.label} className="flex items-center gap-1.5">
            <span className="h-2 w-2 rounded-sm" style={{ background: seg.color }} />
            {seg.label} {Math.round((seg.value / total) * 100)}%
          </span>
        ))}
      </div>
    </div>
  );
}

/** Simple horizontal ranked bars (e.g. requests per model). */
export function RankBars({ rows, color = '#0a0a0a', formatValue = shortNum, className }: {
  rows: Array<{ label: string; value: number }>;
  color?: string;
  formatValue?: (n: number) => string;
  className?: string;
}) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  return (
    <div className={join('space-y-2.5', className)}>
      {rows.map((row) => (
        <div key={row.label}>
          <div className="mb-1 flex justify-between text-xs">
            <span className="truncate text-surface-600">{row.label}</span>
            <span className="font-operational tabular-nums text-surface-400">{formatValue(row.value)}</span>
          </div>
          <div className="h-2 overflow-hidden rounded-full bg-surface-100">
            <div className="h-full rounded-full transition-[width] duration-500" style={{ width: `${(row.value / max) * 100}%`, background: color }} />
          </div>
        </div>
      ))}
    </div>
  );
}

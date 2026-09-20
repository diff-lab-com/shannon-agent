// BarChart — a lightweight SVG bar chart.
//
// No chart-library dependency (we deliberately avoid pulling recharts for
// one chart). Keeps the surface area small: hoverable bars surface the
// raw numbers as a tooltip; a per-row legend explains the stack. The intent
// is "glance and act", not pixel-perfect dashboarding — the audit (table)
// view sits next to it for precise reconciliation.

import { useMemo, useState } from 'react'
import { cn } from '@/lib/utils'

export interface BarSeriesPoint {
  label: string
  /** Each series is rendered as a stacked segment (or a single bar when
   *  only one series is provided). */
  series: { key: string; value: number; color?: string }[]
}

export interface BarSeriesDef {
  key: string
  label: string
  /** Tailwind text-color class (e.g. 'text-primary'); the bar itself uses
   *  a matching fill via Tailwind's safelist (see className below). */
  colorClass: string
}

interface BarChartProps {
  data: BarSeriesPoint[]
  series: BarSeriesDef[]
  height?: number
  /** Optional Y-axis formatter (defaults to compact tokens). */
  formatValue?: (n: number) => string
}

const FALLBACK_COLORS = ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-warning']

export function BarChart({ data, series, height = 220, formatValue }: BarChartProps) {
  const [hover, setHover] = useState<{ idx: number; x: number; y: number } | null>(null)
  const fmt = formatValue ?? ((n: number) => n.toLocaleString())

  const { max, totals } = useMemo(() => {
    let m = 0
    const t: number[] = []
    for (const p of data) {
      let sum = 0
      for (const s of p.series) {
        sum += s.value
        m = Math.max(m, s.value)
      }
      t.push(sum)
    }
    return { max: m || 1, totals: t }
  }, [data])

  // SVG geometry — keep viewBox-driven so the chart scales with container.
  const VB_W = 1000
  const VB_H = height
  const padX = 32
  const padTop = 24
  const padBottom = 56
  const chartH = VB_H - padTop - padBottom
  const colCount = Math.max(data.length, 1)
  const groupW = (VB_W - padX * 2) / colCount
  const colW = Math.min(36, groupW * 0.7)

  if (data.length === 0) {
    return null
  }

  return (
    <div className="relative w-full">
      <svg
        viewBox={`0 0 ${VB_W} ${VB_H}`}
        preserveAspectRatio="none"
        className="w-full"
        style={{ height }}
        role="img"
      >
        {/* Y-axis grid lines + tick labels (0%, 25%, 50%, 75%, 100%) */}
        {[0, 0.25, 0.5, 0.75, 1].map(t => (
          <g key={t}>
            <line
              x1={padX}
              x2={VB_W - padX}
              y1={padTop + chartH * (1 - t)}
              y2={padTop + chartH * (1 - t)}
              stroke="currentColor"
              strokeOpacity={t === 0 ? 0.15 : 0.08}
              strokeDasharray={t === 0 ? undefined : '2 4'}
            />
            <text
              x={padX - 6}
              y={padTop + chartH * (1 - t) + 4}
              textAnchor="end"
              fontSize="11"
              fill="currentColor"
              fillOpacity="0.6"
              className="font-mono"
            >
              {fmt(max * t)}
            </text>
          </g>
        ))}

        {data.map((point, idx) => {
          const groupX = padX + groupW * idx + (groupW - colW) / 2
          let y = padTop + chartH
          // Adaptive X-axis density: when there are too many columns to
          // fit legible labels, drop every other one — otherwise 30 / 90
          // day views crowd into a single illegible row.
          const labelEvery = data.length > 21 ? 5 : data.length > 10 ? 3 : 1
          const showLabel = idx % labelEvery === 0 || idx === data.length - 1
          return (
            <g key={point.label + idx}>
              {point.series.map((seg, sIdx) => {
                const segH = (seg.value / max) * chartH
                y -= segH
                const colorClass =
                  seg.color ??
                  FALLBACK_COLORS[sIdx % FALLBACK_COLORS.length] ??
                  FALLBACK_COLORS[0]
                return (
                  <rect
                    key={seg.key + sIdx}
                    x={groupX}
                    y={y}
                    width={colW}
                    height={Math.max(segH, 0)}
                    className={cn(colorClass, 'opacity-90')}
                    rx={2}
                    onMouseEnter={(e) => {
                      const rect = (e.currentTarget.ownerSVGElement as SVGSVGElement).getBoundingClientRect()
                      setHover({ idx, x: e.clientX - rect.left, y: e.clientY - rect.top })
                    }}
                    onMouseLeave={() => setHover(null)}
                  />
                )
              })}
              {/* X-axis label */}
              {showLabel && (
                <text
                  x={groupX + colW / 2}
                  y={VB_H - padBottom + 18}
                  textAnchor="middle"
                  fontSize="11"
                  fill="currentColor"
                  fillOpacity="0.6"
                  className="font-mono"
                >
                  {point.label}
                </text>
              )}
            </g>
          )
        })}
      </svg>

      {/* Legend */}
      <div className="flex items-center gap-md mt-xs text-label-xs text-on-surface-variant flex-wrap">
        {series.map((s, i) => (
          <span key={s.key} className="inline-flex items-center gap-xs">
            <span className={cn('inline-block w-3 h-3 rounded-sm', FALLBACK_COLORS[i % FALLBACK_COLORS.length])} />
            {s.label}
          </span>
        ))}
      </div>

      {/* Hover tooltip — pinned at the hovered column. */}
      {hover !== null && data[hover.idx] && (
        <div
          className="pointer-events-none absolute z-floating px-sm py-xs rounded-md bg-surface-container-highest shadow-lg border border-outline-variant/30 text-label-xs text-on-surface min-w-[160px]"
          style={{
            left: `calc(${(hover.idx + 0.5) / colCount * 100}% )`,
            top: 0,
            transform: 'translateX(-50%)',
          }}
        >
          <div className="font-bold mb-0.5 truncate max-w-[200px]">{data[hover.idx].label}</div>
          {data[hover.idx].series.map((s, i) => (
            <div key={s.key} className="flex items-center gap-xs">
              <span className={cn('inline-block w-2 h-2 rounded-sm', FALLBACK_COLORS[i % FALLBACK_COLORS.length])} />
              <span className="flex-1">{series[i]?.label ?? s.key}</span>
              <span className="font-mono tabular-nums">{fmt(s.value)}</span>
            </div>
          ))}
          <div className="border-t border-outline-variant/20 mt-0.5 pt-0.5 flex items-center justify-between">
            <span className="font-label-xs uppercase tracking-wider text-on-surface-variant">Total</span>
            <span className="font-mono font-bold tabular-nums">{fmt(totals[hover.idx])}</span>
          </div>
        </div>
      )}
    </div>
  )
}

/** Donut — token share across categories (e.g. by model). */
export function DonutChart({
  segments,
  total,
  centerLabel,
  size = 160,
}: {
  segments: { key: string; label: string; value: number; colorClass?: string }[]
  total: number
  centerLabel?: string
  size?: number
}) {
  const safeTotal = total || 1
  const radius = size / 2 - 6
  const cx = size / 2
  const cy = size / 2
  const C = 2 * Math.PI * radius
  let offset = 0
  const defaultColors = ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-warning', 'bg-error']
  return (
    <svg viewBox={`0 0 ${size} ${size}`} width={size} height={size} role="img">
      <circle cx={cx} cy={cy} r={radius} fill="none" className="stroke-surface-container" strokeWidth={14} />
      {segments.map((s, i) => {
        const len = (s.value / safeTotal) * C
        const el = (
          <circle
            key={s.key}
            cx={cx}
            cy={cy}
            r={radius}
            fill="none"
            className={cn(s.colorClass ?? defaultColors[i % defaultColors.length])}
            strokeWidth={14}
            strokeDasharray={`${len} ${C - len}`}
            strokeDashoffset={-offset}
            transform={`rotate(-90 ${cx} ${cy})`}
            style={{ transition: 'stroke-dasharray 200ms ease' }}
          />
        )
        offset += len
        return el
      })}
      {centerLabel && (
        <text
          x={cx}
          y={cy + 4}
          textAnchor="middle"
          fontSize="13"
          fill="currentColor"
          className="font-bold fill-on-surface"
        >
          {centerLabel}
        </text>
      )}
    </svg>
  )
}

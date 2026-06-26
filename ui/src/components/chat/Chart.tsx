import { memo, useMemo } from 'react'

export interface ChartSpec {
  type: 'bar' | 'line' | 'pie'
  title?: string
  x_label?: string
  y_label?: string
  data: { label: string; value: number }[]
}

interface ChartProps {
  spec: ChartSpec
}

const PADDING = { top: 32, right: 24, bottom: 48, left: 56 }
const WIDTH = 640
const HEIGHT = 360

export const Chart = memo(function Chart({ spec }: ChartProps) {
  if (spec.type === 'pie') return <PieChart spec={spec} />
  return <CartesianChart spec={spec} />
})

interface CartesianChartProps {
  spec: ChartSpec
}

function CartesianChart({ spec }: CartesianChartProps) {
  const geometry = useMemo(() => buildCartesianGeometry(spec), [spec])
  if (!geometry) return <ChartError message={`Invalid chart spec — type "${spec.type}" needs at least one data point.`} />

  const { points, bars, max, min, plotW, plotH, x0, y0 } = geometry
  const yRange = max - min || 1
  const ticks = buildYAxisTicks(min, max)

  return (
    <figure className="my-md p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/30 overflow-x-auto">
      {spec.title && (
        <figcaption className="text-label-md font-bold text-on-surface mb-xs text-center">
          {spec.title}
        </figcaption>
      )}
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        className="w-full h-auto"
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label={spec.title || 'chart'}
      >
        {ticks.map((tv) => {
          const y = y0 - ((tv - min) / yRange) * plotH
          return (
            <g key={tv}>
              <line
                x1={x0}
                y1={y}
                x2={x0 + plotW}
                y2={y}
                stroke="currentColor"
                strokeOpacity={0.1}
              />
              <text x={x0 - 8} y={y + 4} textAnchor="end" fontSize={11} fill="currentColor" fillOpacity={0.6}>
                {formatTick(tv)}
              </text>
            </g>
          )
        })}

        {bars.map((b, i) => (
          <g key={`bar-${i}`}>
            <rect
              x={b.x}
              y={b.y}
              width={b.w}
              height={b.h}
              fill="currentColor"
              fillOpacity={0.7}
              className="text-primary"
              rx={2}
            />
            <text
              x={b.x + b.w / 2}
              y={y0 + 18}
              textAnchor="middle"
              fontSize={11}
              fill="currentColor"
              fillOpacity={0.7}
            >
              {truncate(b.label, 12)}
            </text>
          </g>
        ))}

        {points.length > 1 && spec.type === 'line' && (
          <polyline
            points={points.map((p) => `${p.x},${p.y}`).join(' ')}
            fill="none"
            stroke="currentColor"
            strokeWidth={2}
            className="text-primary"
          />
        )}

        {points.map((p, i) => (
          <g key={`pt-${i}`}>
            <circle
              cx={p.x}
              cy={p.y}
              r={3}
              fill="currentColor"
              className="text-primary"
            />
            <text
              x={p.x}
              y={y0 + 18}
              textAnchor="middle"
              fontSize={11}
              fill="currentColor"
              fillOpacity={0.7}
            >
              {truncate(p.label, 12)}
            </text>
          </g>
        ))}

        <line x1={x0} y1={y0} x2={x0 + plotW} y2={y0} stroke="currentColor" strokeOpacity={0.4} />
        <line x1={x0} y1={y0 - plotH} x2={x0} y2={y0} stroke="currentColor" strokeOpacity={0.4} />

        {spec.x_label && (
          <text x={x0 + plotW / 2} y={HEIGHT - 8} textAnchor="middle" fontSize={12} fill="currentColor" fillOpacity={0.7}>
            {spec.x_label}
          </text>
        )}
        {spec.y_label && (
          <text
            x={12}
            y={y0 - plotH / 2}
            textAnchor="middle"
            fontSize={12}
            fill="currentColor"
            fillOpacity={0.7}
            transform={`rotate(-90 12 ${y0 - plotH / 2})`}
          >
            {spec.y_label}
          </text>
        )}
      </svg>
      <span className="sr-only">
        {spec.data.length} data points. Range: {formatTick(min)} to {formatTick(max)}.
      </span>
    </figure>
  )
}

interface PieChartProps {
  spec: ChartSpec
}

const PIE_COLORS = [
  'var(--chart-series-1)',
  'var(--chart-series-2)',
  'var(--chart-series-3)',
  'var(--chart-series-4)',
  'var(--chart-series-5)',
  'var(--chart-series-6)',
  'var(--chart-series-7)',
  'var(--chart-series-8)',
]

function PieChart({ spec }: PieChartProps) {
  const total = spec.data.reduce((s, d) => s + d.value, 0)
  if (spec.data.length === 0 || total === 0) {
    return <ChartError message="Pie chart needs at least one non-zero value." />
  }

  const cx = WIDTH / 2
  const cy = HEIGHT / 2
  const r = Math.min(WIDTH, HEIGHT) / 2 - 48

  let angle = -Math.PI / 2
  const slices = spec.data.map((d, i) => {
    const slice = (d.value / total) * 2 * Math.PI
    const startAngle = angle
    const endAngle = angle + slice
    angle = endAngle
    return {
      ...d,
      color: PIE_COLORS[i % PIE_COLORS.length],
      path: arcPath(cx, cy, r, startAngle, endAngle),
      midAngle: (startAngle + endAngle) / 2,
    }
  })

  return (
    <figure className="my-md p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/30">
      {spec.title && (
        <figcaption className="text-label-md font-bold text-on-surface mb-xs text-center">
          {spec.title}
        </figcaption>
      )}
      <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} className="w-full h-auto" role="img" aria-label={spec.title || 'pie chart'}>
        {slices.map((s, i) => (
          <path
            key={i}
            d={s.path}
            fill={s.color}
            stroke="var(--color-on-primary)"
            strokeWidth={1.5}
          />
        ))}
        {slices.map((s, i) => {
          const labelR = r * 0.7
          const lx = cx + Math.cos(s.midAngle) * labelR
          const ly = cy + Math.sin(s.midAngle) * labelR
          const pct = ((s.value / total) * 100).toFixed(0)
          return (
            <text
              key={`label-${i}`}
              x={lx}
              y={ly}
              textAnchor="middle"
              fontSize={12}
              fontWeight="bold"
              fill="var(--color-on-primary)"
            >
              {pct}%
            </text>
          )
        })}
      </svg>
      <ul className="flex flex-wrap gap-md justify-center mt-xs text-label-sm">
        {slices.map((s, i) => (
          <li key={i} className="flex items-center gap-xs">
            <span className="inline-block w-3 h-3 rounded-sm" style={{ background: s.color }} aria-hidden="true" />
            <span className="text-on-surface-variant">
              {truncate(s.label, 16)} · {formatTick(s.value)}
            </span>
          </li>
        ))}
      </ul>
    </figure>
  )
}

function ChartError({ message }: { message: string }) {
  return (
    <div className="my-md p-sm rounded-lg bg-error-container/20 border border-error/30 text-label-sm text-error">
      <span className="material-symbols-outlined text-[14px] align-middle mr-xs">error</span>
      {message}
    </div>
  )
}

interface CartesianGeometry {
  points: { x: number; y: number; label: string; value: number }[]
  bars: { x: number; y: number; w: number; h: number; label: string; value: number }[]
  max: number
  min: number
  plotW: number
  plotH: number
  x0: number
  y0: number
}

function buildCartesianGeometry(spec: ChartSpec): CartesianGeometry | null {
  if (spec.data.length === 0) return null
  const x0 = PADDING.left
  const y0 = HEIGHT - PADDING.bottom
  const plotW = WIDTH - PADDING.left - PADDING.right
  const plotH = HEIGHT - PADDING.top - PADDING.bottom
  const stepX = spec.data.length > 1 ? plotW / (spec.data.length - 1) : plotW
  const values = spec.data.map((d) => d.value)
  const max = Math.max(...values, 0)
  const min = Math.min(...values, 0)
  const yRange = max - min || 1

  const points = spec.data.map((d, i) => ({
    x: x0 + i * stepX,
    y: y0 - ((d.value - min) / yRange) * plotH,
    label: d.label,
    value: d.value,
  }))

  const barSlot = plotW / spec.data.length
  const barW = Math.min(barSlot * 0.7, 48)
  const bars = spec.data.map((d, i) => {
    const zeroY = y0 - ((0 - min) / yRange) * plotH
    const valueY = y0 - ((d.value - min) / yRange) * plotH
    const top = Math.min(zeroY, valueY)
    const h = Math.max(Math.abs(zeroY - valueY), 1)
    return {
      x: x0 + i * barSlot + (barSlot - barW) / 2,
      y: top,
      w: barW,
      h,
      label: d.label,
      value: d.value,
    }
  })

  return { points, bars, max, min, plotW, plotH, x0, y0 }
}

function buildYAxisTicks(min: number, max: number): number[] {
  const range = max - min || 1
  const step = niceStep(range / 5)
  const start = Math.floor(min / step) * step
  const end = Math.ceil(max / step) * step
  const ticks: number[] = []
  for (let v = start; v <= end + step / 2; v += step) ticks.push(v)
  return ticks
}

function niceStep(raw: number): number {
  const exp = Math.floor(Math.log10(raw || 1))
  const base = Math.pow(10, exp)
  const m = raw / base
  if (m < 1.5) return base
  if (m < 3) return 2 * base
  if (m < 7) return 5 * base
  return 10 * base
}

function formatTick(v: number): string {
  const abs = Math.abs(v)
  if (abs >= 1_000_000) return (v / 1_000_000).toFixed(1) + 'M'
  if (abs >= 1_000) return (v / 1_000).toFixed(1) + 'k'
  if (Number.isInteger(v)) return String(v)
  return v.toFixed(1)
}

function truncate(s: string, n: number): string {
  return s.length <= n ? s : s.slice(0, n - 1) + '…'
}

function arcPath(cx: number, cy: number, r: number, start: number, end: number): string {
  const x0 = cx + Math.cos(start) * r
  const y0 = cy + Math.sin(start) * r
  const x1 = cx + Math.cos(end) * r
  const y1 = cy + Math.sin(end) * r
  const largeArc = end - start > Math.PI ? 1 : 0
  return `M ${cx} ${cy} L ${x0} ${y0} A ${r} ${r} 0 ${largeArc} 1 ${x1} ${y1} Z`
}

export function parseChartSpec(text: string): ChartSpec | null {
  try {
    const obj = JSON.parse(text)
    if (typeof obj !== 'object' || obj === null) return null
    if (!Array.isArray(obj.data)) return null
    if (!['bar', 'line', 'pie'].includes(obj.type)) return null
    return obj as ChartSpec
  } catch {
    return null
  }
}

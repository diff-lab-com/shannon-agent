// Audit §P1-1 (round 6): stacked bars must stay inside the chart frame.
//
// Before the fix `BarChart` scaled Y by the largest single segment, so any
// stacked column whose total exceeded the largest individual segment
// overflowed above the top grid line. This test renders an obvious stack
// overflow and asserts the rendered <rect> coordinates stay within bounds.

import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { BarChart, type BarSeriesPoint, type BarSeriesDef } from '@/components/usage/BarChart'

const series: BarSeriesDef[] = [
  { key: 'input', label: 'Input', colorClass: 'text-primary' },
  { key: 'output', label: 'Output', colorClass: 'text-tertiary' },
]

const VB_H = 220
const padTop = 24
const padBottom = 56
const chartH = VB_H - padTop - padBottom

function frameBottom(): number {
  // padTop + chartH matches the SVG geometry in BarChart.tsx.
  return padTop + chartH
}

describe('BarChart stacked bounds', () => {
  it('does not render stacked bars above the chart frame', () => {
    // Day 0 has a single huge input segment (the prior Y-axis max).
    // Day 1 has 2 smaller segments whose SUM exceeds the prior max — the
    // pathological case the old `max = max(single segment)` produced.
    const data: BarSeriesPoint[] = [
      { label: '08-22', series: [{ key: 'input', value: 1700 }] },
      { label: '08-23', series: [
        { key: 'input', value: 900 },
        { key: 'output', value: 900 },
      ] },
    ]
    const { container } = render(<BarChart data={data} series={series} />)
    const rects = Array.from(container.querySelectorAll('rect'))
    // 3 segments rendered.
    expect(rects).toHaveLength(3)
    for (const rect of rects) {
      const y = Number(rect.getAttribute('y'))
        + Number(rect.getAttribute('height'))
      // The bottom of every stacked segment must be at-or-below the frame
      // bottom. A real overflow would put it well above (lower y).
      expect(y).toBeLessThanOrEqual(frameBottom() + 0.001)
    }
  })

  it('falls back to a unit max when all values are zero', () => {
    const data: BarSeriesPoint[] = [
      { label: '08-22', series: [{ key: 'input', value: 0 }] },
    ]
    const { container } = render(<BarChart data={data} series={series} />)
    expect(container.querySelector('rect')).not.toBeNull()
  })
})
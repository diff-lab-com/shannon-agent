import { describe, expect, it } from 'vitest'
import { niceAxisMax } from '../OpcAnalyticsDashboard'

// Review 2026-09-16 (UI-review #18): the daily-activity chart's Y axis shows
// max / max/2 / 0 — the axis max must be even so every tick is an integer.
describe('niceAxisMax', () => {
  it('keeps even maxima untouched', () => {
    expect(niceAxisMax(4)).toBe(4)
    expect(niceAxisMax(16)).toBe(16)
  })

  it('rounds odd maxima up to the next even value', () => {
    expect(niceAxisMax(5)).toBe(6)
    expect(niceAxisMax(3)).toBe(4)
    expect(niceAxisMax(7)).toBe(8)
  })

  it('always yields ticks that are integers', () => {
    for (const raw of [1, 2, 3, 5, 7, 9, 11, 13, 17, 25, 48, 97]) {
      const max = niceAxisMax(raw)
      expect(max % 2).toBe(0)
      expect((max / 2) % 1).toBe(0)
      expect(max).toBeGreaterThanOrEqual(raw)
    }
  })

  it('clamps to at least 1 (zero-data charts render one tick)', () => {
    expect(niceAxisMax(0)).toBe(2)
  })
})

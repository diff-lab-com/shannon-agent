// R2-3 — the composer picker's per-row meta suffix reuses the Settings
// catalog formatting (`formatPrice`) and never fabricates unknown values
// (ADR-0005 P0-2 honest cost/context).
import { describe, it, expect } from 'vitest'
import { formatPrice, modelPickerMeta } from '@/components/settings/models-settings/types'

describe('formatPrice', () => {
  it('formats known prices with two decimals', () => {
    expect(formatPrice(3)).toBe('3.00')
    expect(formatPrice(0.8)).toBe('0.80')
    expect(formatPrice(0)).toBe('0.00')
  })

  it('returns the em-dash placeholder for unknown prices', () => {
    expect(formatPrice(null)).toBe('—')
    expect(formatPrice(undefined)).toBe('—')
    expect(formatPrice(Number.NaN)).toBe('—')
  })
})

describe('modelPickerMeta', () => {
  it('renders context in k and per-million prices on one compact line', () => {
    expect(modelPickerMeta({ context_window: 200_000, price_in: 3, price_out: 15 })).toBe(
      '200k · $3.00/$15.00',
    )
  })

  it('keeps sub-k contexts as-is', () => {
    expect(modelPickerMeta({ context_window: 32_000, price_in: 0, price_out: 0 })).toBe(
      '32k · $0.00/$0.00',
    )
  })

  it('never fabricates: unknown context and prices each render as "—"', () => {
    expect(modelPickerMeta({ context_window: 0, price_in: null, price_out: undefined })).toBe(
      '— · $—/$—',
    )
    expect(modelPickerMeta({ context_window: 128_000, price_in: Number.NaN, price_out: 2 })).toBe(
      '128k · $—/$2.00',
    )
  })
})

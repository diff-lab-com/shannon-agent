// R3-3 — phaseTier.ts pure resolver tests. The backend
// (`desktop/src/phase_tier.rs` + `commands_chat::apply_tier_override`) is
// authoritative at query time; these pin the UI mirror so the controls can
// never render a different state than the next send will use.

import { describe, expect, it } from 'vitest'
import {
  effectivePhaseTier,
  normalizePhaseTierPref,
  phaseTierConfigValue,
  phaseTierLabelKey,
  resolveTierModel,
  type PhaseTierPref,
} from '@/lib/phaseTier'
import type { ModelInfo } from '@/types'

type Row = Pick<ModelInfo, 'id' | 'tier' | 'price_in' | 'price_out'>
const row = (id: string, tier: string | null, price_in?: number, price_out?: number): Row => ({
  id,
  tier,
  price_in: price_in ?? null,
  price_out: price_out ?? null,
})

describe('normalizePhaseTierPref', () => {
  it('maps canonical tiers and degrades everything else to inherit', () => {
    expect(normalizePhaseTierPref('fast')).toBe('fast')
    expect(normalizePhaseTierPref(' standard ')).toBe('standard')
    expect(normalizePhaseTierPref('PRO')).toBe('pro')
    expect(normalizePhaseTierPref(null)).toBe('inherit')
    expect(normalizePhaseTierPref(undefined)).toBe('inherit')
    expect(normalizePhaseTierPref('')).toBe('inherit')
    expect(normalizePhaseTierPref('inherit')).toBe('inherit')
    // TUI aliases / legacy junk never render as a wrong tier.
    expect(normalizePhaseTierPref('haiku')).toBe('inherit')
    expect(normalizePhaseTierPref('ultra')).toBe('inherit')
  })
})

describe('effectivePhaseTier (mirror of the backend precedence)', () => {
  it('uses the plan tier while approval_mode is plan', () => {
    expect(effectivePhaseTier('plan', 'pro', 'fast')).toBe('pro')
  })

  it('uses the act tier in every non-plan mode', () => {
    for (const mode of ['suggest', 'readonly', 'auto', 'full_auto', 'confirm', null, undefined]) {
      expect(effectivePhaseTier(mode, 'pro', 'fast')).toBe('fast')
    }
  })

  it('returns null for inherit / junk on the applicable phase', () => {
    expect(effectivePhaseTier('plan', 'inherit', 'fast')).toBeNull()
    expect(effectivePhaseTier('plan', 'ultra', 'fast')).toBeNull()
    expect(effectivePhaseTier('suggest', 'pro', 'inherit')).toBeNull()
    expect(effectivePhaseTier('suggest', null, null)).toBeNull()
  })
})

describe('resolveTierModel (display-side tier → model)', () => {
  const catalog: Row[] = [
    row('claude-haiku-4-5', 'fast', 0.8, 4),
    row('claude-sonnet-4-6', 'standard', 3, 15),
    row('claude-opus-4-7', 'pro', 15, 75),
    row('gpt-5-mini', 'fast', 0.25, 2),
  ]

  it('picks the cheapest model within the tier', () => {
    expect(resolveTierModel('fast', catalog)).toBe('gpt-5-mini')
    expect(resolveTierModel('standard', catalog)).toBe('claude-sonnet-4-6')
    expect(resolveTierModel('pro', catalog)).toBe('claude-opus-4-7')
  })

  it('returns null when no model carries the tier', () => {
    expect(resolveTierModel('standard', [row('a', 'fast', 1, 1)])).toBeNull()
    expect(resolveTierModel('fast', [])).toBeNull()
  })

  it('a priced model beats an unpriced one; ties keep list order', () => {
    const unpriced: Row[] = [row('unpriced-fast', 'fast'), row('priced-fast', 'fast', 5, 5)]
    expect(resolveTierModel('fast', unpriced)).toBe('priced-fast')
    // Equal totals → first listed stays (stable tie-break).
    const tie: Row[] = [row('first', 'fast', 1, 1), row('second', 'fast', 1, 1)]
    expect(resolveTierModel('fast', tie)).toBe('first')
  })

  it('sums input + output for the cheapest comparison', () => {
    const sums: Row[] = [row('cheap-in', 'fast', 0.1, 9), row('balanced', 'fast', 1, 1)]
    expect(resolveTierModel('fast', sums)).toBe('balanced')
  })
})

describe('wire mapping', () => {
  it('sends the literal pref as the configure value', () => {
    const prefs: PhaseTierPref[] = ['inherit', 'fast', 'standard', 'pro']
    expect(prefs.map(phaseTierConfigValue)).toEqual(['inherit', 'fast', 'standard', 'pro'])
  })

  it('labels come from the chat.phaseTier.tier namespace', () => {
    expect(phaseTierLabelKey('inherit')).toBe('chat.phaseTier.tier.inherit')
    expect(phaseTierLabelKey('pro')).toBe('chat.phaseTier.tier.pro')
  })
})

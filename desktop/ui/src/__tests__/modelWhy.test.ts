// S3-1 (P-N10/P-N11) — the why-active derivation: one test per precedence
// rung plus the noise-suppression edges. These pin the UI mirror of the
// engine's session override > phase tier > global default chain (the
// authoritative pin lives in commands_chat.rs; changing the semantics there
// must update BOTH).

import { describe, expect, it } from 'vitest'
import { modelWhyFor, tierSteer, type ModelWhyContext } from '@/lib/modelWhy'

const CATALOG = [
  { id: 'big-pro', tier: 'pro', price_in: 3, price_out: 15 },
  { id: 'mid-standard', tier: 'standard', price_in: 1, price_out: 2 },
  { id: 'tiny-fast', tier: 'fast', price_in: 0.1, price_out: 0.2 },
  { id: 'unpriced-pro', tier: 'pro', price_in: null, price_out: null },
] as const

function ctx(over: Partial<ModelWhyContext> = {}): ModelWhyContext {
  return {
    override: null,
    approvalMode: 'suggest',
    planTier: null,
    actTier: null,
    globalModel: 'big-pro',
    activeProfile: 'default',
    ...over,
  }
}

const row = (id: string) => ({ id, name: `Name of ${id}` })

describe('tierSteer', () => {
  it('returns null when both tiers inherit', () => {
    expect(tierSteer({ approvalMode: 'suggest', planTier: null, actTier: null }, CATALOG)).toBeNull()
  })

  it('picks the act tier outside plan mode', () => {
    expect(tierSteer({ approvalMode: 'suggest', planTier: 'pro', actTier: 'fast' }, CATALOG))
      .toEqual({ phase: 'act', modelId: 'tiny-fast' })
  })

  it('picks the plan tier in plan mode', () => {
    expect(tierSteer({ approvalMode: 'plan', planTier: 'pro', actTier: 'fast' }, CATALOG))
      .toEqual({ phase: 'plan', modelId: 'big-pro' })
  })

  it('returns null when the tier has no catalog match', () => {
    expect(tierSteer({ approvalMode: 'suggest', planTier: null, actTier: 'standard' }, [
      { id: 'x', tier: 'pro', price_in: 1, price_out: 1 },
    ])).toBeNull()
  })
})

describe('modelWhyFor', () => {
  it('labels the override row "session" — the top of the chain', () => {
    const why = modelWhyFor(row('tiny-fast'), ctx({ override: { provider: 'openai', model: 'tiny-fast' } }), CATALOG)
    expect(why).toEqual({ kind: 'session' })
  })

  it('matches the override by NAME too (the chip lookup contract)', () => {
    const why = modelWhyFor({ id: 'other-id', name: 'tiny-fast' }, ctx({ override: { provider: 'openai', model: 'tiny-fast' } }), CATALOG)
    expect(why).toEqual({ kind: 'session' })
  })

  it('labels the tier-resolved row when the tier steers AWAY from the default', () => {
    const why = modelWhyFor(row('tiny-fast'), ctx({ actTier: 'fast', globalModel: 'big-pro' }), CATALOG)
    expect(why).toEqual({ kind: 'tier', phase: 'act' })
  })

  it('suppresses the tier label when the tier resolves to the default model (no steering)', () => {
    const why = modelWhyFor(row('big-pro'), ctx({ actTier: 'pro', globalModel: 'big-pro' }), CATALOG)
    expect(why).toEqual({ kind: 'global' })
  })

  it('the session label wins over the tier label on the same row', () => {
    const why = modelWhyFor(
      row('tiny-fast'),
      ctx({ override: { provider: 'openai', model: 'tiny-fast' }, actTier: 'fast', globalModel: 'big-pro' }),
      CATALOG,
    )
    expect(why).toEqual({ kind: 'session' })
  })

  it('attributes the default row to its pinning profile when one is explicitly active', () => {
    const why = modelWhyFor(row('big-pro'), ctx({ activeProfile: 'lab' }), CATALOG)
    expect(why).toEqual({ kind: 'profile', profile: 'lab' })
  })

  it('renders the plain "global" label for the "default" sentinel', () => {
    expect(modelWhyFor(row('big-pro'), ctx({ activeProfile: 'default' }), CATALOG)).toEqual({ kind: 'global' })
    expect(modelWhyFor(row('big-pro'), ctx({ activeProfile: null }), CATALOG)).toEqual({ kind: 'global' })
    expect(modelWhyFor(row('big-pro'), ctx({ activeProfile: undefined }), CATALOG)).toEqual({ kind: 'global' })
  })

  it('non-effective rows carry no label', () => {
    expect(modelWhyFor(row('mid-standard'), ctx(), CATALOG)).toBeNull()
  })

  it('no session label on rows without a session context (Header surfaces)', () => {
    expect(modelWhyFor(row('big-pro'), ctx({ override: null, activeProfile: 'default' }), CATALOG)).toEqual({ kind: 'global' })
  })
})

// S2-1 capacity guardrails (裁定⑥) for the fetch-models curation flow.
//
// Pins:
//   - soft cap 50: beyond it "select all" is REFUSED (the over-cap path);
//   - within the cap "select all" is allowed (the confirm path — the
//     dialog itself is the component's job, this decides IF it may run);
//   - id-only projection: the selection becomes id-only specs, endpoint
//     order preserved, out-of-list selected ids appended.

import { describe, expect, it } from 'vitest'
import {
  MODEL_VAULT_SOFT_CAP,
  isOverCap,
  planSelectAll,
  selectedToInputs,
  selectionEquals,
  toggled,
} from '../components/settings/add-provider-modal/modelCuration'

describe('planSelectAll (裁定⑥ capacity guardrails)', () => {
  it('refuses select-all beyond the soft cap', () => {
    const ids = Array.from({ length: MODEL_VAULT_SOFT_CAP + 1 }, (_, i) => `model-${i}`)
    const decision = planSelectAll(ids)
    expect(decision).toEqual({
      ok: false,
      reason: 'over-cap',
      cap: MODEL_VAULT_SOFT_CAP,
      total: MODEL_VAULT_SOFT_CAP + 1,
    })
  })

  it('allows select-all at exactly the soft cap (confirm path)', () => {
    const ids = Array.from({ length: MODEL_VAULT_SOFT_CAP }, (_, i) => `model-${i}`)
    const decision = planSelectAll(ids)
    expect(decision.ok).toBe(true)
    if (decision.ok) expect(decision.ids).toEqual(ids)
  })

  it('allows select-all for normal curated-size lists', () => {
    const decision = planSelectAll(['a', 'b', 'c'])
    expect(decision.ok).toBe(true)
    if (decision.ok) expect(decision.ids).toEqual(['a', 'b', 'c'])
  })

  it('honors a custom cap', () => {
    expect(planSelectAll(['a', 'b', 'c'], 2)).toEqual({
      ok: false,
      reason: 'over-cap',
      cap: 2,
      total: 3,
    })
  })

  it('isOverCap mirrors the refusal boundary', () => {
    expect(isOverCap(50)).toBe(false)
    expect(isOverCap(51)).toBe(true)
  })
})

describe('selection helpers', () => {
  it('toggle adds and removes', () => {
    let s = new Set<string>()
    s = toggled(s, 'a', true)
    expect(s.has('a')).toBe(true)
    s = toggled(s, 'a', false)
    expect(s.has('a')).toBe(false)
  })

  it('selectedToInputs projects id-only specs in endpoint order', () => {
    const inputs = selectedToInputs(['m3', 'm1', 'm2'], new Set(['m2', 'm1']))
    expect(inputs).toEqual([{ id: 'm1' }, { id: 'm2' }])
  })

  it('selectedToInputs keeps selected ids that fell out of the refetched list', () => {
    // An existing declaration the endpoint no longer returns stays curated
    // until the user explicitly unchecks it.
    const inputs = selectedToInputs(['fresh'], new Set(['fresh', 'legacy-declared']))
    expect(inputs).toEqual([{ id: 'fresh' }, { id: 'legacy-declared' }])
  })

  it('selectedToInputs handles null fetch (edit mode without a refetch)', () => {
    expect(selectedToInputs(null, new Set(['x']))).toEqual([{ id: 'x' }])
    expect(selectedToInputs(null, new Set())).toEqual([])
  })

  it('selectionEquals is a value comparison', () => {
    expect(selectionEquals(new Set(['a', 'b']), new Set(['b', 'a']))).toBe(true)
    expect(selectionEquals(new Set(['a']), new Set(['a', 'b']))).toBe(false)
    expect(selectionEquals(new Set(), new Set())).toBe(true)
  })
})

// GB P2-4 — the shared approval-tier table (round-1 R3: FOUR tiers).
//
// History: the composer and Settings → General each carried their own
// five-value table over the same `approval_mode` key and never agreed; the
// first unification kept General's five — but two of them (suggest and
// confirm) behave IDENTICALLY engine-side (`"confirm" => Suggest` in
// desktop/src/commands.rs), so switching between them was a no-op. The
// controller ruling merges the table to four tiers named by REAL engine
// semantics: strict(readonly) / balanced(suggest) / permissive(auto_edit) /
// full(full_auto). These tests pin that shape.

import { describe, it, expect } from 'vitest'
import { APPROVAL_MODES, approvalModeOption } from '@/lib/approvalModes'

describe('APPROVAL_MODES (shared composer + settings table)', () => {
  it('lists exactly the four engine-distinct tiers, most to least supervised', () => {
    expect(APPROVAL_MODES.map(m => m.value)).toEqual([
      'readonly',
      'suggest',
      'auto_edit',
      'full_auto',
    ])
  })

  it('every tier maps to a DISTINCT engine behavior (no suggest≡confirm no-ops)', () => {
    const values = APPROVAL_MODES.map(m => m.value)
    expect(new Set(values).size).toBe(values.length)
    // The no-op pair is gone by construction…
    expect(values).not.toContain('confirm')
    // …and the engine folds "confirm" into Suggest, i.e. Balanced's value.
    // (Contract mirrored from desktop/src/commands.rs parse_approval_mode.)
    expect(values).toContain('suggest')
  })

  it('every tier carries label + description message ids, icon and tone', () => {
    for (const mode of APPROVAL_MODES) {
      expect(mode.labelKey, mode.value).toMatch(/^settings\.general\.approvalMode\.(strict|balanced|permissive|full)\./)
      expect(mode.descriptionKey, mode.value).toMatch(/^settings\.general\.approvalMode\.(strict|balanced|permissive|full)\./)
      expect(mode.icon, mode.value).toBeTruthy()
      expect(mode.tone, mode.value).toBeTruthy()
    }
  })
})

describe('approvalModeOption', () => {
  it('resolves each listed tier', () => {
    for (const mode of APPROVAL_MODES) {
      expect(approvalModeOption(mode.value)).toBe(mode)
    }
  })

  it('falls back to Balanced (suggest) for a blank/missing config value', () => {
    expect(approvalModeOption(null)).toBe(APPROVAL_MODES[1])
    expect(approvalModeOption(undefined)).toBe(APPROVAL_MODES[1])
    expect(approvalModeOption('')).toBe(APPROVAL_MODES[1])
    expect(APPROVAL_MODES[1].value).toBe('suggest')
  })

  it('the engine alias "auto" shows the permissive tier (same AutoEdit behavior)', () => {
    expect(approvalModeOption('auto')).toBe(APPROVAL_MODES[2])
  })

  it('plan keeps its own honest labels (owned by the composer plan toggle)', () => {
    const plan = approvalModeOption('plan')
    expect(plan.value).toBe('plan')
    expect(plan.labelKey).toBe('chat.input.mode.plan')
    expect(APPROVAL_MODES).not.toContain(plan)
  })

  it('R3: "confirm" is an OUT-OF-TABLE value — raw readout, never a fake tier', () => {
    const option = approvalModeOption('confirm')
    expect(option.value).toBe('confirm')
    expect(option.rawLabel).toBe('confirm')
    expect(APPROVAL_MODES).not.toContain(option)
    // And it must not masquerade as the balanced tier it behaves like.
    expect(option.labelKey).not.toBe(APPROVAL_MODES[1].labelKey)
  })

  it('unknown engine aliases keep their raw value as the pill label', () => {
    const option = approvalModeOption('dont_ask')
    expect(option.value).toBe('dont_ask')
    expect(option.rawLabel).toBe('dont_ask')
    expect(APPROVAL_MODES).not.toContain(option)
  })
})

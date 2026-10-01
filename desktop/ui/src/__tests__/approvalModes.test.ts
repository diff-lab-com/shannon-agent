// GB P2-4 — the shared five-tier approval table.
//
// The regression this guards: the composer and Settings → General each used
// to carry their own five-value table over the same `approval_mode` key and
// the two never agreed (composer listed readonly/auto; General listed
// confirm/auto_edit). Both surfaces now import the ONE table — these tests
// pin its shape and the display fallbacks for out-of-table engine modes.

import { describe, it, expect } from 'vitest'
import { APPROVAL_MODES, approvalModeOption } from '@/lib/approvalModes'

describe('APPROVAL_MODES (shared composer + settings table)', () => {
  it('lists exactly the five General-page tiers, most to least supervised', () => {
    expect(APPROVAL_MODES.map(m => m.value)).toEqual([
      'suggest',
      'confirm',
      'plan',
      'auto_edit',
      'full_auto',
    ])
  })

  it('every tier carries label + description message ids, icon and tone', () => {
    for (const mode of APPROVAL_MODES) {
      expect(mode.labelKey, mode.value).toMatch(/^settings\.general\.approvalMode\./)
      expect(mode.descriptionKey, mode.value).toMatch(/^settings\.general\.approvalMode\./)
      expect(mode.icon, mode.value).toBeTruthy()
      expect(mode.tone, mode.value).toBeTruthy()
    }
  })

  it('values are unique', () => {
    expect(new Set(APPROVAL_MODES.map(m => m.value)).size).toBe(APPROVAL_MODES.length)
  })
})

describe('approvalModeOption', () => {
  it('resolves each listed tier', () => {
    for (const mode of APPROVAL_MODES) {
      expect(approvalModeOption(mode.value)).toBe(mode)
    }
  })

  it('falls back to Suggest for a blank/missing config value (the composer default)', () => {
    expect(approvalModeOption(null)).toBe(APPROVAL_MODES[0])
    expect(approvalModeOption(undefined)).toBe(APPROVAL_MODES[0])
    expect(approvalModeOption('')).toBe(APPROVAL_MODES[0])
    expect(APPROVAL_MODES[0].value).toBe('suggest')
  })

  it('renders out-of-table engine modes honestly via fallback entries, not as Suggest', () => {
    // readonly / auto are real engine modes settable outside these five
    // (permission profiles, CLI /mode) — they keep their composer labels.
    const ro = approvalModeOption('readonly')
    expect(ro.value).toBe('readonly')
    expect(ro.labelKey).toBe('chat.input.mode.readonly')
    const auto = approvalModeOption('auto')
    expect(auto.value).toBe('auto')
    expect(auto.labelKey).toBe('chat.input.mode.auto')
  })

  it('unknown engine aliases keep their raw value as the pill label', () => {
    const option = approvalModeOption('dont_ask')
    expect(option.value).toBe('dont_ask')
    expect(option.rawLabel).toBe('dont_ask')
    // Never claim a listed tier it is not.
    expect(APPROVAL_MODES).not.toContain(option)
  })
})

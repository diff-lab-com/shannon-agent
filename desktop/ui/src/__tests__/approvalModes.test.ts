// approvalModes — the shared approval-tier table (4+3 model, 2026-10-05).
//
// History: the composer and Settings → General each carried their own
// five-value table over the same `approval_mode` key and never agreed; the
// round-1 R3 unification kept four tiers; the 2026-10-05 convergence moved
// to the 4+3 model (docs/plans/2026-10-04-permission-mode-naming-design.md):
// THREE autonomy-ladder quick tiers (ask / auto-edit / full-auto), plan as a
// composer-owned workflow tier, and readonly / dontAsk / bypassPermissions
// as EXPERT modes reachable from Settings → General "Advanced". Legacy
// stored values normalize to their canonical tier for display. These tests
// pin that shape.

import { describe, it, expect } from 'vitest'
import { ADVANCED_MODES, APPROVAL_MODES, approvalModeOption, normalizeApprovalMode } from '@/lib/approvalModes'

describe('APPROVAL_MODES (shared composer + settings quick tiers)', () => {
  it('lists exactly the three ladder stops, most to least supervised', () => {
    expect(APPROVAL_MODES.map(m => m.value)).toEqual([
      'ask',
      'auto-edit',
      'full-auto',
    ])
  })

  it('every tier maps to a DISTINCT engine behavior', () => {
    const values = APPROVAL_MODES.map(m => m.value)
    expect(new Set(values).size).toBe(values.length)
    // The old no-op pair (suggest≡confirm) is gone by construction — the
    // ladder base is `ask`, with both legacy spellings normalizing into it.
    expect(values).not.toContain('suggest')
    expect(values).not.toContain('confirm')
  })

  it('every tier carries label + description message ids, icon and tone', () => {
    for (const mode of APPROVAL_MODES) {
      expect(mode.labelKey, mode.value).toMatch(/^settings\.general\.approvalMode\.(ask|autoEdit|full)\./)
      expect(mode.descriptionKey, mode.value).toMatch(/^settings\.general\.approvalMode\.(ask|autoEdit|full)\./)
      expect(mode.icon, mode.value).toBeTruthy()
      expect(mode.tone, mode.value).toBeTruthy()
    }
  })
})

describe('ADVANCED_MODES (expert tiers, Settings "Advanced" picker)', () => {
  it('lists readonly / dontAsk / bypassPermissions, never quick-pickable', () => {
    expect(ADVANCED_MODES.map(m => m.value)).toEqual([
      'readonly',
      'dontAsk',
      'bypassPermissions',
    ])
    for (const adv of ADVANCED_MODES) {
      expect(APPROVAL_MODES.find(m => m.value === adv.value)).toBeUndefined()
    }
  })
})

describe('normalizeApprovalMode (legacy stored values)', () => {
  it('folds the legacy vocabulary into the canonical tiers', () => {
    expect(normalizeApprovalMode('suggest')).toBe('ask')
    expect(normalizeApprovalMode('confirm')).toBe('ask')
    expect(normalizeApprovalMode('default')).toBe('ask')
    expect(normalizeApprovalMode('auto')).toBe('auto-edit')
    expect(normalizeApprovalMode('auto_edit')).toBe('auto-edit')
    expect(normalizeApprovalMode('permissive')).toBe('auto-edit')
    expect(normalizeApprovalMode('full_auto')).toBe('full-auto')
    expect(normalizeApprovalMode('full')).toBe('full-auto')
    expect(normalizeApprovalMode('strict')).toBe('readonly')
    expect(normalizeApprovalMode('plan_ro')).toBe('readonly')
    expect(normalizeApprovalMode('dont_ask')).toBe('dontAsk')
    expect(normalizeApprovalMode('bypass_permissions')).toBe('bypassPermissions')
  })
})

describe('approvalModeOption', () => {
  it('resolves each listed ladder tier', () => {
    for (const mode of APPROVAL_MODES) {
      expect(approvalModeOption(mode.value)).toBe(mode)
    }
  })

  it('resolves each expert tier', () => {
    for (const mode of ADVANCED_MODES) {
      expect(approvalModeOption(mode.value)).toBe(mode)
    }
  })

  it('falls back to auto-edit (the engine default) for a blank/missing config value', () => {
    expect(approvalModeOption(null)).toBe(APPROVAL_MODES[1])
    expect(approvalModeOption(undefined)).toBe(APPROVAL_MODES[1])
    expect(approvalModeOption('')).toBe(APPROVAL_MODES[1])
    expect(APPROVAL_MODES[1].value).toBe('auto-edit')
  })

  it('the engine legacy alias "auto" shows the auto-edit tier (same AutoEdit behavior)', () => {
    expect(approvalModeOption('auto')).toBe(APPROVAL_MODES[1])
  })

  it('plan keeps its own honest labels (owned by the composer plan toggle)', () => {
    const plan = approvalModeOption('plan')
    expect(plan.value).toBe('plan')
    expect(plan.labelKey).toBe('chat.input.mode.plan')
    expect(APPROVAL_MODES).not.toContain(plan)
  })

  it('legacy "confirm" normalizes into the ask tier — no fake raw readout', () => {
    const option = approvalModeOption('confirm')
    expect(option.value).toBe('ask')
    expect(option).toBe(APPROVAL_MODES[0])
  })

  it('unknown engine aliases keep their raw value as the pill label', () => {
    const option = approvalModeOption('mystery-mode')
    expect(option.value).toBe('mystery-mode')
    expect(option.rawLabel).toBe('mystery-mode')
    expect(APPROVAL_MODES).not.toContain(option)
  })
})

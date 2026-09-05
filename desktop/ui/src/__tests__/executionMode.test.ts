import { describe, expect, it } from 'vitest'
import {
  deriveExecutionMode,
  isBuiltinProfile,
} from '@/lib/executionMode'
import type { DesktopConfig } from '@/types'

function cfg(partial: Partial<DesktopConfig>): DesktopConfig {
  return partial as DesktopConfig
}

describe('deriveExecutionMode (P1-3 mode switcher state machine)', () => {
  it('maps builtin profiles to their same-name tier', () => {
    expect(deriveExecutionMode(cfg({ active_permission_profile: 'strict' })).mode).toBe('strict')
    expect(deriveExecutionMode(cfg({ active_permission_profile: 'balanced' })).mode).toBe('balanced')
    expect(deriveExecutionMode(cfg({ active_permission_profile: 'permissive' })).mode).toBe('permissive')
  })

  it('falls back to balanced when no profile is active (legacy configs)', () => {
    expect(deriveExecutionMode(cfg({})).mode).toBe('balanced')
    expect(deriveExecutionMode(cfg({ active_permission_profile: null })).mode).toBe('balanced')
    expect(deriveExecutionMode(cfg({ active_permission_profile: '   ' })).mode).toBe('balanced')
    expect(deriveExecutionMode(null).mode).toBe('balanced')
    expect(deriveExecutionMode(undefined).profile).toBeNull()
  })

  it('shows the custom tier with the profile name for non-builtin profiles', () => {
    const state = deriveExecutionMode(cfg({ active_permission_profile: 'research-mode' }))
    expect(state.mode).toBe('custom')
    expect(state.profile).toBe('research-mode')
  })

  it('surfaces the approval mode for the label', () => {
    expect(
      deriveExecutionMode(cfg({ active_permission_profile: 'permissive', approval_mode: 'auto_edit' }))
        .approvalMode,
    ).toBe('auto_edit')
  })

  it('isBuiltinProfile is case-insensitive and null-safe', () => {
    expect(isBuiltinProfile('Strict')).toBe(true)
    expect(isBuiltinProfile('BALANCED')).toBe(true)
    expect(isBuiltinProfile('research-mode')).toBe(false)
    expect(isBuiltinProfile(null)).toBe(false)
    expect(isBuiltinProfile(undefined)).toBe(false)
  })
})

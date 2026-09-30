// R3-2 (desktop slice) — providerProfiles.ts pure state-logic tests.
// Per the batch brief: state logic only, no popup interactions (the
// ConfirmDialog flow is covered in ProfilesSection.test.tsx via plain
// button events; Base-UI-style popups stay e2e territory).

import { describe, expect, it } from 'vitest'
import {
  activeProfileName,
  isDuplicateName,
  needsEmptyConfirm,
  validateProfileName,
} from '@/lib/providerProfiles'
import type { ProviderProfileSummary } from '@/types'

const row = (name: string, provider_count = 1, active = false): ProviderProfileSummary => ({
  name,
  provider_count,
  active,
  model: provider_count > 0 ? 'some-model' : null,
})

describe('validateProfileName (client mirror of the engine validator)', () => {
  it('accepts a normal single-token name', () => {
    expect(validateProfileName('work')).toBeNull()
    expect(validateProfileName('  work  ')).toBeNull()
    expect(validateProfileName('Work-2_Profile')).toBeNull()
  })

  it('rejects empty / whitespace-only names', () => {
    expect(validateProfileName('')).toBe('settings.models.profiles.nameRequired')
    expect(validateProfileName('   ')).toBe('settings.models.profiles.nameRequired')
  })

  it('rejects names over 64 chars', () => {
    expect(validateProfileName('a'.repeat(65))).toBe('settings.models.profiles.nameTooLong')
    expect(validateProfileName('a'.repeat(64))).toBeNull()
  })

  it('rejects whitespace and control characters (TOML key safety)', () => {
    expect(validateProfileName('my profile')).toBe('settings.models.profiles.nameInvalid')
    expect(validateProfileName('tab\tname')).toBe('settings.models.profiles.nameInvalid')
    expect(validateProfileName('line\nname')).toBe('settings.models.profiles.nameInvalid')
    expect(validateProfileName('ctl\u0007name')).toBe('settings.models.profiles.nameInvalid')
  })
})

describe('needsEmptyConfirm', () => {
  it('flags provider-less profiles for the confirm dialog', () => {
    expect(needsEmptyConfirm(row('fresh', 0))).toBe(true)
    expect(needsEmptyConfirm(row('default', 2))).toBe(false)
  })
})

describe('isDuplicateName', () => {
  it('blocks exact and case-insensitive collisions', () => {
    const rows = [row('default'), row('work')]
    expect(isDuplicateName(rows, 'work')).toBe(true)
    expect(isDuplicateName(rows, '  WORK ')).toBe(true)
    expect(isDuplicateName(rows, 'research')).toBe(false)
  })
})

describe('activeProfileName', () => {
  it('follows the backend active marker', () => {
    const rows = [row('default', 2, false), row('work', 1, true)]
    expect(activeProfileName(rows)).toBe('work')
  })

  it('is null when nothing is marked (fresh store)', () => {
    expect(activeProfileName([])).toBeNull()
    expect(activeProfileName([row('default', 0, false)])).toBeNull()
  })
})

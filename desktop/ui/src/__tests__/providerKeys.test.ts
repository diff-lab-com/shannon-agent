// R4-3 (desktop slice) — pure state logic for the per-provider "API keys"
// panel. All derivations the panel renders from, pinned against the
// engine's credential-store semantics (slot 0 = ACTIVE; removing the
// active key promotes the next stored one; the last remaining key cannot
// be removed).

import { describe, expect, it } from 'vitest'
import {
  activeKeyIndex,
  canRemoveKey,
  keyHintAt,
  removalPromotesNext,
  validateNewKey,
} from '@/lib/providerKeys'
import type { ProviderKeySummary } from '@/types'

function row(index: number, masked_hint: string, active = false): ProviderKeySummary {
  return { index, active, masked_hint }
}

const ROTATION: ProviderKeySummary[] = [
  row(0, 'sk-ant…aaaa', true),
  row(1, 'sk-ant…bbbb'),
  row(2, 'sk-ant…cccc'),
]

describe('providerKeys state logic (R4-3 desktop)', () => {
  it('derives the active key from the marker, not from position assumptions', () => {
    expect(activeKeyIndex(ROTATION)).toBe(0)
    expect(activeKeyIndex([row(0, 'sk-x…1111'), row(1, 'sk-x…2222', true)])).toBe(1)
    expect(activeKeyIndex([])).toBeNull()
  })

  it('allows removing every key except the LAST remaining one', () => {
    expect(canRemoveKey(ROTATION, 0)).toBe(true)
    expect(canRemoveKey(ROTATION, 2)).toBe(true)
    // Single-key provider: the engine refuses — the UI disables instead.
    expect(canRemoveKey([row(0, 'sk-only…k1', true)], 0)).toBe(false)
    // Unknown index (already gone) is not removable.
    expect(canRemoveKey(ROTATION, 9)).toBe(false)
  })

  it('flags the promotion warning only for the ACTIVE key of a multi-key list', () => {
    expect(removalPromotesNext(ROTATION, 0)).toBe(true)
    expect(removalPromotesNext(ROTATION, 1)).toBe(false)
    // Even the active key of a single-key list promotes nothing.
    expect(removalPromotesNext([row(0, 'sk-only…k1', true)], 0)).toBe(false)
  })

  it('reads masked hints for confirmation copy', () => {
    expect(keyHintAt(ROTATION, 1)).toBe('sk-ant…bbbb')
    expect(keyHintAt(ROTATION, 9)).toBeNull()
  })

  it('rejects blank add attempts client-side (duplicates are backend-only)', () => {
    expect(validateNewKey('   ')).toBe('settings.models.keys.keyRequired')
    expect(validateNewKey('')).toBe('settings.models.keys.keyRequired')
    expect(validateNewKey('sk-ant-api03-real-key')).toBeNull()
  })
})

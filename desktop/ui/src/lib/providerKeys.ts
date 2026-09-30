// R4-3 (desktop slice) — pure state logic for the per-provider "API keys"
// panel: rotation-order derivation and the remove/activate confirmation
// rules, mirroring the engine's credential-store semantics
// (`CredentialManager::remove_key` promotes the next stored key when slot
// 0 goes away and refuses to remove the LAST remaining key). No
// Tauri/React imports — all unit-testable (jsdom popup interactions are
// deliberately NOT tested; that coverage is e2e's).

import type { ProviderKeySummary } from '@/types'

/**
 * The rotation position of the ACTIVE key. The backend stores the active
 * key at slot 0, but the UI derives from the marker rather than assuming —
 * a list where nothing is marked (empty provider) yields null.
 */
export function activeKeyIndex(rows: ReadonlyArray<ProviderKeySummary>): number | null {
  return rows.find((r) => r.active)?.index ?? null
}

/**
 * True when the engine accepts a remove for `index`: any row except the
 * LAST remaining one (a credential entry always keeps at least one key —
 * replacing the final key goes through the provider's Edit modal).
 */
export function canRemoveKey(rows: ReadonlyArray<ProviderKeySummary>, index: number): boolean {
  return rows.length > 1 && rows.some((r) => r.index === index)
}

/**
 * True when removing `index` would PROMOTE the next stored key (the row is
 * the currently active one) — the delete confirmation says this out loud.
 */
export function removalPromotesNext(rows: ReadonlyArray<ProviderKeySummary>, index: number): boolean {
  return activeKeyIndex(rows) === index && rows.length > 1
}

/** The masked hint of the key at `index`, for confirmation/toast copy. */
export function keyHintAt(rows: ReadonlyArray<ProviderKeySummary>, index: number): string | null {
  return rows.find((r) => r.index === index)?.masked_hint ?? null
}

/**
 * Client-side gate for the add form: the backend rejects empty/blank keys
 * (duplicates too, but the UI never sees full key material so it cannot
 * pre-check those — the backend error names the offending slot). Checking
 * the blank case here keeps the trivial mistake off the wire.
 * Returns the i18n message key for the failure, or null when acceptable.
 */
export type ProviderKeyErrorKey = 'settings.models.keys.keyRequired'

export function validateNewKey(raw: string): ProviderKeyErrorKey | null {
  if (raw.trim().length === 0) return 'settings.models.keys.keyRequired'
  return null
}

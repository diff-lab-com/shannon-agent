// R3-2 (desktop slice) — pure state logic for the Settings → Models
// "Profiles" section: client-side name validation (mirroring the engine's
// shared `validate_profile_name` so the form rejects before the backend
// round trip), the empty-profile switch confirmation rule, and row
// derivation. No Tauri/React imports — all unit-testable (jsdom popup
// interactions are deliberately NOT tested; that coverage is e2e's).

import type { ProviderProfileSummary } from '@/types'

/**
 * Client mirror of the engine's `validate_profile_name` (shannon-types):
 * trimmed non-empty, ≤ 64 chars, no whitespace, no control characters —
 * the name is the `profiles` TOML table key and the switch argument.
 * Returns the i18n message key for the failure, or null when valid.
 */
export type ProfileNameErrorKey =
  | 'settings.models.profiles.nameRequired'
  | 'settings.models.profiles.nameTooLong'
  | 'settings.models.profiles.nameInvalid'

export function validateProfileName(raw: string): ProfileNameErrorKey | null {
  const name = raw.trim()
  if (name.length === 0) return 'settings.models.profiles.nameRequired'
  if (name.length > 64) return 'settings.models.profiles.nameTooLong'
  if (/[\s]/.test(name)) return 'settings.models.profiles.nameInvalid'
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001F\u007F]/.test(name)) return 'settings.models.profiles.nameInvalid'
  return null
}

/** True when the row has no provider slots — switching needs a confirm. */
export function needsEmptyConfirm(row: ProviderProfileSummary): boolean {
  return row.provider_count === 0
}

/**
 * Does `name` collide with an existing row (case-insensitively — the
 * backend map is exact-match, but a near-duplicate display name reads as
 * a bug, so the form blocks it early)?
 */
export function isDuplicateName(rows: ReadonlyArray<ProviderProfileSummary>, raw: string): boolean {
  const name = raw.trim().toLowerCase()
  return rows.some((r) => r.name.toLowerCase() === name)
}

/**
 * Which row is active: the row the backend marks (the engine's
 * `active_profile` pointer), or null when nothing is marked (fresh store
 * with no profiles at all). The UI never fabricates an active marker.
 */
export function activeProfileName(rows: ReadonlyArray<ProviderProfileSummary>): string | null {
  return rows.find((r) => r.active)?.name ?? null
}

// ── R5: rename / delete (the R3-2 deferred slice) ────────────────────────

/**
 * Does `newName` collide with an existing row OTHER than `currentName`
 * (the rename form's duplicate rule — renaming a profile to its own
 * current name, possibly re-cased, is allowed and short-circuits to a
 * no-op)? Case-insensitive, matching `isDuplicateName`.
 */
export function isDuplicateRename(
  rows: ReadonlyArray<ProviderProfileSummary>,
  currentName: string,
  newName: string,
): boolean {
  const next = newName.trim().toLowerCase()
  return rows.some((r) => r.name !== currentName && r.name.toLowerCase() === next)
}

/**
 * Which profile the engine's active pointer falls back to when `name` is
 * deleted — the client mirror of `remove_model_profile`'s fallback rule,
 * so the delete confirmation can say out loud who takes over BEFORE the
 * destructive call:
 *
 * 1. `"default"` when it survives (the deleted profile is not `"default"`
 *    and `"default"` still exists);
 * 2. else the alphabetically-first remaining profile
 *    (`profile_names()` is sorted engine-side);
 * 3. else null (nothing remains — the engine refuses deleting the LAST
 *    profile outright, so this arm only matters for display symmetry).
 */
export function profileFallbackAfterDelete(
  rows: ReadonlyArray<ProviderProfileSummary>,
  name: string,
): string | null {
  const remaining = rows.filter((r) => r.name !== name).map((r) => r.name)
  if (name !== 'default' && remaining.includes('default')) return 'default'
  return [...remaining].sort()[0] ?? null
}

/** True when the engine would refuse the delete (the LAST profile cannot
 *  be removed — a config must keep at least one). The UI disables the
 *  affordance instead of surfacing the backend error. */
export function isLastRemainingProfile(rows: ReadonlyArray<ProviderProfileSummary>): boolean {
  return rows.length <= 1
}

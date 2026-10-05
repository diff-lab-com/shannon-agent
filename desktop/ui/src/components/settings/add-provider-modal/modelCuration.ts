// S2-1 (模型仓固化) — pure curation logic for the fetch-models multi-select.
//
// The curated selection becomes the provider slot's `models: Vec<ModelSpec>`
// in providers.toml v2 (the "model vault"). The guardrails here implement
// 裁定⑥'s capacity policy on the client:
//   - default zero selected (the component initializes the set),
//   - soft cap 50: a fetched list beyond the cap shows a warning and
//     "select all" is REFUSED (over-cap selections are almost always an
//     accidental select-all on a 400-model endpoint — OpenRouter precedent),
//   - within the cap, select-all still requires the UI's confirm dialog
//     (the component owns that; this module only decides IF it may run).
//
// Pure + unit-tested (`__tests__/modelCuration.test.ts`); no React, no API.

import type { DeclaredModelInput } from '@/types'

/// Soft cap on curated models per provider (裁定⑥, adjustable by policy —
/// competitive curated usage is 5-20; 50 covers it with headroom).
export const MODEL_VAULT_SOFT_CAP = 50

export type SelectAllDecision =
  | { ok: true; ids: string[] }
  | { ok: false; reason: 'over-cap'; cap: number; total: number }

/// Decide whether "select all" may run over `allIds`. Beyond the soft cap
/// the request is refused — the UI renders the warning and keeps the
/// button inert.
export function planSelectAll(allIds: string[], cap = MODEL_VAULT_SOFT_CAP): SelectAllDecision {
  if (allIds.length > cap) {
    return { ok: false, reason: 'over-cap', cap, total: allIds.length }
  }
  return { ok: true, ids: allIds }
}

/// True when the fetched list size trips the soft-cap warning path.
export function isOverCap(fetchedCount: number, cap = MODEL_VAULT_SOFT_CAP): boolean {
  return fetchedCount > cap
}

/// Toggle one id in a selection set (pure — returns a new Set).
export function toggled(selection: Set<string>, id: string, checked: boolean): Set<string> {
  const next = new Set(selection)
  if (checked) {
    next.add(id)
  } else {
    next.delete(id)
  }
  return next
}

/// Project the selection into the wire input for `set_provider_models`.
/// A selected id with no extra metadata becomes an id-only spec (the
/// catalog keeps supplying pricing/context/vision for ids it knows, 裁定③
/// merge-priority pin). Order follows `fetched` first (the endpoint's
/// order), then any selected ids not in the fetched list (e.g. an existing
/// declaration the refetch no longer returns — it stays curated unless the
/// user unchecks it).
export function selectedToInputs(
  fetched: string[] | null,
  selected: Set<string>,
): DeclaredModelInput[] {
  const inputs: DeclaredModelInput[] = []
  const seen = new Set<string>()
  for (const id of fetched ?? []) {
    if (selected.has(id) && !seen.has(id)) {
      inputs.push({ id })
      seen.add(id)
    }
  }
  for (const id of selected) {
    if (!seen.has(id)) {
      inputs.push({ id })
      seen.add(id)
    }
  }
  return inputs
}

/// Shallow equality for selection sets (the modal's dirty guard).
export function selectionEquals(a: Set<string>, b: Set<string>): boolean {
  if (a.size !== b.size) return false
  for (const id of a) {
    if (!b.has(id)) return false
  }
  return true
}

// R3-3 — Plan/Act dual-tier model preference: pure derivation + display
// resolution for the header switcher and the Settings → Models dropdowns.
//
// The BACKEND is authoritative at query time: `desktop/src/phase_tier.rs`
// decides which phase tier applies (approval mode `plan` ⇒ plan tier, else
// act tier) and resolves it to a concrete model through the engine's
// `resolve_tier` — the same resolution the TUI's `/model --tier` performs.
// This module mirrors those rules on the UI side so the controls render
// the same state the next send will use, and — for the "which model would
// this tier pick?" display line — consults the SAME catalog the picker
// lists (`list_models`, whose entries now carry the catalog `tier` label).
//
// Precedence (roadmap R3-3): **session model override (R2-1) > phase tier
// > global default** — a phase tier only applies when the session has no
// explicit override, and both phases default to `inherit` (= no override).

import type { ModelInfo } from '@/types'

/** Canonical tier names — the same vocabulary the engine's tier system and
 *  the Add Provider modal's per-provider `tiers` table speak. */
export type ModelTier = 'fast' | 'standard' | 'pro'

/** A phase-tier preference value. `inherit` = no override for that phase. */
export type PhaseTierPref = 'inherit' | ModelTier

/** Frozen option order for both controls (header popover + settings dropdowns). */
export const PHASE_TIER_PREFS: readonly PhaseTierPref[] = ['inherit', 'fast', 'standard', 'pro'] as const

/** The config value sent to `configure('plan_tier' | 'act_tier')` for a
 *  preference. `inherit` is sent literally — the backend clears the stored
 *  preference when it sees it (empty is accepted too). */
export function phaseTierConfigValue(pref: PhaseTierPref): string {
  return pref
}

/**
 * Lenient reader for a persisted preference: anything that isn't a
 * canonical tier name (null, unset, legacy junk, TUI aliases like `haiku`)
 * degrades to `inherit`, exactly like the backend's `normalize_tier_pref`.
 * A bad stored value must render as "inherit", never as a wrong tier.
 */
export function normalizePhaseTierPref(raw: string | null | undefined): PhaseTierPref {
  switch ((raw ?? '').trim().toLowerCase()) {
    case 'fast':
      return 'fast'
    case 'standard':
      return 'standard'
    case 'pro':
      return 'pro'
    default:
      return 'inherit'
  }
}

/**
 * Which phase tier applies right now — the UI mirror of the backend's
 * `effective_phase_tier`. `approvalMode` is the desktop config's global
 * approval mode; the composer's plan mode writes `"plan"` into it, and
 * every other mode counts as the act phase. Returns null when the
 * applicable phase is `inherit` (no override → global default).
 */
export function effectivePhaseTier(
  approvalMode: string | null | undefined,
  planTier: string | null | undefined,
  actTier: string | null | undefined,
): ModelTier | null {
  const pref = (approvalMode ?? '').trim() === 'plan'
    ? normalizePhaseTierPref(planTier)
    : normalizePhaseTierPref(actTier)
  return pref === 'inherit' ? null : pref
}

/**
 * Display resolution: which catalog model would this tier pick for the
 * picker's list? Mirrors the engine tie-break, documented here as the
 * contract both sides follow:
 *
 * 1. tier match — only entries the catalog classifies as this tier
 *    (`m.tier === tier`; the Add Provider modal's per-provider explicit
 *    overrides are backend-side and, when present, win before inference);
 * 2. cheapest first — lowest `price_in + price_out`. An entry with no price
 *    data at all sorts after any priced entry (a priced model always beats
 *    an unpriced one); exact ties keep the earlier entry (stable — the
 *    catalog is a stable array);
 * 3. null — no entry carries this tier ("tier not available"); the UI
 *    shows the unresolved hint and the backend keeps the global default.
 */
export function resolveTierModel(
  tier: ModelTier,
  models: ReadonlyArray<Pick<ModelInfo, 'id' | 'tier' | 'price_in' | 'price_out'>>,
): string | null {
  const priced = (m: Pick<ModelInfo, 'price_in' | 'price_out'>): number =>
    (m.price_in ?? 0) + (m.price_out ?? 0)
  const hasPrice = (m: Pick<ModelInfo, 'price_in' | 'price_out'>): boolean =>
    m.price_in != null || m.price_out != null
  let best: Pick<ModelInfo, 'id' | 'price_in' | 'price_out'> | null = null
  for (const m of models) {
    if (m.tier !== tier) continue
    if (best == null) {
      best = m
      continue
    }
    // Cheapest wins; unpriced entries lose against any priced one; ties
    // keep the earlier (stable) entry.
    if (hasPrice(m) && !hasPrice(best)) best = m
    else if (hasPrice(m) === hasPrice(best) && priced(m) < priced(best)) best = m
  }
  return best?.id ?? null
}

/** i18n key for a preference's label (`chat.phaseTier.tier.*`). */
export function phaseTierLabelKey(pref: PhaseTierPref): string {
  return `chat.phaseTier.tier.${pref}`
}

// S3-1 (P-N10/P-N11) — "why is this model active" derivation, shared by the
// composer chip picker and the Header selector.
//
// The ENGINE is authoritative at query time (the precedence pin lives in
// `desktop/src/commands_chat.rs::resolve_client_config_for_session`):
//
//   session override (R2-1) > plan/act phase tier (R3-3) > global default
//
// This module mirrors that chain on the UI side so each picker row can wear
// the Claude-Code-style context label ("why does this row look like this")
// instead of the state being silent. Pure — vitest-covered. The tier half
// reuses the phaseTier module's resolution, which is the same mirror the
// PhaseTierSection dropdowns render with.

import type { ModelInfo } from '@/types'
import { effectivePhaseTier, resolveTierModel } from '@/lib/phaseTier'

/** Why-active label kinds, in engine precedence order. */
export type ModelWhy =
  | { kind: 'session' }
  | { kind: 'tier'; phase: 'plan' | 'act' }
  | { kind: 'profile'; profile: string }
  | { kind: 'global' }

/** Everything the derivation reads, pre-resolved by the caller (the picker
 *  surfaces own the invoke layer; this stays pure). */
export interface ModelWhyContext {
  /** The focused session's override (R2-1). `null` on surfaces without a
   *  session context (Header) — the session label is chat-only by the R2-1
   *  route-ownership convention. */
  override: { provider: string; model: string } | null
  approvalMode: string | null | undefined
  planTier: string | null | undefined
  actTier: string | null | undefined
  /** The global default model id (`status.model`). */
  globalModel: string | null | undefined
  /** The engine store's ACTIVE profile name (`status.active_profile`,
   *  S3-1 wire extension). `undefined`/`null`/`"default"` (the unset
   *  sentinel) render the plain "global default" label. */
  activeProfile: string | null | undefined
}

/** Which phase tier is steering right now, and the catalog model it
 *  resolves to. `null` when no tier applies (both `inherit`) or the tier
 *  has no catalog match — the backend keeps the global default either way. */
export function tierSteer(
  ctx: Pick<ModelWhyContext, 'approvalMode' | 'planTier' | 'actTier'>,
  models: ReadonlyArray<Pick<ModelInfo, 'id' | 'tier' | 'price_in' | 'price_out'>>,
): { phase: 'plan' | 'act'; modelId: string } | null {
  const phase: 'plan' | 'act' =
    (ctx.approvalMode ?? '').trim() === 'plan' ? 'plan' : 'act'
  const tier = effectivePhaseTier(
    ctx.approvalMode,
    ctx.planTier,
    ctx.actTier,
  )
  if (!tier) return null
  const modelId = resolveTierModel(tier, models)
  return modelId ? { phase, modelId } : null
}

/**
 * The why-active label for ONE picker row, or `null` when the row carries
 * no special state (every non-effective row). Matches the same id-or-name
 * contract the chip uses for its effective-model lookup, so the label can
 * never contradict the chip's own display.
 */
export function modelWhyFor(
  m: Pick<ModelInfo, 'id' | 'name'>,
  ctx: ModelWhyContext,
  models: ReadonlyArray<Pick<ModelInfo, 'id' | 'tier' | 'price_in' | 'price_out'>>,
): ModelWhy | null {
  // 1. Session override — beats everything (chat surfaces only).
  if (
    ctx.override &&
    (m.id === ctx.override.model || m.name === ctx.override.model)
  ) {
    return { kind: 'session' }
  }
  // 2. Phase tier steering — only labelled when it actually re-targets
  //    away from the global default (a tier resolving to the default model
  //    is not steering anything; labelling it would be noise).
  const steer = tierSteer(ctx, models)
  if (
    !ctx.override &&
    steer &&
    steer.modelId !== ctx.globalModel &&
    m.id === steer.modelId
  ) {
    return { kind: 'tier', phase: steer.phase }
  }
  // 3. The global default row — attributed to its pinning profile when one
  //    is explicitly active (the engine rebuilds the default from the
  //    active profile's target, so "profile pin" IS where the default
  //    lives). The `"default"` sentinel (pointer unset) stays generic.
  if (m.id === ctx.globalModel) {
    const profile = (ctx.activeProfile ?? '').trim()
    if (profile && profile !== 'default') return { kind: 'profile', profile }
    return { kind: 'global' }
  }
  return null
}

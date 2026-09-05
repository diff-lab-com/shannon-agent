// P1-3: execution-mode derivation for the header mode switcher.
//
// The switcher shows four tiers — 严格 / 平衡 / 宽松 / 自定义. The backend
// source of truth is the desktop config pair
// `active_permission_profile` (+ `approval_mode`, synced by the backend on
// activation). Derivation is a pure function so the switcher stays dumb and
// the mapping is unit-testable.

import type { DesktopConfig } from '@/types'

/** Frozen mode-switcher tiers. `custom` = a non-builtin active profile. */
export type ExecutionMode = 'strict' | 'balanced' | 'permissive' | 'custom'

export interface ExecutionModeState {
  mode: ExecutionMode
  /** Active profile id/name (custom name for the `custom` tier), or null. */
  profile: string | null
  /** The config's approval_mode, surfaced for the switcher label. */
  approvalMode: string | null
}

const BUILTIN_PROFILES = ['strict', 'balanced', 'permissive'] as const

export function isBuiltinProfile(name: string | null | undefined): boolean {
  return BUILTIN_PROFILES.includes((name ?? '').toLowerCase() as (typeof BUILTIN_PROFILES)[number])
}

/**
 * Derive the visible tier from the desktop config.
 *
 * - `strict` / `balanced` / `permissive` → the same-name tier.
 * - any other non-empty active profile → `custom` (name preserved).
 * - no active profile → `balanced` (the neutral default; the plain
 *   `approval_mode` drives the engine exactly as before profiles existed).
 */
export function deriveExecutionMode(config: DesktopConfig | null | undefined): ExecutionModeState {
  const profile = config?.active_permission_profile?.trim() ?? ''
  const approvalMode = config?.approval_mode ?? null
  if (profile === '') {
    return { mode: 'balanced', profile: null, approvalMode }
  }
  if (isBuiltinProfile(profile)) {
    return { mode: profile.toLowerCase() as ExecutionMode, profile: profile.toLowerCase(), approvalMode }
  }
  return { mode: 'custom', profile, approvalMode }
}

/** The activation payload per tier — the backend maps tier → approval_mode. */
export function tierProfileName(mode: Exclude<ExecutionMode, 'custom'>): string {
  return mode
}

// approvalModes — the single source for the approval tiers (v2, 4+3 model).
//
// docs/plans/2026-10-04-permission-mode-naming-design.md: the UI presents
// THREE autonomy ladder tiers — `ask` → `auto-edit` → `full-auto` — plus
// `plan` (Aurora 2026-10, 裁决 B1: a legal engine approval_mode that now
// rides IN the composer's four-stop segmented control), and three EXPERT
// modes reachable only from Settings ("advanced"): readonly / dontAsk (CI) /
// bypassPermissions. The TUI's Shift+Tab still cycles the original three
// ladder stops.
//
// Legacy stored values (suggest/confirm/auto/auto_edit/permissive/
// full_auto/full/strict/plan_ro/…) are normalized for display by
// `normalizeApprovalMode` and still parse engine-side; picking a tier always
// writes a canonical token.
import type { ApprovalMode } from '@/types'

export interface ApprovalModeOption {
  value: ApprovalMode
  /** react-intl id for the short label. */
  labelKey: string
  /** react-intl id for the one-line description. */
  descriptionKey: string
  /** Material symbol name for the option/pill icon. */
  icon: string
  /** Composer pill border tone (design-token color utility). */
  tone: string
  /** Set on fallback entries whose value has no translation — the pill
   *  renders this verbatim instead of a message id. */
  rawLabel?: string
}

/**
 * The autonomy ladder — the SAME three stops the TUI cycles with Shift+Tab,
 * ordered from most to least supervised. Each maps to a distinct engine
 * behavior, so switching always changes something:
 *
 *   ask        → reads run freely, everything else asks
 *   auto-edit  → file edits run automatically; commands still ask
 *   full-auto  → everything below critical risk runs automatically
 */
export const APPROVAL_MODES: readonly ApprovalModeOption[] = [
  {
    value: 'ask',
    labelKey: 'settings.general.approvalMode.ask.label',
    descriptionKey: 'settings.general.approvalMode.ask.description',
    icon: 'shield',
    tone: 'border-warning/50',
  },
  {
    value: 'auto-edit',
    labelKey: 'settings.general.approvalMode.autoEdit.label',
    descriptionKey: 'settings.general.approvalMode.autoEdit.description',
    icon: 'flash_auto',
    tone: 'border-warning/50',
  },
  {
    value: 'full-auto',
    labelKey: 'settings.general.approvalMode.full.label',
    descriptionKey: 'settings.general.approvalMode.full.description',
    icon: 'bolt',
    tone: 'border-error/50',
  },
]

/**
 * Expert modes (design §4.1): never offered in the composer pill; Settings
 * → General exposes them in the "advanced" picker. bypassPermissions shows
 * a warning tone — entry is additionally guardrailed engine-side (root
 * refusal / SHANNON_DISABLE_BYPASS).
 */
export const ADVANCED_MODES: readonly ApprovalModeOption[] = [
  {
    value: 'readonly',
    labelKey: 'settings.general.approvalMode.strict.label',
    descriptionKey: 'settings.general.approvalMode.strict.description',
    icon: 'lock',
    tone: 'border-success/50',
  },
  {
    value: 'dontAsk',
    labelKey: 'settings.general.approvalMode.unattended.label',
    descriptionKey: 'settings.general.approvalMode.unattended.description',
    icon: 'smart_toy',
    tone: 'border-outline-variant/50',
  },
  {
    value: 'bypassPermissions',
    labelKey: 'settings.general.approvalMode.bypass.label',
    descriptionKey: 'settings.general.approvalMode.bypass.description',
    icon: 'dangerous',
    tone: 'border-error/70',
  },
]

/** Legacy `approval_mode` values → the canonical token they mean today. */
const LEGACY_ALIASES: Readonly<Record<string, ApprovalMode>> = {
  suggest: 'ask',
  confirm: 'ask',
  balanced: 'ask',
  default: 'ask',
  auto: 'auto-edit',
  auto_edit: 'auto-edit',
  autoedit: 'auto-edit',
  permissive: 'auto-edit',
  full_auto: 'full-auto',
  fullauto: 'full-auto',
  full: 'full-auto',
  strict: 'readonly',
  plan_ro: 'readonly',
  planreadonly: 'readonly',
  dont_ask: 'dontAsk',
  bypass_permissions: 'bypassPermissions',
}

/** Canonicalize a stored config value for display/picking. */
export function normalizeApprovalMode(value: string): ApprovalMode {
  return LEGACY_ALIASES[value] ?? (value as ApprovalMode)
}

/**
 * The plan workflow tier (design §5). Aurora 2026-10 (裁决 B1): `plan` is a
 * LEGAL engine approval_mode (ApprovalMode::Plan — desktop parse_approval_mode
 * maps "plan" → Plan, unit-tested), so it takes its seat in the composer's
 * four-stop segmented control AND in the <1200px chip Select's option list.
 * Exported so both surfaces share one definition.
 */
export const PLAN_OPTION: ApprovalModeOption = {
  value: 'plan',
  labelKey: 'chat.input.mode.plan',
  descriptionKey: 'chat.input.mode.plan.desc',
  icon: 'route',
  tone: 'border-success/50',
}

// ─── 规则预设 (rule presets) — the permission-profile vocabulary ────────────
//
// 缓期批 3 convergence (docs/reviews/2026-10-09-cleanup-plan-and-deferred-
// research.md §执行模式收敛, M 级表): a true merge of the two controls is
// impossible — profile → approval_mode is not injective (strict AND balanced
// both mean `ask`), so any combined control would lose information. Backend
// 8803a519d settled the conflict: `activate_permission_profile` changes ONLY
// `active_permission_profile` and never touches `approval_mode`. The UI
// contract that follows: the composer owns the approval ladder above (the
// four-stop pill), while the presets below are presented as RULE PRESETS in
// Settings → 权限与安全 — switching a preset never changes the execution
// mode. Every user-visible preset label comes from THIS table, the same
// single-source discipline as the tiers (the composer switcher's competing
// `execMode.tier.*` vocabulary was retired with it).

export interface PermissionPresetOption {
  id: 'strict' | 'balanced' | 'permissive'
  /** react-intl id for the preset name (严格 / 平衡 / 宽松). */
  labelKey: string
  /** react-intl id for the one-line description of what the rules approve. */
  descriptionKey: string
  /** Material symbol name for the card icon. */
  icon: string
}

export const PERMISSION_PRESETS: readonly PermissionPresetOption[] = [
  {
    id: 'strict',
    labelKey: 'settings.permissions.builtin.name.strict',
    descriptionKey: 'settings.permissions.builtin.desc.strict',
    icon: 'shield_lock',
  },
  {
    id: 'balanced',
    labelKey: 'settings.permissions.builtin.name.balanced',
    descriptionKey: 'settings.permissions.builtin.desc.balanced',
    icon: 'balance',
  },
  {
    id: 'permissive',
    labelKey: 'settings.permissions.builtin.name.permissive',
    descriptionKey: 'settings.permissions.builtin.desc.permissive',
    icon: 'speed',
  },
]

/**
 * Resolve a builtin profile id to its preset vocabulary. Case-insensitive
 * (the backend normalizes builtin ids on activation); `undefined` for an
 * unknown id so future engine profiles fall back to the engine-provided
 * strings instead of a wrong label.
 */
export function permissionPresetOption(id: string): PermissionPresetOption | undefined {
  return PERMISSION_PRESETS.find((p) => p.id === id.toLowerCase())
}

/**
 * Resolve the config's `approval_mode` to a displayable option. Legacy
 * values normalize to their canonical tier; `plan` renders its own option;
 * a truly unknown NON-blank value keeps its raw string as the pill label —
 * the pill must never claim a safer (or different) mode than the engine is
 * actually in.
 */
export function approvalModeOption(value: string | null | undefined): ApprovalModeOption {
  if (value) {
    const normalized = normalizeApprovalMode(value)
    const ladder = APPROVAL_MODES.find(m => m.value === normalized)
    if (ladder) return ladder
    const advanced = ADVANCED_MODES.find(m => m.value === normalized)
    if (advanced) return advanced
    if (normalized === 'plan') return PLAN_OPTION
    // Unrecognized engine value: show it raw so the user can see (and
    // switch away from) a mode this UI doesn't manage.
    return {
      value: normalized,
      labelKey: 'chat.input.mode.label',
      descriptionKey: 'chat.input.mode.label',
      icon: 'tune',
      tone: 'border-outline-variant/50',
      rawLabel: value,
    }
  }
  // No mode configured: the engine default is auto-edit (K1).
  return APPROVAL_MODES[1]
}

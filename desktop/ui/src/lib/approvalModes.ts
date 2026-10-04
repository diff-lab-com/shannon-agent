// approvalModes — the single source for the approval tiers (v2, 4+3 model).
//
// docs/plans/2026-10-04-permission-mode-naming-design.md: the UI presents
// THREE autonomy ladder tiers — `ask` → `auto-edit` → `full-auto` — plus a
// separate composer plan toggle (plan is a workflow tier, never a ladder
// stop), and three EXPERT modes reachable only from Settings ("advanced"):
// readonly / dontAsk (CI) / bypassPermissions. This mirrors the TUI, whose
// Shift+Tab cycles the same three ladder stops.
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

/** The composer's plan toggle owns this workflow tier (design §5). */
const PLAN_OPTION: ApprovalModeOption = {
  value: 'plan',
  labelKey: 'chat.input.mode.plan',
  descriptionKey: 'chat.input.mode.plan.desc',
  icon: 'route',
  tone: 'border-success/50',
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

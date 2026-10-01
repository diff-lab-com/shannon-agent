// approvalModes — GB P2-4: the single source for the approval tiers.
//
// The composer's quick switcher and Settings → General's segmented control
// used to carry two DIFFERENT five-value tables over the same
// `approval_mode` config key (composer: readonly/plan/suggest/auto/full_auto;
// settings: suggest/confirm/plan/auto_edit/full_auto) — two controls that
// never agreed on what the tiers are. Both now render this one table, so a
// tier picked in either surface is the same tier the other one shows
// (「解锁式」语义: Settings keeps the full descriptions, the composer is the
// fast switcher; the storage — config `approval_mode` — never changed).
//
// Round-1 review (controller ruling R3): the table is FOUR tiers, named by
// REAL engine semantics. The old settings-page pair suggest/confirm was a
// no-op — the engine maps `"confirm" => ApprovalMode::Suggest`
// (desktop/src/commands.rs), so switching between the two changed nothing.
// The four tiers below are four DISTINCT engine behaviors:
//
//   strict     → `readonly`   (nothing is ever modified)
//   balanced   → `suggest`    (asks before every action)
//   permissive → `auto_edit`  (file edits auto-approved, commands ask;
//                               the engine's "auto" alias)
//   full       → `full_auto`  (approves everything except critical)
//
// Engine-only values outside the table — `confirm` (≡ suggest, kept for
// config compat and surfaced via the honest raw-value fallback), `plan`
// (own composer toggle), `auto` (alias of auto_edit), dont_ask /
// bypass_permissions / plan_ro — still render truthfully via
// `approvalModeOption`'s fallbacks; they are just not offered for
// quick-pick. Distinguishing confirm from suggest engine-side stays on the
// backlog (not a UI fix).

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
 * The four quick-switch tiers — deliberately the SAME values, labels and
 * descriptions Settings → General shows (one shared table), ordered from
 * most to least supervised. Each maps to a distinct engine approval
 * behavior, so switching always changes something.
 */
export const APPROVAL_MODES: readonly ApprovalModeOption[] = [
  {
    value: 'readonly',
    labelKey: 'settings.general.approvalMode.strict.label',
    descriptionKey: 'settings.general.approvalMode.strict.description',
    icon: 'lock',
    tone: 'border-success/50',
  },
  {
    value: 'suggest',
    labelKey: 'settings.general.approvalMode.balanced.label',
    descriptionKey: 'settings.general.approvalMode.balanced.description',
    icon: 'shield',
    tone: 'border-warning/50',
  },
  {
    value: 'auto_edit',
    labelKey: 'settings.general.approvalMode.permissive.label',
    descriptionKey: 'settings.general.approvalMode.permissive.description',
    icon: 'flash_auto',
    tone: 'border-warning/50',
  },
  {
    value: 'full_auto',
    labelKey: 'settings.general.approvalMode.full.label',
    descriptionKey: 'settings.general.approvalMode.full.description',
    icon: 'bolt',
    tone: 'border-error/50',
  },
]

/**
 * Honest display fallbacks for engine values the quick switcher doesn't
 * list. `auto` is the engine's ALIAS of auto_edit (from_str_ci maps both to
 * AutoEdit) — it shows the permissive tier's labels, which is the truth.
 * `plan` is a real distinct mode owned by the composer's plan toggle.
 */
const FALLBACK_OPTIONS: Readonly<Record<string, ApprovalModeOption>> = {
  auto: APPROVAL_MODES[2],
  plan: {
    value: 'plan',
    labelKey: 'chat.input.mode.plan',
    descriptionKey: 'chat.input.mode.plan.desc',
    icon: 'route',
    tone: 'border-success/50',
  },
}

/**
 * Resolve the config's `approval_mode` to a displayable option. Unknown or
 * blank values fall back to the Balanced tier for PICKING, but an unknown
 * NON-blank value keeps its raw string as the pill label — the pill must
 * never claim a safer (or different) mode than the engine is actually in.
 * `confirm` lands here by design (R3): it IS suggest engine-side, and the
 * raw readout says so instead of pretending there are two tiers.
 */
export function approvalModeOption(value: string | null | undefined): ApprovalModeOption {
  if (value) {
    const listed = APPROVAL_MODES.find(m => m.value === value)
    if (listed) return listed
    const fallback = FALLBACK_OPTIONS[value]
    if (fallback) return fallback
    // Engine-only alias (confirm / dont_ask / bypass_permissions / plan_ro
    // …): show the raw value, described as the permission mode, so the user
    // can see (and switch away from) a mode this UI doesn't manage.
    return {
      value: value as ApprovalMode,
      labelKey: 'chat.input.mode.label',
      descriptionKey: 'chat.input.mode.label',
      icon: 'tune',
      tone: 'border-outline-variant/50',
      rawLabel: value,
    }
  }
  // No mode configured: the engine treats unparseable as Suggest — surface
  // Balanced, the tier the composer has always shown for a blank value.
  return APPROVAL_MODES[1]
}

// approvalModes — GB P2-4: the single source for the five approval tiers.
//
// The composer's quick switcher and Settings → General's segmented control
// used to carry two DIFFERENT five-value tables over the same
// `approval_mode` config key (composer: readonly/plan/suggest/auto/full_auto;
// settings: suggest/confirm/plan/auto_edit/full_auto) — two controls that
// never agreed on what the five tiers are. Both now render this one table,
// so a tier picked in either surface is the same tier the other one shows
// (「解锁式」语义: Settings keeps the full descriptions, the composer is the
// fast switcher; the storage — config `approval_mode` — never changed).
//
// The engine still accepts the wider alias set (crates/shannon-engine
// permissions.rs `ApprovalMode::from_str_ci`; desktop/src/commands.rs maps
// legacy names like "confirm" → Suggest), and a mode that was set OUTSIDE
// these five (e.g. `readonly` via a permission profile, or an engine-only
// alias) still renders honestly in the composer via `approvalModeOption`'s
// fallbacks — it is just not offered for quick-pick, per the five-tier spec.

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
 * The five quick-switch tiers — deliberately the SAME values, labels and
 * descriptions Settings → General has always shown (「General 页那 5 档」).
 * Order runs from most to least supervised.
 */
export const APPROVAL_MODES: readonly ApprovalModeOption[] = [
  {
    value: 'suggest',
    labelKey: 'settings.general.approvalMode.suggest.label',
    descriptionKey: 'settings.general.approvalMode.suggest.description',
    icon: 'shield',
    tone: 'border-warning/50',
  },
  {
    value: 'confirm',
    labelKey: 'settings.general.approvalMode.confirm.label',
    descriptionKey: 'settings.general.approvalMode.confirm.description',
    icon: 'verified_user',
    tone: 'border-warning/50',
  },
  {
    value: 'plan',
    labelKey: 'settings.general.approvalMode.plan.label',
    descriptionKey: 'settings.general.approvalMode.plan.description',
    icon: 'description',
    tone: 'border-success/50',
  },
  {
    value: 'auto_edit',
    labelKey: 'settings.general.approvalMode.autoEdit.label',
    descriptionKey: 'settings.general.approvalMode.autoEdit.description',
    icon: 'flash_auto',
    tone: 'border-warning/50',
  },
  {
    value: 'full_auto',
    labelKey: 'settings.general.approvalMode.fullAuto.label',
    descriptionKey: 'settings.general.approvalMode.fullAuto.description',
    icon: 'bolt',
    tone: 'border-error/50',
  },
]

/**
 * Honest display fallbacks for engine modes the quick switcher doesn't list.
 * They reuse the composer's existing translations so a user whose mode was
 * set elsewhere (permission profiles, CLI /mode) still reads a real label.
 */
const FALLBACK_OPTIONS: Readonly<Record<string, ApprovalModeOption>> = {
  readonly: {
    value: 'readonly',
    labelKey: 'chat.input.mode.readonly',
    descriptionKey: 'chat.input.mode.readonly.desc',
    icon: 'lock',
    tone: 'border-success/50',
  },
  auto: {
    value: 'auto',
    labelKey: 'chat.input.mode.auto',
    descriptionKey: 'chat.input.mode.auto.desc',
    icon: 'flash_auto',
    tone: 'border-warning/50',
  },
}

/**
 * Resolve the config's `approval_mode` to a displayable option. Unknown or
 * blank values fall back to the Suggest tier for PICKING, but an unknown
 * NON-blank value keeps its raw string as the pill label — the pill must
 * never claim a safer mode than the engine is actually in.
 */
export function approvalModeOption(value: string | null | undefined): ApprovalModeOption {
  if (value) {
    const listed = APPROVAL_MODES.find(m => m.value === value)
    if (listed) return listed
    const fallback = FALLBACK_OPTIONS[value]
    if (fallback) return fallback
    // Engine-only alias (dont_ask / bypass_permissions / plan_ro …): show
    // the raw value, described as the permission mode, so the user can see
    // (and switch away from) a mode this UI doesn't manage.
    return {
      value: value as ApprovalMode,
      labelKey: 'chat.input.mode.label',
      descriptionKey: 'chat.input.mode.label',
      icon: 'tune',
      tone: 'border-outline-variant/50',
      rawLabel: value,
    }
  }
  // No mode configured: the engine default (desktop config seeds "confirm",
  // the engine treats unparseable as Suggest) — surface Suggest, the tier
  // the composer has always shown for a blank value.
  return APPROVAL_MODES[0]
}

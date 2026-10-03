import { useState } from 'react'
import { Menu } from '@base-ui/react/menu'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import {
  normalizePhaseTierPref,
  phaseTierConfigValue,
  phaseTierLabelKey,
  type PhaseTierPref,
} from '@/lib/phaseTier'

/**
 * R3-3 — compact Plan/Act tier pair for the chat header (Cline-style
 * plan/act models mapped onto Shannon's tier system).
 *
 * Each phase (规划 planning / 执行 execution) can be `inherit` (default —
 * no override), `fast`, `standard`, or `pro`. Values persist through the
 * GLOBAL desktop config (`configure('plan_tier' | 'act_tier')`), not per
 * session; precedence at query time is
 * **session override (R2-1) > phase tier > global default** — the backend
 * (`desktop/src/commands_chat.rs`) owns that chain, this control only
 * renders and writes the preferences.
 *
 * The popover is a **Base UI Menu** (w4 refactor/header-menus-baseui): Base
 * UI owns the body-level Portal, trigger anchoring (auto re-anchor on
 * sidebar resize) and the keyboard contract — supersedes the w3 hand-rolled
 * portal that escaped the header's `glass-surface fixed z-header`
 * `contain: paint` stacking context. The z-modal token class rides the
 * positioner (token scale: header 40 < modal 50), and the radios keep their
 * `radiogroup`/`radio` ARIA presentation (overridden per part, Base UI's
 * roving-focus/typeahead machinery is untouched) — the e2e anchor drives
 * them with REAL pointer clicks.
 *
 * Deliberately stays a two-radio-group popover (not two Selects): both
 * phases stay visible and pickable without reopening — a pick does NOT
 * close the popover (`closeOnClick={false}`), matching the pre-Base-UI
 * behavior and the Cline-style plan/act workflow.
 */

const PREFS: readonly PhaseTierPref[] = ['inherit', 'fast', 'standard', 'pro'] as const

function PhaseGroup({
  heading,
  value,
  onPick,
  t,
}: {
  heading: string
  value: PhaseTierPref
  onPick: (pref: PhaseTierPref) => void
  t: (id: string) => string
}) {
  return (
    // Controlled from the config so aria-checked and the visual state share
    // one source of truth (the config only flips after the backend write).
    <Menu.RadioGroup
      role="radiogroup"
      aria-label={heading}
      value={value}
      onValueChange={(pref) => onPick(pref as PhaseTierPref)}
      className="px-md pt-sm pb-xs"
    >
      <div className="font-label-sm text-label-sm text-on-surface-variant font-bold uppercase tracking-wider mb-xs px-md">
        {heading}
      </div>
      <div className="flex gap-xs">
        {PREFS.map((pref) => (
          <Menu.RadioItem
            key={pref}
            role="radio"
            value={pref}
            label={t(phaseTierLabelKey(pref))}
            closeOnClick={false}
            title={t(phaseTierLabelKey(pref))}
            className={cn(
              'flex-1 justify-center px-sm py-xs h-auto rounded-lg font-label-sm cursor-pointer outline-none transition-colors',
              value === pref
                ? 'bg-primary-container text-on-primary-container font-bold'
                : 'text-on-surface-variant',
              'data-[highlighted]:bg-primary/10',
            )}
          >
            {t(phaseTierLabelKey(pref))}
          </Menu.RadioItem>
        ))}
      </div>
    </Menu.RadioGroup>
  )
}

export function PhaseTierSwitcher() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const { config, refreshConfig } = useCatalog()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState<PhaseTierPref | null>(null)

  // Lenient readers — legacy/junk stored values render as 继承 (inherit),
  // mirroring the backend's normalize (a bad value never misleads).
  const planPref = normalizePhaseTierPref(config?.plan_tier)
  const actPref = normalizePhaseTierPref(config?.act_tier)

  const handlePick = async (phase: 'plan' | 'act', pref: PhaseTierPref) => {
    if (busy != null) return
    const current = phase === 'plan' ? planPref : actPref
    if (current === pref) return
    setBusy(pref)
    try {
      await api.configure({ key: phase === 'plan' ? 'plan_tier' : 'act_tier', value: phaseTierConfigValue(pref) })
      await refreshConfig()
      toast.success(
        t(phase === 'plan' ? 'chat.phaseTier.toast.plan' : 'chat.phaseTier.toast.act', {
          tier: t(phaseTierLabelKey(pref)),
        }),
      )
    } catch (e) {
      toastError(t('chat.phaseTier.failed'), e)
    } finally {
      setBusy(null)
    }
  }

  const ariaLabel = t('chat.phaseTier.toggle.aria', {
    plan: t(phaseTierLabelKey(planPref)),
    act: t(phaseTierLabelKey(actPref)),
  })

  return (
    <Menu.Root open={open} onOpenChange={setOpen} modal={false}>
      <Menu.Trigger
        render={
          <Button
            variant="ghost"
            aria-label={ariaLabel}
            title={ariaLabel}
            data-testid="phase-tier-switcher"
            className="flex items-center gap-xs px-md py-sm rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-all"
          >
            <span className="material-symbols-outlined icon-md" aria-hidden="true">alt_route</span>
            <span className="font-label-sm text-label-sm whitespace-nowrap tabular-nums">
              {t('chat.phaseTier.planShort')}
              {' '}
              <span className="font-bold text-on-surface">{t(phaseTierLabelKey(planPref))}</span>
              {' · '}
              {t('chat.phaseTier.actShort')}
              {' '}
              <span className="font-bold text-on-surface">{t(phaseTierLabelKey(actPref))}</span>
            </span>
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">expand_more</span>
          </Button>
        }
      />
      <Menu.Portal>
        {/* z-modal rides the POSITIONER (see ExecutionModeSwitcher for the
            stacking-context rationale and token scale). */}
        <Menu.Positioner align="end" sideOffset={8} className="isolate z-modal">
          {/* role="dialog" + dedicated label as before; aria-labelledby is
              cleared because Base UI would label the popup from the trigger.
              data-testid="phase-tier-menu" anchors the e2e. */}
          <Menu.Popup
            role="dialog"
            aria-labelledby={undefined}
            aria-label={t('chat.phaseTier.title')}
            data-testid="phase-tier-menu"
            className="glass-overlay animate-panel-in w-[320px] rounded-xl py-sm outline-none"
          >
            <div className="px-md pt-xs pb-sm">
              <div className="font-headline-sm text-on-surface font-bold">{t('chat.phaseTier.title')}</div>
              <div className="text-label-sm text-on-surface-variant">{t('chat.phaseTier.hint')}</div>
            </div>
            <PhaseGroup
              heading={t('chat.phaseTier.plan')}
              value={planPref}
              onPick={(pref) => { void handlePick('plan', pref) }}
              t={t}
            />
            <PhaseGroup
              heading={t('chat.phaseTier.act')}
              value={actPref}
              onPick={(pref) => { void handlePick('act', pref) }}
              t={t}
            />
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  )
}

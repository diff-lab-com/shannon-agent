import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
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
 * Deliberately NOT a Base UI Select: the hand-rolled popover (same pattern
 * as the neighboring ExecutionModeSwitcher) keeps the control jsdom-stable
 * for tests and consistent with the header's existing controls.
 *
 * The popover is a **body-level portal** (w3 fix/header-dropdown-hit-test):
 * inline it sat inside the header's `glass-surface fixed z-header` element,
 * whose `contain: paint` (plus backdrop-filter and the z-index + fixed
 * pair) forms a stacking context AND clips descendants to the 64px bar —
 * the `z-modal` dropdown was trapped inside and its body painted/hit-tested
 * away under the chat message area, so real pointer clicks never reached
 * the radios. Portalled to body with the `z-modal` token class the popover
 * paints above the chat main area (token scale: header 40 < modal 50)
 * without leaving the design-token system.
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
    <div
      role="radiogroup"
      aria-label={heading}
      className="px-md pt-sm pb-xs"
    >
      <div className="font-label-sm text-label-sm text-on-surface-variant font-bold uppercase tracking-wider mb-xs px-md">
        {heading}
      </div>
      <div className="flex gap-xs">
        {PREFS.map((pref) => {
          const selected = value === pref
          return (
            <Button
              key={pref}
              variant="ghost"
              role="radio"
              aria-checked={selected}
              title={t(phaseTierLabelKey(pref))}
              className={cn(
                'flex-1 justify-center px-sm py-xs h-auto rounded-lg font-label-sm cursor-pointer transition-colors',
                selected
                  ? 'bg-primary-container text-on-primary-container font-bold'
                  : 'text-on-surface-variant hover:bg-primary/5',
              )}
              onClick={() => onPick(pref)}
            >
              {t(phaseTierLabelKey(pref))}
            </Button>
          )
        })}
      </div>
    </div>
  )
}

export function PhaseTierSwitcher() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const { config, refreshConfig } = useCatalog()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState<PhaseTierPref | null>(null)
  const ref = useRef<HTMLDivElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  // Viewport-anchored position for the portalled popover, computed from the
  // trigger box on open (and on window resize while open — the header is
  // fixed, so page scroll never moves the anchor). jsdom reports zero rects
  // and the popover still mounts; only pixel alignment depends on this.
  const [menuPos, setMenuPos] = useState<{ top: number; right: number } | null>(null)

  // Lenient readers — legacy/junk stored values render as 继承 (inherit),
  // mirroring the backend's normalize (a bad value never misleads).
  const planPref = normalizePhaseTierPref(config?.plan_tier)
  const actPref = normalizePhaseTierPref(config?.act_tier)

  useEffect(() => {
    if (!open) return
    // Portal means the popover is no longer inside `ref` — treat presses
    // inside the popover itself as "inside" or every pick's mousedown would
    // close it before the click lands.
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (ref.current?.contains(target) || menuRef.current?.contains(target)) return
      setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  // Keep the portalled popover anchored under the trigger across resizes.
  // Layout effect: the popover's FIRST painted frame must already sit at
  // the trigger — a passive effect would paint one frame at the corner.
  useLayoutEffect(() => {
    if (!open) return
    const updatePos = () => {
      const rect = ref.current?.getBoundingClientRect()
      if (rect) setMenuPos({ top: rect.bottom, right: window.innerWidth - rect.right })
    }
    updatePos()
    window.addEventListener('resize', updatePos)
    return () => window.removeEventListener('resize', updatePos)
  }, [open])

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
    <div className="relative" ref={ref}>
      <Button
        variant="ghost"
        aria-label={ariaLabel}
        title={ariaLabel}
        aria-haspopup="true"
        aria-expanded={open}
        data-testid="phase-tier-switcher"
        className="flex items-center gap-xs px-md py-sm rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-all"
        onClick={() => setOpen((o) => !o)}
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
      {open && createPortal(
        <div
          ref={menuRef}
          className="glass-overlay animate-panel-in fixed mt-sm w-[320px] rounded-xl z-modal py-sm"
          style={{ top: menuPos?.top ?? 0, right: menuPos?.right ?? 0 }}
          role="dialog"
          aria-label={t('chat.phaseTier.title')}
          data-testid="phase-tier-menu"
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
        </div>,
        document.body,
      )}
    </div>
  )
}

import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useIntl, type PrimitiveType } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import {
  deriveExecutionMode,
  tierProfileName,
  type ExecutionMode,
} from '@/lib/executionMode'

const TIERS: Array<{ id: Exclude<ExecutionMode, 'custom'>; icon: string }> = [
  { id: 'strict', icon: 'shield_lock' },
  { id: 'balanced', icon: 'balance' },
  { id: 'permissive', icon: 'speed' },
]

const TIER_LABEL_KEY: Record<string, string> = {
  strict: 'execMode.tier.strict',
  balanced: 'execMode.tier.balanced',
  permissive: 'execMode.tier.permissive',
  custom: 'execMode.tier.custom',
}

/**
 * P1-3: compact execution-mode switcher for the chat header.
 *
 * Four tiers — 严格 / 平衡 / 宽松 / 自定义 — backed by
 * `activate_permission_profile`. Switching persists the active profile (and
 * its synced approval_mode) on the backend and takes effect on the next
 * turn. The `custom` row is a navigation shortcut to the Profiles settings
 * page rather than an activation target.
 *
 * The menu is a **body-level portal** (w3 fix/header-dropdown-hit-test):
 * inline it used to live inside the header's `<header class="glass-surface
 * fixed z-header">`, whose `contain: paint` (plus backdrop-filter and the
 * z-header + fixed pair) forms a stacking context AND clips descendants to
 * the 64px bar — the dropdown's `z-modal` was trapped inside and its body
 * painted/hit-tested away under the chat message area, so real pointer
 * clicks never reached the options (specs had to fake clicks with
 * dispatchEvent). Portalled to body with the `z-modal` token class the
 * menu paints above the chat main area (token scale: header 40 < modal 50)
 * without leaving the design-token system.
 */
export function ExecutionModeSwitcher() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const navigate = useNavigate()
  const { config, refreshConfig } = useCatalog()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [focus, setFocus] = useState(-1)
  const ref = useRef<HTMLDivElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([])
  // Viewport-anchored position for the portalled menu, computed from the
  // trigger box on open (and on window resize while open — the header is
  // fixed, so page scroll never moves the anchor). jsdom reports zero rects
  // and the menu still mounts; only pixel alignment depends on this.
  const [menuPos, setMenuPos] = useState<{ top: number; right: number } | null>(null)

  const state = deriveExecutionMode(config)
  const options: Array<{ key: string; tier: ExecutionMode; profile: string | null }> = [
    ...TIERS.map((tier) => ({ key: tier.id, tier: tier.id as ExecutionMode, profile: tierProfileName(tier.id) })),
    { key: 'custom', tier: 'custom' as ExecutionMode, profile: state.mode === 'custom' ? state.profile : null },
  ]

  useEffect(() => {
    if (!open) return
    // Portal means the menu is no longer inside `ref` — treat presses inside
    // the menu itself as "inside" or every pick's mousedown would close the
    // menu before the click lands.
    const handleClick = (e: MouseEvent) => {
      const target = e.target as Node
      if (ref.current?.contains(target) || menuRef.current?.contains(target)) return
      setOpen(false)
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  // Keep the portalled menu anchored under the trigger across resizes.
  // Layout effect: the menu's FIRST painted frame must already sit at the
  // trigger — a passive effect would paint one frame at the viewport corner.
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

  // P2-10: opening the menu moves focus to the selected item (listbox
  // roving-focus pattern) — previously focus stayed on the trigger, so the
  // menu's key handling was unreachable dead code.
  useEffect(() => {
    if (!open) return
    const selectedIdx = options.findIndex((o) => o.tier === state.mode)
    const idx = selectedIdx >= 0 ? selectedIdx : 0
    setFocus(idx)
    optionRefs.current[idx]?.focus()
    // options/state.mode are stable while the menu is open; re-running on
    // every render would yank focus back after hover/arrow navigation.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open])

  /** Close and put focus back on the trigger (Escape / activation). */
  const closeAndRestoreFocus = () => {
    setOpen(false)
    ref.current?.querySelector<HTMLButtonElement>(':scope > button')?.focus()
  }

  const handlePick = async (option: (typeof options)[number]) => {
    if (busy) return
    // 自定义 → the Profiles settings page (enable/edit lives there).
    if (option.tier === 'custom') {
      closeAndRestoreFocus()
      navigate('/settings/permissions')
      return
    }
    setBusy(true)
    try {
      await api.activatePermissionProfile(option.profile)
      await refreshConfig()
      closeAndRestoreFocus()
      toast.success(t('execMode.toast.switched', { tier: t(TIER_LABEL_KEY[option.tier]) }))
    } catch (e) {
      toastError(t('execMode.toast.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  const moveFocus = (delta: number) => {
    const next = (focus + delta + options.length) % options.length
    setFocus(next)
    optionRefs.current[next]?.focus()
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      moveFocus(1)
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      moveFocus(-1)
    } else if (e.key === 'Enter') {
      // preventDefault also suppresses the focused option's native click,
      // so Enter activates exactly once.
      e.preventDefault()
      if (focus >= 0) void handlePick(options[focus])
    } else if (e.key === 'Escape') {
      e.preventDefault()
      closeAndRestoreFocus()
    }
  }

  const currentLabel =
    state.mode === 'custom'
      ? state.profile
        ? t('execMode.tier.customNamed', { name: state.profile })
        : t(TIER_LABEL_KEY.custom)
      : t(TIER_LABEL_KEY[state.mode])

  return (
    <div className="relative" ref={ref}>
      <Button
        variant="ghost"
        data-testid="execution-mode-switcher"
        aria-label={t('execMode.toggle.aria', { tier: currentLabel })}
        title={t('execMode.toggle.title')}
        aria-haspopup="listbox"
        aria-expanded={open}
        className="flex items-center gap-xs px-md py-sm rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-all"
        onClick={() => {
          setOpen((o) => !o)
          setFocus(-1)
        }}
      >
        <span className="material-symbols-outlined icon-md" aria-hidden="true">
          tune
        </span>
        <span className="font-label-sm text-label-sm whitespace-nowrap max-w-[110px] truncate">
          {currentLabel}
        </span>
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
          expand_more
        </span>
      </Button>
      {open && createPortal(
        <div
          ref={menuRef}
          className="glass-overlay animate-panel-in fixed mt-sm w-[240px] rounded-xl z-modal py-sm"
          style={{ top: menuPos?.top ?? 0, right: menuPos?.right ?? 0 }}
          role="listbox"
          aria-label={t('execMode.menu.aria')}
          onKeyDown={handleKeyDown}
        >
          {options.map((option, i) => {
            const selected = state.mode === option.tier
            const label =
              option.tier === 'custom'
                ? option.profile
                  ? t('execMode.tier.customNamed', { name: option.profile })
                  : t(TIER_LABEL_KEY.custom)
                : t(TIER_LABEL_KEY[option.tier])
            return (
              <Button
                key={option.key}
                ref={(el) => { optionRefs.current[i] = el }}
                variant="ghost"
                role="option"
                aria-selected={selected}
                className={cn(
                  'w-full justify-between px-md py-sm h-auto rounded-none',
                  i === focus
                    ? 'bg-primary-container text-on-primary-container'
                    : selected
                      ? 'text-primary font-bold'
                      : 'text-on-surface hover:bg-primary/5',
                )}
                onClick={() => void handlePick(option)}
                onMouseEnter={() => setFocus(i)}
              >
                <span className="font-label-md truncate">{label}</span>
                {selected && (
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                    check
                  </span>
                )}
              </Button>
            )
          })}
          <div className="px-md pt-xs pb-sm text-label-sm text-on-surface-variant" aria-hidden="true">
            {t('execMode.menu.hint')}
          </div>
        </div>,
        document.body,
      )}
    </div>
  )
}

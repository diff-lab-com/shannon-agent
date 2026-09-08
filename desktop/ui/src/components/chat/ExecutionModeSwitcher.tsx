import { useEffect, useRef, useState } from 'react'
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

  const state = deriveExecutionMode(config)
  const options: Array<{ key: string; tier: ExecutionMode; profile: string | null }> = [
    ...TIERS.map((tier) => ({ key: tier.id, tier: tier.id as ExecutionMode, profile: tierProfileName(tier.id) })),
    { key: 'custom', tier: 'custom' as ExecutionMode, profile: state.mode === 'custom' ? state.profile : null },
  ]

  useEffect(() => {
    if (!open) return
    const handleClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', handleClick)
    return () => document.removeEventListener('mousedown', handleClick)
  }, [open])

  const handlePick = async (option: (typeof options)[number]) => {
    if (busy) return
    // 自定义 → the Profiles settings page (enable/edit lives there).
    if (option.tier === 'custom') {
      setOpen(false)
      navigate('/settings/permissions')
      return
    }
    setBusy(true)
    try {
      await api.activatePermissionProfile(option.profile)
      await refreshConfig()
      setOpen(false)
      toast.success(t('execMode.toast.switched', { tier: t(TIER_LABEL_KEY[option.tier]) }))
    } catch (e) {
      toastError(t('execMode.toast.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setFocus((f) => Math.min(f + 1, options.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setFocus((f) => Math.max(f - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (focus >= 0) void handlePick(options[focus])
    } else if (e.key === 'Escape') {
      setOpen(false)
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
        <span className="font-label-sm text-[12px] whitespace-nowrap max-w-[110px] truncate">
          {currentLabel}
        </span>
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
          expand_more
        </span>
      </Button>
      {open && (
        <div
          className="absolute right-0 top-full mt-sm w-[240px] bg-surface-container-lowest/95 backdrop-blur-lg rounded-xl border border-outline-variant/20 shadow-xl z-modal py-sm"
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
                variant="ghost"
                role="option"
                aria-selected={selected}
                className={cn(
                  'w-full justify-between px-md py-sm h-auto rounded-none',
                  i === focus
                    ? 'bg-primary/10 text-primary'
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
        </div>
      )}
    </div>
  )
}

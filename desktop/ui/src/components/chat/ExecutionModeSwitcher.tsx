import { useState } from 'react'
import { Menu } from '@base-ui/react/menu'
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
 * The menu is a **Base UI Menu** (w4 refactor/header-menus-baseui): Base UI
 * owns the body-level Portal, the trigger anchoring and the full keyboard
 * contract (arrow/typeahead navigation, Escape, focus-out close, focus
 * return to the trigger). That supersedes the w3 hand-rolled portal: the
 * menu used to live inside the header's `glass-surface fixed z-header`
 * element, whose `contain: paint` (plus backdrop-filter and the z-header +
 * fixed pair) forms a stacking context AND clips descendants to the 64px
 * bar — the dropdown's `z-modal` was trapped inside and its body
 * painted/hit-tested away under the chat message area, so real pointer
 * clicks never reached the options (specs had to fake clicks with
 * dispatchEvent). Base UI's Portal + `z-modal` positioner token class the
 * menu paints above the chat main area (token scale: header 40 < modal 50)
 * and re-anchors automatically when the sidebar resizes.
 *
 * The `role="listbox"`/`option` presentation is deliberate (the pre-Base-UI
 * contract this surface shipped with): `aria-haspopup="menu"`-style
 * `menuitem` roles would read as a command menu, while these rows are a
 * single-value selection — Base UI lets the ARIA roles be overridden per
 * part without losing its roving-focus/typeahead machinery.
 */
export function ExecutionModeSwitcher() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) =>
    intl.formatMessage({ id }, values)
  const navigate = useNavigate()
  const { config, refreshConfig } = useCatalog()
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)

  const state = deriveExecutionMode(config)
  const options: Array<{ key: string; tier: ExecutionMode; profile: string | null }> = [
    ...TIERS.map((tier) => ({ key: tier.id, tier: tier.id as ExecutionMode, profile: tierProfileName(tier.id) })),
    { key: 'custom', tier: 'custom' as ExecutionMode, profile: state.mode === 'custom' ? state.profile : null },
  ]

  /** Close with Base UI's focus-return-to-trigger semantics. */
  const close = () => setOpen(false)

  const handlePick = async (option: (typeof options)[number]) => {
    if (busy) return
    // 自定义 → the Profiles settings page (enable/edit lives there).
    if (option.tier === 'custom') {
      close()
      navigate('/settings/permissions')
      return
    }
    setBusy(true)
    try {
      await api.activatePermissionProfile(option.profile)
      await refreshConfig()
      close()
      toast.success(t('execMode.toast.switched', { tier: t(TIER_LABEL_KEY[option.tier]) }))
    } catch (e) {
      toastError(t('execMode.toast.failed'), e)
    } finally {
      setBusy(false)
    }
  }

  const currentLabel =
    state.mode === 'custom'
      ? state.profile
        ? t('execMode.tier.customNamed', { name: state.profile })
        : t(TIER_LABEL_KEY.custom)
      : t(TIER_LABEL_KEY[state.mode])

  return (
    // highlightItemOnHover={false} keeps CSS :hover (bg-primary/5, the
    // pre-Base-UI hover paint) separate from the keyboard's data-highlighted
    // (primary-container) — zero visual drift, per fix round 1.
    <Menu.Root open={open} onOpenChange={setOpen} modal={false} highlightItemOnHover={false}>
      <Menu.Trigger
        render={
          <Button
            variant="ghost"
            data-testid="execution-mode-switcher"
            aria-label={t('execMode.toggle.aria', { tier: currentLabel })}
            title={t('execMode.toggle.title')}
            className="flex items-center gap-xs px-md py-sm rounded-lg hover:bg-surface-container-low text-on-surface-variant hover:text-primary transition-all"
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
        }
      />
      <Menu.Portal>
        {/* z-modal rides the POSITIONER (as ui/select.tsx): floating-ui's
            transform makes the positioner the portal subtree's stacking
            context, so a z-index on the popup inside it could never win
            against the header (40) or the message area overlays. Token scale
            keeps the menu below the permission scrim/dialog (z-flash). */}
        <Menu.Positioner align="end" sideOffset={8} className="isolate z-modal">
          {/* role="listbox" replaces Base UI's default "menu" (see component
              doc); aria-labelledby is explicitly cleared because Base UI
              labels the popup from the trigger — the e2e + AT contract is
              the dedicated "Execution mode options" name. */}
          <Menu.Popup
            role="listbox"
            aria-labelledby={undefined}
            aria-label={t('execMode.menu.aria')}
            aria-orientation="vertical"
            className="glass-overlay animate-panel-in w-[240px] rounded-xl py-sm outline-none"
          >
            {options.map((option) => {
              const selected = state.mode === option.tier
              const label =
                option.tier === 'custom'
                  ? option.profile
                    ? t('execMode.tier.customNamed', { name: option.profile })
                    : t(TIER_LABEL_KEY.custom)
                  : t(TIER_LABEL_KEY[option.tier])
              return (
                <Menu.Item
                  key={option.key}
                  role="option"
                  aria-selected={selected}
                  label={label}
                  // Keep the pre-Base-UI error semantics: a failed
                  // activation leaves the menu open for a retry, so items
                  // opt out of the automatic close-on-press.
                  closeOnClick={false}
                  className={cn(
                    'flex w-full cursor-pointer items-center justify-between gap-sm px-md py-sm text-left font-label-md outline-none transition-colors',
                    selected
                      ? 'text-primary font-bold'
                      : 'text-on-surface hover:bg-primary/5',
                    'data-[highlighted]:bg-primary-container data-[highlighted]:text-on-primary-container',
                  )}
                  onClick={() => void handlePick(option)}
                >
                  <span className="truncate">{label}</span>
                  {selected && (
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                      check
                    </span>
                  )}
                </Menu.Item>
              )
            })}
            <div className="px-md pt-xs pb-sm text-label-sm text-on-surface-variant" aria-hidden="true">
              {t('execMode.menu.hint')}
            </div>
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  )
}

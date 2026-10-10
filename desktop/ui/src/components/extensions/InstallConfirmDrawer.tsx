// InstallConfirmDrawer — the Dangerous-install confirmation drawer.
//
// Rendered by `useInstallGate` when one of the five gated install commands
// rejects with the structured `confirmation_required` payload (backend:
// extensions_commands.rs). It shows the scan matches — pattern + category +
// matched substring, i.e. WHY the user is being stopped — and requires the
// entry name typed back EXACTLY before the confirm button enables.
//
// Honesty rules (2026-10-10 gate design, D-A):
// - The comparison is case-sensitive and NOT trimmed client-side. The
//   backend trims before comparing, but the honest gesture is exact typing.
// - A live match/mismatch hint explains the state while typing.
// - Nothing has been installed when this drawer shows: the backend refused
//   before writing anything. Cancel is a real no-op.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import {
  SidePanel,
  SidePanelBody,
  SidePanelCloseButton,
  SidePanelHeader,
  SidePanelTitle,
} from '@/components/ui/side-panel'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import type { ConfirmationRequiredPayload } from '@/lib/installGate'

export interface InstallConfirmDrawerProps {
  payload: ConfirmationRequiredPayload
  /** Called with the typed name once the user confirms (exact match only). */
  onConfirm: (typedName: string) => void
  /** Called on cancel / close — nothing was installed. */
  onCancel: () => void
}

export default function InstallConfirmDrawer({
  payload,
  onConfirm,
  onCancel,
}: InstallConfirmDrawerProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [typed, setTyped] = useState('')
  // Exact, case-sensitive, untrimmed — mirrors what the backend accepts.
  const exactMatch = typed === payload.name
  const touched = typed.length > 0

  return (
    <SidePanel
      open
      onClose={onCancel}
      ariaLabel={t('extensions.installGate.ariaLabel', { name: payload.name })}
      width="460px"
    >
      <SidePanelHeader className="items-start">
        <div className="flex min-w-0 flex-1 items-start gap-sm">
          <span className="material-symbols-outlined icon-md text-error mt-[2px]" aria-hidden="true">warning</span>
          <SidePanelTitle>{t('extensions.installGate.title', { name: payload.name })}</SidePanelTitle>
        </div>
        <SidePanelCloseButton onClick={onCancel} label={t('ui.modal.close.aria')} />
      </SidePanelHeader>

      <SidePanelBody data-testid="install-confirm-drawer" className="flex flex-col gap-md">
        <p className="text-label-sm text-on-surface-variant">
          {t('extensions.installGate.reason')}
        </p>

        {/* The "why you're being stopped" list — every pattern the install-time
            rescan fired, with the category and the matched substring. */}
        <div>
          <h3 className="text-label-xs font-bold text-on-surface-variant uppercase tracking-wide mb-xs">
            {t('extensions.installGate.matchesHeading', { count: payload.match_count })}
          </h3>
          <ul className="flex flex-col gap-xs">
            {payload.matches.map((m, i) => (
              <li
                key={`${m.pattern}-${i}`}
                className="rounded-lg border border-error/30 bg-error-container/10 px-sm py-xs"
              >
                <div className="flex items-center gap-xs">
                  <span className="material-symbols-outlined icon-xs text-error" aria-hidden="true">bolt</span>
                  <span className="font-mono text-label-xs font-bold text-error">{m.category}</span>
                </div>
                <div className="mt-[2px] font-mono text-label-xs text-on-surface break-all">
                  “{m.matched_substring}”
                </div>
                <div className="mt-[1px] font-mono text-label-xs text-on-surface-variant break-all">
                  {m.pattern}
                </div>
              </li>
            ))}
          </ul>
        </div>

        <label className="block">
          <span className="mb-[2px] block text-label-xs text-on-surface-variant">
            {t('extensions.installGate.typePrompt')}
          </span>
          <input
            type="text"
            data-testid="install-confirm-input"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            spellCheck={false}
            autoComplete="off"
            aria-invalid={!exactMatch && touched ? true : undefined}
            className="w-full rounded-sm border border-outline-variant bg-surface px-sm py-xs font-mono text-label-sm"
          />
          <span className="mt-xs block font-mono text-label-xs text-on-surface-variant">
            {t('extensions.installGate.typeTarget', { name: payload.name })}
          </span>
          {touched && (
            <span
              role="status"
              className={cn('mt-xs block text-label-xs', exactMatch ? 'text-success' : 'text-error')}
            >
              {exactMatch
                ? t('extensions.installGate.matchHint')
                : t('extensions.installGate.mismatchHint')}
            </span>
          )}
        </label>

        <div className="mt-md flex gap-sm">
          <Button
            type="button"
            variant="secondary"
            onClick={onCancel}
            className="flex-1 rounded-lg"
          >
            {t('extensions.installGate.cancel')}
          </Button>
          <Button
            type="button"
            data-testid="install-confirm-button"
            onClick={() => {
              if (exactMatch) onConfirm(typed)
            }}
            disabled={!exactMatch}
            className="flex-1 rounded-lg"
          >
            {t('extensions.installGate.confirm')}
          </Button>
        </div>
      </SidePanelBody>
    </SidePanel>
  )
}

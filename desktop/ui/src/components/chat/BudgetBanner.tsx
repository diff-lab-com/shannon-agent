// BudgetBanner — P0-4 budget:warning (yellow) / budget:exceeded (red) bars.
//
// Rendered inside the chat page, directly under the header area. The
// warning bar is dismissible; the exceeded bar offers the frozen three
// actions — continue once (R2 W2-4: labeled by what it actually delivers —
// the held blocked payload, or the recorded last user turn as the explicit
// fallback — and sent with the budget-bypass flag), raise the budget
// (opens BudgetDialog), stop.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Banner } from '@/components/ui/banner'
import { Button } from '@/components/ui/button'
import BudgetDialog from '@/components/chat/BudgetDialog'
import { useT } from '@/i18n'
import type { BudgetStatusPayload } from '@/types'

export interface BudgetBannerProps {
  warning: BudgetStatusPayload | null
  exceeded: BudgetStatusPayload | null
  clearWarning: () => void
  clearExceeded: () => void
  /** Deliver one budget-exempt send (the continue target held by the page). */
  onContinueOnce: () => void
  /**
   * R2 W2-4: what "Continue once" will actually send —
   *   - 'blocked': the payload the pre-turn guard refused (held by the page);
   *   - 'last-message': the recorded last user turn (labeled fallback);
   *   - 'none': nothing to deliver — the action hides instead of staying a
   *     clickable no-op. Required (chat-testing 裁定 2026-10-03): the old
   *     `null`-with-optional-slot contract let a JS caller omit the prop and
   *     slip the `!== null` gate into a bogus continueLast button — the
   *     sentinel removes undefined from the type surface entirely.
   */
  continueTarget: 'blocked' | 'last-message' | 'none'
  sessionId: string | null
}

export default function BudgetBanner({
  warning,
  exceeded,
  clearWarning,
  clearExceeded,
  onContinueOnce,
  continueTarget,
  sessionId,
}: BudgetBannerProps) {
  const t = useT()
  const intl = useIntl()
  const [raiseOpen, setRaiseOpen] = useState(false)

  // B4 P2-11: USD via Intl (same approach as SlashResultCard) — localized
  // grouping/decimal separators instead of a hardcoded `$x.xx`.
  const fmt = (n: number) =>
    new Intl.NumberFormat(intl.locale, { style: 'currency', currency: 'USD' }).format(n)

  return (
    <>
      {warning && !exceeded && (
        <Banner variant="bar" tone="warning" onDismiss={clearWarning} dismissLabel={t('budget.banner.dismiss')}>
          <span className="material-symbols-outlined icon-md text-warning mt-[2px]" aria-hidden="true">error</span>
          <div className="flex-1 min-w-0">
            <p className="font-label-md font-bold text-on-surface">{t('budget.warning.title')}</p>
            <p className="text-body-sm text-on-surface-variant">
              {t('budget.warning.body', { spent: fmt(warning.spentUsd), budget: fmt(warning.budgetUsd) })}
            </p>
          </div>
        </Banner>
      )}
      {exceeded && (
        <Banner variant="bar" tone="error" onDismiss={clearExceeded} dismissLabel={t('budget.banner.dismiss')}>
          <span className="material-symbols-outlined icon-md text-error mt-[2px]" aria-hidden="true">block</span>
          <div className="flex-1 min-w-0">
            <p className="font-label-md font-bold text-on-surface">{t('budget.exceeded.title')}</p>
            <p className="text-body-sm text-on-surface-variant">
              {t('budget.exceeded.body', { spent: fmt(exceeded.spentUsd), budget: fmt(exceeded.budgetUsd) })}
            </p>
            <div className="flex flex-wrap gap-sm mt-sm">
              {/* Positive allowlist, not `!== 'none'`: a JS caller bypassing
                  the type (missing prop → undefined) must also get the hidden
                  action, not the old continueLast mis-render. */}
              {(continueTarget === 'blocked' || continueTarget === 'last-message') && (
                <Button
                  className="px-md py-xs rounded-full bg-primary text-on-primary font-label-md hover:bg-primary/90"
                  onClick={() => { clearExceeded(); onContinueOnce() }}
                >
                  {continueTarget === 'blocked'
                    ? t('budget.exceeded.continue')
                    : t('budget.exceeded.continueLast')}
                </Button>
              )}
              <Button
                variant="outline"
                className="px-md py-xs rounded-full font-label-md border-outline-variant/40 bg-surface-container-lowest/70"
                onClick={() => setRaiseOpen(true)}
              >
                {t('budget.exceeded.raise')}
              </Button>
              <Button
                variant="ghost"
                className="px-md py-xs rounded-full font-label-md text-on-surface-variant hover:bg-surface-container"
                onClick={clearExceeded}
              >
                {t('budget.exceeded.stop')}
              </Button>
            </div>
          </div>
        </Banner>
      )}
      <BudgetDialog
        open={raiseOpen}
        sessionId={sessionId}
        budget={exceeded?.budgetUsd ?? null}
        onClose={() => setRaiseOpen(false)}
        onSaved={() => {
          // The cap moved past the spend — the exceed condition is gone.
          clearExceeded()
          clearWarning()
        }}
      />
    </>
  )
}

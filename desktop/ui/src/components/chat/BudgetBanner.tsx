// BudgetBanner — P0-4 budget:warning (yellow) / budget:exceeded (red) bars.
//
// Rendered inside the chat page, directly under the header area. The
// warning bar is dismissible; the exceeded bar offers the frozen three
// actions — continue once (resends the last user message with the
// budget-bypass flag), raise the budget (opens BudgetDialog), stop.

import { useState } from 'react'
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
  /** Resend the last user message with the budget-bypass flag. */
  onContinueOnce: () => void
  sessionId: string | null
}

export default function BudgetBanner({
  warning,
  exceeded,
  clearWarning,
  clearExceeded,
  onContinueOnce,
  sessionId,
}: BudgetBannerProps) {
  const t = useT()
  const [raiseOpen, setRaiseOpen] = useState(false)

  const fmt = (n: number) => `$${n.toFixed(2)}`

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
              <Button
                className="px-md py-xs rounded-full bg-primary text-on-primary font-label-md hover:bg-primary/90"
                onClick={() => { clearExceeded(); onContinueOnce() }}
              >
                {t('budget.exceeded.continue')}
              </Button>
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

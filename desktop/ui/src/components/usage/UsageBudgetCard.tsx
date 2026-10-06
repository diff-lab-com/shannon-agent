// UsageBudgetCard — P2-1 the /usage page's monthly-budget visualization and
// setting surface (Notion 触顶 pattern): month-to-date spend against the
// user-set budget on a bar with the 80% / 100% thresholds marked, plus the
// one budget input (persisted via `configure('monthly_budget_usd')`).
//
// Read-only bar + honest empty state without a budget; the input is the
// single write path and never blocks anything.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import type { UsageGovernance } from '@/types'

function fmtUsd(locale: string, n: number): string {
  return `$${new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(n)}`
}

export default function UsageBudgetCard({
  governance,
  onSaved,
}: {
  governance: UsageGovernance
  onSaved: () => void
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const budgeted = governance.budgetUsd != null
  const percent = governance.percent ?? 0
  const overCap = budgeted && percent >= 100
  const overWarn = budgeted && !overCap && percent >= 80

  // The input holds the raw text; empty means "no budget". Synced from the
  // persisted snapshot so an external change (factory reset, another
  // surface) never leaves it lying.
  const [value, setValue] = useState(budgeted ? String(governance.budgetUsd) : '')
  const [saving, setSaving] = useState(false)
  useEffect(() => {
    setValue(governance.budgetUsd != null ? String(governance.budgetUsd) : '')
  }, [governance.budgetUsd, governance.month])

  const invalid = value.trim() !== '' && !(Number(value) > 0)

  const save = async () => {
    if (saving || invalid) return
    setSaving(true)
    try {
      await api.configure({ key: 'monthly_budget_usd', value: value.trim() })
      toast.success(t('usage.governance.saved'))
      onSaved()
    } catch (e) {
      toastError(t('usage.governance.saveFailed'), e)
    }
    setSaving(false)
  }

  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 p-lg" data-testid="usage-budget-card">
      <div className="flex items-center gap-xs mb-md">
        <span className="material-symbols-outlined icon-sm text-primary">savings</span>
        <h2 className="font-label-md font-bold text-on-surface">{t('usage.governance.cardTitle')}</h2>
      </div>

      {budgeted ? (
        <>
          <div className="flex items-baseline justify-between gap-md mb-xs min-w-0">
            <span className="font-body-lg font-bold text-on-surface tabular-nums truncate">
              {fmtUsd(intl.locale, governance.monthCostUsd)}
              <span className="font-label-sm font-normal text-on-surface-variant">
                {' '} / {fmtUsd(intl.locale, governance.budgetUsd!)}
              </span>
            </span>
            <span
              className={cn(
                'font-label-md font-bold tabular-nums shrink-0',
                overCap ? 'text-error' : overWarn ? 'text-warning' : 'text-on-surface-variant',
              )}
              data-testid="usage-budget-percent"
            >
              {Math.round(percent)}%
            </span>
          </div>
          {/* The bar with the 80% warn line marked (Notion threshold style). */}
          <div
            className="relative w-full h-2 rounded-full bg-surface-container overflow-hidden"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.min(100, Math.round(percent))}
            aria-label={t('usage.governance.barAria')}
          >
            <div
              data-testid="usage-budget-fill"
              className={cn(
                'h-full rounded-full transition-all',
                overCap ? 'bg-error' : overWarn ? 'bg-warning' : 'bg-primary',
              )}
              style={{ width: `${Math.min(100, Math.max(0, percent))}%` }}
            />
          </div>
          <div className="relative w-full h-3 mb-md" aria-hidden="true">
            <span
              className="absolute top-0 w-px h-3 bg-outline-variant"
              style={{ left: '80%' }}
            />
          </div>
          <p className="font-label-sm text-label-xs text-on-surface-variant">
            {t('usage.governance.resetHint')}
          </p>
        </>
      ) : (
        <p className="font-body-sm text-on-surface-variant mb-md">
          {t('usage.governance.noBudgetHint')}
        </p>
      )}

      {/* The one budget setting (brief: /settings/advanced 或 /usage — this
          is the canonical surface, next to the numbers it governs). */}
      <div className="flex flex-col sm:flex-row sm:items-end gap-sm mt-md">
        <label className="flex flex-col gap-xs flex-1 min-w-0">
          <span className="font-label-sm text-label-sm text-on-surface-variant">
            {t('usage.governance.inputLabel')}
          </span>
          <input
            type="number"
            min={0}
            step="0.01"
            value={value}
            onChange={e => setValue(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter') void save() }}
            placeholder={t('usage.governance.inputPlaceholder')}
            aria-label={t('usage.governance.inputLabel')}
            aria-invalid={invalid || undefined}
            data-testid="usage-budget-input"
            className="bg-surface-container-lowest rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
          {invalid ? (
            <span className="font-label-sm text-label-xs text-error">{t('usage.governance.invalid')}</span>
          ) : (
            <span className="font-label-sm text-label-xs text-on-surface-variant">
              {t('usage.governance.inputHint')}
            </span>
          )}
        </label>
        <Button
          className="px-xl py-md bg-primary text-on-primary rounded-lg font-label-md text-body-sm font-bold hover:bg-primary/90 shadow-e1 active:scale-[0.98] transition-all whitespace-nowrap cursor-pointer disabled:opacity-50"
          onClick={() => void save()}
          disabled={saving || invalid}
          aria-label={t('usage.governance.save')}
        >
          {saving ? t('usage.governance.saving') : t('usage.governance.save')}
        </Button>
      </div>
    </div>
  )
}

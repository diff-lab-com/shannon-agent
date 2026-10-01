// SidebarUsageMeter — P2-1 the sidebar's persistent, lightweight usage bar.
//
// Claude-侧栏-%条 pattern: one thin line above the account section showing
// month-to-date spend against the user-set monthly budget (color steps at
// the 80% / 100% thresholds). Without a budget it degrades to the trailing
// 7-day cost as a plain number (the brief's fallback口径). Hover explains
// the numbers + the monthly reset; clicking opens /usage.
//
// Renders nothing until the governance snapshot arrives — a failing or
// not-yet-loaded fetch must never occupy rail space.

import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { cn } from '@/lib/utils'
import { useUsageGovernance } from '@/hooks/useUsageGovernance'

function fmtUsd(locale: string, n: number): string {
  return `$${new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(n)}`
}

export default function SidebarUsageMeter({ onNavigate }: { onNavigate?: () => void }) {
  const intl = useIntl()
  const navigate = useNavigate()
  const { governance } = useUsageGovernance()

  if (!governance) return null

  const budgeted = governance.budgetUsd != null && governance.percent != null
  const percent = budgeted ? Math.round(governance.percent!) : null
  const overCap = budgeted && governance.percent! >= 100
  const overWarn = budgeted && !overCap && governance.percent! >= 80

  return (
    <button
      type="button"
      data-testid="sidebar-usage-meter"
      onClick={() => { navigate('/usage'); onNavigate?.() }}
      aria-label={
        budgeted
          ? intl.formatMessage({ id: 'sidebar.usage.meterBudgetAria' }, { percent })
          : intl.formatMessage({ id: 'sidebar.usage.meterNoBudgetAria' }, { spent: fmtUsd(intl.locale, governance.last7dCostUsd) })
      }
      title={
        budgeted
          ? intl.formatMessage(
              { id: 'sidebar.usage.meterBudgetTitle' },
              { spent: fmtUsd(intl.locale, governance.monthCostUsd), budget: fmtUsd(intl.locale, governance.budgetUsd!), percent },
            )
          : intl.formatMessage({ id: 'sidebar.usage.meterNoBudgetTitle' }, { spent: fmtUsd(intl.locale, governance.last7dCostUsd) })
      }
      className="w-full flex flex-col gap-1 px-3 py-sm rounded-lg font-label-md text-label-sm text-on-surface-variant hover:bg-surface-container-low hover:text-primary cursor-pointer transition-all h-auto min-w-0 text-left"
    >
      <span className="flex items-center gap-sm min-w-0 w-full">
        <span
          className={cn(
            'material-symbols-outlined icon-sm shrink-0',
            overCap ? 'text-error' : overWarn ? 'text-warning' : 'text-secondary',
          )}
          aria-hidden="true"
        >
          monitoring
        </span>
        <span className="flex-1 min-w-0 truncate">
          {budgeted
            ? intl.formatMessage({ id: 'sidebar.usage.meterBudgetLabel' }, { percent })
            : intl.formatMessage(
                { id: 'sidebar.usage.meterNoBudgetLabel' },
                { spent: fmtUsd(intl.locale, governance.last7dCostUsd) },
              )}
        </span>
      </span>
      {budgeted && (
        <span
          className="block w-full h-1 rounded-full bg-surface-container overflow-hidden"
          aria-hidden="true"
        >
          <span
            data-testid="sidebar-usage-meter-fill"
            className={cn(
              'block h-full rounded-full transition-all',
              overCap ? 'bg-error' : overWarn ? 'bg-warning' : 'bg-primary',
            )}
            style={{ width: `${Math.min(100, Math.max(0, governance.percent!))}%` }}
          />
        </span>
      )}
    </button>
  )
}

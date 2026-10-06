// CostEstimateHint — P2-6 the pre-task cost estimate (Manus 任务前预估
// pattern): a read-only line showing the historical run-cost range of the
// routine (or, with no taskId yet, the all-routines "similar tasks"
// baseline) before the user confirms a creation. Never blocks: fetch
// failures and empty history render the honest "first run" copy or nothing.
//
// Debounced 350ms so typing-driven remounts (OPC quick-add) don't fire one
// query per keystroke.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'
import type { TaskCostEstimate } from '@/types'

function fmtUsd(locale: string, n: number): string {
  return `$${new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(n)}`
}

export default function CostEstimateHint({
  taskId = null,
  className,
}: {
  taskId?: string | null
  className?: string
}) {
  const intl = useIntl()
  const [estimate, setEstimate] = useState<TaskCostEstimate | null>(null)

  useEffect(() => {
    let cancelled = false
    const timer = setTimeout(() => {
      api.estimateTaskCost(taskId)
        .then(e => { if (!cancelled) setEstimate(e) })
        .catch(() => { if (!cancelled) setEstimate(null) })
    }, 350)
    return () => {
      cancelled = true
      clearTimeout(timer)
    }
  }, [taskId])

  // Hidden until the estimate lands — no layout flicker for a nice-to-have.
  if (!estimate) return null

  return (
    <div
      data-testid="cost-estimate-hint"
      className={cn(
        'font-label-sm text-label-xs text-on-surface-variant flex items-center gap-xs',
        className,
      )}
      aria-label={intl.formatMessage({ id: 'tasks.costEstimate.aria' })}
    >
      <span className="material-symbols-outlined icon-sm text-secondary shrink-0" aria-hidden="true">
        payments
      </span>
      {estimate.hasHistory ? (
        <span>
          {intl.formatMessage(
            { id: 'tasks.costEstimate.range' },
            {
              min: fmtUsd(intl.locale, estimate.minUsd ?? 0),
              max: fmtUsd(intl.locale, estimate.maxUsd ?? 0),
              avg: fmtUsd(intl.locale, estimate.avgUsd ?? 0),
              count: estimate.runsCounted,
            },
          )}
        </span>
      ) : (
        <span>{intl.formatMessage({ id: 'tasks.costEstimate.firstRun' })}</span>
      )}
    </div>
  )
}

// 目标 side card (design 04-tasks 右列下卡, G4a): compact goal-run summary —
// name + status chip + the aurora-gradient progress bar (spent/budget %) +
// a rounds/spent meta line. Data rides the same useGoalRuns subscription the
// run cards use (list_goal_runs + goal:updated push). No runs → the honest
// empty line (goal.empty.description), never a fake bar.

import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { useGoalRuns } from '@/hooks/goalRuns'
import { projectKeyOf } from '@/components/SidebarSessions'
import type { GoalRunDto, GoalRunStatus } from '@/types'

/** Full-value semantic container pair per status (no alpha text — the
    axe contrast lesson from G1/G3). */
const STATUS_CHIP: Record<GoalRunStatus, string> = {
  running: 'bg-primary-container text-on-primary-container',
  paused: 'bg-tertiary-container text-on-tertiary-container',
  completed: 'bg-success-container text-on-success-container',
  blocked: 'bg-error-container text-on-error-container',
  stopped: 'bg-surface-container-high text-on-surface-variant',
  interrupted: 'bg-surface-container-high text-on-surface-variant',
}

function GoalSideRow({ run }: { run: GoalRunDto }) {
  const intl = useIntl()
  const budgetFrac =
    run.budgetUsd && run.budgetUsd > 0 ? Math.min(1, run.spentUsd / run.budgetUsd) : null
  const pct = budgetFrac != null ? `${Math.round(budgetFrac * 100)}%` : null

  return (
    <li className="min-w-0" data-testid="tasks-goal-row" data-status={run.status}>
      <div className="flex items-center gap-xs min-w-0">
        <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">flag</span>
        <span className="flex-1 min-w-0 truncate font-label-md text-on-surface font-medium" title={run.title}>
          {run.title}
        </span>
        <span className={cn('px-sm py-[2px] rounded-full text-label-2xs font-bold shrink-0', STATUS_CHIP[run.status])}>
          {intl.formatMessage({ id: `goal.status.${run.status}` })}
        </span>
      </div>
      {/* Aurora gradient progress (S3 signature — .aurora-bar), share of the
          USD budget; a run without a budget cap shows no bar (honest). */}
      {budgetFrac != null && pct && (
        <div
          role="progressbar"
          aria-valuenow={Math.round(budgetFrac * 100)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={intl.formatMessage({ id: 'goal.card.budgetUsage' })}
          className="mt-xs h-1.5 rounded-full bg-surface-container-highest overflow-hidden"
        >
          <div className="h-full rounded-full aurora-bar" style={{ width: pct }} />
        </div>
      )}
      <div className="mt-[2px] font-label-xs text-on-surface-variant tabular-nums">
        {intl.formatMessage({ id: 'goal.card.iterations' })}{' '}
        {intl.formatNumber(run.iterations)}
        {run.maxTurns ? ` / ${intl.formatNumber(run.maxTurns)}` : ''}
        {run.spentUsd > 0 && (
          <>
            {' · '}
            {intl.formatNumber(run.spentUsd, { style: 'currency', currency: 'USD' })}
          </>
        )}
      </div>
    </li>
  )
}

export default function GoalsSideCard({ projectDir }: { projectDir?: string | null }) {
  const intl = useIntl()
  const { runs } = useGoalRuns()
  // P-U3 parity with GoalRunPanel: under /tasks?project= the summary only
  // shows runs rooted in that project (same normalized workingDir key);
  // unhoused runs drop out while the filter is active.
  const scoped = projectDir
    ? runs.filter(r => projectKeyOf({ working_dir: r.workingDir }) === projectDir)
    : runs
  // Live runs lead; cap at four so the rail stays a summary (the full cards
  // live in the Runs tab's GoalRunPanel).
  const rows = [...scoped]
    .sort((a, b) => b.updatedAtMs - a.updatedAtMs)
    .slice(0, 4)

  return (
    <section
      aria-label={intl.formatMessage({ id: 'tasks.goals.title' })}
      data-testid="tasks-goals-card"
      className="bg-surface-container-lowest border border-outline-variant/30 rounded-2xl p-lg shadow-e1"
    >
      <div className="flex items-center justify-between mb-md">
        <div className="flex items-center gap-xs min-w-0">
          <span className="material-symbols-outlined icon-md text-on-surface-variant" aria-hidden="true">flag</span>
          <h4 className="font-headline-md text-body-lg text-on-surface">
            {intl.formatMessage({ id: 'tasks.goals.title' })}
          </h4>
        </div>
        <span className="font-label-xs text-on-surface-variant tabular-nums">
          {intl.formatDate(new Date(), { month: 'short', day: 'numeric' })}
        </span>
      </div>

      {rows.length === 0 ? (
        <p className="font-label-sm text-on-surface-variant" data-testid="tasks-goals-empty">
          {intl.formatMessage({ id: 'goal.empty.description' })}
        </p>
      ) : (
        <ul className="space-y-md">
          {rows.map(run => <GoalSideRow key={run.sessionId} run={run} />)}
        </ul>
      )}
    </section>
  )
}

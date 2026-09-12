// Goal run cards (P0-2) — live view of the desktop goal runner on the
// Tasks page's Active tab. Data comes from `useGoalRuns` (list_goal_runs +
// `goal:updated` push), so cards update the moment a turn finishes without
// polling. Hidden entirely when there is nothing to show — the entry point
// is the composer's /goal slash command, which surfaces runs here.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useGoalRuns } from '@/hooks/goalRuns'
import NewGoalDialog from './NewGoalDialog'
import type { GoalRunDto, GoalRunStatus } from '@/types'

/** MD3 badge classes per goal-run status (theme semantic tokens). */
function goalStatusBadge(status: GoalRunStatus): { bg: string; dot: string; icon: string; labelId: string } {
  switch (status) {
    case 'running':
      return { bg: 'bg-primary/10 text-primary border-primary/20', dot: 'bg-primary animate-pulse', icon: 'autorenew', labelId: 'goal.status.running' }
    case 'paused':
      return { bg: 'bg-tertiary/10 text-tertiary border-tertiary/20', dot: 'bg-tertiary', icon: 'pause_circle', labelId: 'goal.status.paused' }
    case 'completed':
      return { bg: 'bg-success/10 text-success border-success/20', dot: 'bg-success', icon: 'check_circle', labelId: 'goal.status.completed' }
    case 'blocked':
      return { bg: 'bg-error/10 text-error border-error/20', dot: 'bg-error', icon: 'block', labelId: 'goal.status.blocked' }
    case 'stopped':
      return { bg: 'bg-surface-container-high text-on-surface-variant border-outline-variant/30', dot: 'bg-outline-variant', icon: 'stop_circle', labelId: 'goal.status.stopped' }
    case 'interrupted':
      return { bg: 'bg-secondary/20 text-on-surface border-outline-variant/30', dot: 'bg-secondary', icon: 'power_off', labelId: 'goal.status.interrupted' }
  }
}

interface GoalRunCardProps {
  run: GoalRunDto
  onPause: (id: string) => void
  onResume: (id: string) => void
  onStop: (id: string) => void
  onUpdateObjective: (id: string, objective: string) => Promise<boolean>
  onViewSession: (id: string) => void
}

export function GoalRunCard({ run, onPause, onResume, onStop, onUpdateObjective, onViewSession }: GoalRunCardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState(run.objective)
  const badge = goalStatusBadge(run.status)
  const active = run.status === 'running' || run.status === 'paused'

  const fmtPct = (v: number) => `${Math.round(v * 100)}%`
  const budgetFrac =
    run.budgetUsd && run.budgetUsd > 0 ? Math.min(1, run.spentUsd / run.budgetUsd) : null

  const submitObjective = async () => {
    const next = draft.trim()
    setEditing(false)
    if (!next || next === run.objective) return
    await onUpdateObjective(run.sessionId, next)
  }

  return (
    <div
      className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm bg-surface-container-lowest/80"
      data-testid="goal-run-card"
      data-status={run.status}
    >
      <div className="flex items-start justify-between gap-md">
        <div className="flex items-start gap-md min-w-0">
          <div className="w-10 h-10 rounded-xl bg-primary/10 flex items-center justify-center text-primary shrink-0">
            <span className="material-symbols-outlined text-[24px]">flag</span>
          </div>
          <div className="min-w-0">
            <h3 className="font-body-lg font-semibold text-on-surface truncate">{run.title}</h3>
            {editing ? (
              <div className="mt-1 flex items-center gap-xs">
                <input
                  aria-label={t('goal.card.objectiveEditAria')}
                  className="flex-1 h-8 px-sm rounded-lg bg-surface-container-high border border-outline-variant/40 font-label-sm text-on-surface focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                  value={draft}
                  onChange={e => setDraft(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') void submitObjective() }}
                  autoFocus
                />
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-8 cursor-pointer"
                  onClick={() => void submitObjective()}
                >
                  {t('goal.card.save')}
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-8 cursor-pointer"
                  onClick={() => { setDraft(run.objective); setEditing(false) }}
                >
                  {t('goal.card.cancelEdit')}
                </Button>
              </div>
            ) : (
              <p className="font-label-sm text-on-surface-variant line-clamp-2">{run.objective}</p>
            )}
          </div>
        </div>
        <div
          title={t(`goal.status.${run.status}`)}
          className={cn('flex items-center gap-xs px-sm py-1 rounded-full border shrink-0', badge.bg)}
        >
          <span className={cn('w-2 h-2 rounded-full', badge.dot)} />
          <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">
            {t(badge.labelId)}
          </span>
        </div>
      </div>

      <dl className="mt-sm grid grid-cols-2 sm:grid-cols-4 gap-x-md gap-y-xxs font-label-sm">
        <div>
          <dt className="text-on-surface-variant">{t('goal.card.iterations')}</dt>
          <dd className="text-on-surface tabular-nums">
            {intl.formatNumber(run.iterations)}
            {run.maxTurns ? <span className="text-on-surface-variant"> / {intl.formatNumber(run.maxTurns)}</span> : null}
          </dd>
        </div>
        <div>
          <dt className="text-on-surface-variant">{t('goal.card.spent')}</dt>
          <dd className="text-on-surface tabular-nums">
            {intl.formatNumber(run.spentUsd, { style: 'currency', currency: 'USD' })}
            {run.budgetUsd ? <span className="text-on-surface-variant"> / {intl.formatNumber(run.budgetUsd, { style: 'currency', currency: 'USD' })}</span> : null}
          </dd>
        </div>
        <div>
          <dt className="text-on-surface-variant">{t('goal.card.stallStrikes')}</dt>
          <dd className="text-on-surface tabular-nums">{intl.formatNumber(run.stallStrikes)}</dd>
        </div>
        <div>
          <dt className="text-on-surface-variant">{t('goal.card.session')}</dt>
          <dd className="text-on-surface font-mono text-[11px] truncate">{run.sessionId.slice(0, 8)}</dd>
        </div>
      </dl>

      {budgetFrac !== null && active && (
        <div
          role="progressbar"
          aria-valuenow={Math.round(budgetFrac * 100)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label={t('goal.card.budgetBarAria')}
          className="mt-xs h-1.5 rounded-full bg-surface-container-highest overflow-hidden"
        >
          <div
            className={cn('h-full rounded-full', budgetFrac > 0.9 ? 'bg-error' : 'bg-primary')}
            style={{ width: fmtPct(budgetFrac) }}
          />
        </div>
      )}

      {run.lastError && (
        <p role="note" className="mt-xs font-label-xs text-error line-clamp-2" title={run.lastError}>
          {run.lastError}
        </p>
      )}

      <div className="mt-sm flex items-center gap-xs justify-end">
        <Button
          variant="ghost"
          size="sm"
          className="h-8 font-label-sm cursor-pointer"
          onClick={() => onViewSession(run.sessionId)}
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">chat</span>
          {t('goal.card.viewSession')}
        </Button>
        {!editing && (
          <Button
            variant="ghost"
            size="sm"
            className="h-8 font-label-sm cursor-pointer"
            onClick={() => setEditing(true)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">edit</span>
            {t('goal.card.editObjective')}
          </Button>
        )}
        {run.status === 'running' && (
          <Button
            variant="ghost"
            size="sm"
            aria-label={t('goal.card.pauseAria')}
            className="h-8 font-label-sm cursor-pointer"
            onClick={() => onPause(run.sessionId)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">pause_circle</span>
            {t('goal.card.pause')}
          </Button>
        )}
        {run.status === 'paused' && (
          <Button
            variant="ghost"
            size="sm"
            aria-label={t('goal.card.resumeAria')}
            className="h-8 font-label-sm cursor-pointer"
            onClick={() => onResume(run.sessionId)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">play_circle</span>
            {t('goal.card.resume')}
          </Button>
        )}
        {active && (
          <Button
            variant="ghost"
            size="sm"
            aria-label={t('goal.card.stopAria')}
            className="h-8 font-label-sm text-error hover:bg-error/10 cursor-pointer"
            onClick={() => onStop(run.sessionId)}
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">stop_circle</span>
            {t('goal.card.stop')}
          </Button>
        )}
      </div>
    </div>
  )
}

function GoalRunPanelImpl({ onViewSession }: { onViewSession: (id: string) => void }) {
  const intl = useIntl()
  const { runs, pause, resume, stop, updateObjective, start } = useGoalRuns()
  const [creating, setCreating] = useState(false)

  return (
    <section aria-labelledby="goal-runs-heading" className="mb-lg" data-testid="goal-run-panel">
      <div className="flex items-center justify-between mb-sm">
        <h2 id="goal-runs-heading" className="font-label-lg font-bold text-on-surface flex items-center gap-xs">
          <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">flag</span>
          {intl.formatMessage({ id: 'goal.panel.heading' })}
        </h2>
        <Button
          type="button"
          size="sm"
          onClick={() => setCreating(true)}
          className="cursor-pointer inline-flex items-center gap-xs"
          data-testid="goal-new-button"
        >
          <span className="material-symbols-outlined text-[16px]" aria-hidden="true">add</span>
          {intl.formatMessage({ id: 'goal.new.button' })}
        </Button>
      </div>
      {runs.length === 0 ? (
        <p className="font-body-sm text-on-surface-variant px-sm py-md rounded-xl border border-outline-variant/30 bg-surface-container-lowest/60">
          {intl.formatMessage({ id: 'goal.empty.description' })}
        </p>
      ) : (
        <div className="space-y-sm">
          {runs.map(run => (
            <GoalRunCard
              key={run.sessionId}
              run={run}
              onPause={pause}
              onResume={resume}
              onStop={stop}
              onUpdateObjective={updateObjective}
              onViewSession={onViewSession}
            />
          ))}
        </div>
      )}
      <NewGoalDialog open={creating} onClose={() => setCreating(false)} onStart={start} />
    </section>
  )
}

export default function GoalRunPanel({ onViewSession }: { onViewSession: (id: string) => void }) {
  return <GoalRunPanelImpl onViewSession={onViewSession} />
}

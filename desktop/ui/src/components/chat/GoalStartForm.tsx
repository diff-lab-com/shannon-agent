// Inline /goal form (P0-2).
//
// Rendered inside SlashResultCard for `{ kind: 'goalForm' }`. The slash
// parser only fires on a bare `/name`, so /goal cannot carry arguments as
// text — the brief's chosen pattern is this small pinned form next to the
// composer: title, objective, optional turn cap and budget. Submitting
// calls `start_goal_run` on the current session (the backend creates a
// dedicated goal session when none is open) and the run becomes visible on
// the Tasks page and, live, in the chat via the standard query:* events.

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import * as api from '@/lib/tauri-api'

interface GoalStartFormProps {
  /** Session the goal runs in; null lets the backend create one. */
  sessionId: string | null
  onDismiss: () => void
}

const inputClass =
  'w-full h-9 px-sm rounded-lg bg-surface-container-high border border-outline-variant/40 ' +
  'font-body-md text-on-surface placeholder:text-on-surface-variant/60 ' +
  'focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40'

export default function GoalStartForm({ sessionId, onDismiss }: GoalStartFormProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [title, setTitle] = useState('')
  const [objective, setObjective] = useState('')
  const [maxTurns, setMaxTurns] = useState('')
  const [budgetUsd, setBudgetUsd] = useState('')
  const [submitting, setSubmitting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [startedSessionId, setStartedSessionId] = useState<string | null>(null)

  const canSubmit = objective.trim().length > 0 && !submitting

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setSubmitting(true)
    setError(null)
    try {
      const turns = Number.parseInt(maxTurns, 10)
      const budget = Number.parseFloat(budgetUsd)
      const { sessionId: id } = await api.startGoalRun({
        sessionId,
        title: title.trim() || objective.trim(),
        objective: objective.trim(),
        maxTurns: Number.isFinite(turns) && turns > 0 ? turns : undefined,
        budgetUsd: Number.isFinite(budget) && budget > 0 ? budget : undefined,
      })
      setStartedSessionId(id)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setSubmitting(false)
    }
  }

  if (startedSessionId) {
    return (
      <div className="flex items-start gap-sm" data-testid="goal-start-success">
        <span className="material-symbols-outlined icon-md text-tertiary" aria-hidden="true">check_circle</span>
        <div className="flex-1">
          <p className="font-label-md text-on-surface">{t('slash.card.goal.started')}</p>
          <p className="mt-xxs font-label-xs text-on-surface-variant">{t('slash.card.goal.startedHint')}</p>
        </div>
      </div>
    )
  }

  return (
    <form onSubmit={handleSubmit} className="flex flex-col gap-sm" aria-label={t('slash.card.goal.title')}>
      <div className="grid grid-cols-1 gap-sm">
        <label className="flex flex-col gap-xxs">
          <span className="font-label-sm text-on-surface-variant">{t('slash.card.goal.titleField')}</span>
          <input
            className={inputClass}
            value={title}
            onChange={e => setTitle(e.target.value)}
            placeholder={t('slash.card.goal.titlePlaceholder')}
            maxLength={120}
          />
        </label>
        <label className="flex flex-col gap-xxs">
          <span className="font-label-sm text-on-surface-variant">{t('slash.card.goal.objective')}</span>
          <textarea
            className={cn(inputClass, 'h-auto py-sm min-h-16 resize-y')}
            value={objective}
            onChange={e => setObjective(e.target.value)}
            placeholder={t('slash.card.goal.objectivePlaceholder')}
            required
            rows={2}
          />
        </label>
        <div className="grid grid-cols-2 gap-sm">
          <label className="flex flex-col gap-xxs">
            <span className="font-label-sm text-on-surface-variant">{t('slash.card.goal.maxTurns')}</span>
            <input
              className={inputClass}
              value={maxTurns}
              onChange={e => setMaxTurns(e.target.value)}
              inputMode="numeric"
              placeholder={t('slash.card.goal.unlimited')}
            />
          </label>
          <label className="flex flex-col gap-xxs">
            <span className="font-label-sm text-on-surface-variant">{t('slash.card.goal.budget')}</span>
            <input
              className={inputClass}
              value={budgetUsd}
              onChange={e => setBudgetUsd(e.target.value)}
              inputMode="decimal"
              placeholder={t('slash.card.goal.noCap')}
            />
          </label>
        </div>
      </div>
      {error && (
        <p role="alert" className="font-label-sm text-error">{error}</p>
      )}
      <div className="flex items-center justify-between gap-sm">
        <span className="font-label-xs text-on-surface-variant">{t('slash.card.goal.hint')}</span>
        <div className="flex items-center gap-xs">
          <Button
            type="button"
            variant="ghost"
            className="px-md py-sm rounded-lg font-label-md cursor-pointer"
            onClick={onDismiss}
          >
            {t('slash.card.goal.cancel')}
          </Button>
          <Button
            type="submit"
            disabled={!canSubmit}
            className="bg-primary text-on-primary px-md py-sm rounded-lg font-label-md flex items-center gap-xs hover:brightness-110 active:scale-95 transition-all cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
          >
            <span className="material-symbols-outlined icon-md" aria-hidden="true">play_arrow</span>
            {submitting ? t('slash.card.goal.starting') : t('slash.card.goal.start')}
          </Button>
        </div>
      </div>
    </form>
  )
}

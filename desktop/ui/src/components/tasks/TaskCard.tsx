// Single task row in the list view.
//
// MD3 tokens. Glass-panel styling. Shows status badge, assignee, priority, and
// action buttons (Cancel for running, Run Now for all).

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { TaskItem } from '@/types'
import { statusBadge } from './shared'
import { cn } from '@/lib/utils'

interface TaskCardProps {
  task: TaskItem
  isRunning: boolean
  onSelect: () => void
  onRunNow: () => void
  onCancel: () => void
}

export default function TaskCard({ task, isRunning, onSelect, onRunNow, onCancel }: TaskCardProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const badge = statusBadge(task.status)
  const isActive = task.status === 'running' || task.status === 'in_progress'
  return (
    // B6-37 (§5 任务): the card is clickable via its title button. The card
    // container itself must NOT carry role="button" — it contains real action
    // buttons, and nested interactive controls are an axe serious violation
    // (caught by the walkthrough gate). The title button is the keyboard-
    // reachable open action; inner buttons stay natively focusable and
    // stopPropagation keeps their clicks card-local.
    <div className="bg-surface-container-lowest border border-outline-variant/10 rounded-xl p-md shadow-e1 hover:shadow-e2 hover:-translate-y-0.5 transition-all duration-(--duration-slow) group">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-md">
          <div className="w-12 h-12 rounded-xl bg-primary-container flex items-center justify-center text-on-primary-container">
            <span className="material-symbols-outlined icon-xl">task_alt</span>
          </div>
          <div>
            <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-primary transition-colors">
              <button
                type="button"
                className="text-left w-full hover:underline underline-offset-2 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded-sm"
                onClick={onSelect}
              >
                {task.title}
              </button>
            </h3>
            <div className="flex items-center gap-md mt-xs">
              {task.assignee ? (
                <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                  <span className="material-symbols-outlined icon-sm">smart_toy</span>
                  {task.assignee}
                </span>
              ) : null}
              {task.priority ? (
                <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                  <span className="material-symbols-outlined icon-sm">flag</span>
                  {task.priority}
                </span>
              ) : null}
              {task.team ? (
                <span
                  // B6-36: the tooltip used to hardcode "Team: …".
                  title={intl.formatMessage({ id: 'tasks.taskCard.teamTitle' }, { team: task.team })}
                  className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs"
                >
                  <span className="material-symbols-outlined icon-sm">groups</span>
                  {task.team}
                </span>
              ) : null}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-lg">
          {/* B6-37: status changes announce politely. */}
          <div aria-live="polite" title={intl.formatMessage({ id: badge.tipId }, badge.values)} className={cn('flex items-center gap-xs px-sm py-xs rounded-full border', badge.bg)}>
            <span className={cn('w-2 h-2 rounded-full', badge.dot)} />
            <span className="font-label-sm text-label-xs font-bold uppercase tracking-wider">{intl.formatMessage({ id: badge.labelId }, badge.values)}</span>
          </div>
          <div className="flex items-center gap-sm">
            {isActive ? (
              <Button
                aria-label={t('tasks.taskCard.cancelAria')}
                className="p-sm rounded-lg hover:bg-error/10 text-error transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-error/40"
                onClick={e => { e.stopPropagation(); onCancel() }}
              >
                <span className="material-symbols-outlined" aria-hidden="true">stop_circle</span>
              </Button>
            ) : null}
            <Button
              className={cn('text-on-primary px-md py-sm rounded-lg font-label-md flex items-center gap-xs hover:brightness-110 active:scale-95 transition-all cursor-pointer', isRunning ? 'bg-tertiary' : 'bg-primary')}
              onClick={e => { e.stopPropagation(); onRunNow() }}
              disabled={isRunning}
            >
              {isRunning ? (
                <>
                  <span className="material-symbols-outlined icon-md">check_circle</span>
                  {t('tasks.taskCard.success')}
                </>
              ) : (
                <>
                  <span className="material-symbols-outlined icon-md">play_arrow</span>
                  {t('tasks.taskCard.runNow')}
                </>
              )}
            </Button>
          </div>
        </div>
      </div>
      {task.description ? <p className="mt-sm text-body-sm text-on-surface-variant pl-[72px]">{task.description}</p> : null}
    </div>
  )
}

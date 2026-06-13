// Single task row in the list view.
//
// MD3 tokens. Glass-panel styling. Shows status badge, assignee, priority, and
// action buttons (Cancel for running, Run Now for all).

import { Button } from '@/components/ui/button'
import type { TaskItem } from '@/types'
import { statusBadge } from './shared'

interface TaskCardProps {
  task: TaskItem
  isRunning: boolean
  onSelect: () => void
  onRunNow: () => void
  onCancel: () => void
}

export default function TaskCard({ task, isRunning, onSelect, onRunNow, onCancel }: TaskCardProps) {
  const badge = statusBadge(task.status)
  const isActive = task.status === 'running' || task.status === 'in_progress'
  return (
    <div
      className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-300 group bg-surface-container-lowest/80 cursor-pointer"
      onClick={onSelect}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-md">
          <div className="w-12 h-12 rounded-xl bg-primary/10 flex items-center justify-center text-primary">
            <span className="material-symbols-outlined text-[28px]">task_alt</span>
          </div>
          <div>
            <h3 className="font-body-lg font-semibold text-on-surface group-hover:text-primary transition-colors">{task.title}</h3>
            <div className="flex items-center gap-md mt-1">
              {task.assignee ? (
                <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                  <span className="material-symbols-outlined text-[14px]">smart_toy</span>
                  {task.assignee}
                </span>
              ) : null}
              {task.priority ? (
                <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                  <span className="material-symbols-outlined text-[14px]">flag</span>
                  {task.priority}
                </span>
              ) : null}
              {task.team ? (
                <span
                  title={`Team: ${task.team}`}
                  className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs"
                >
                  <span className="material-symbols-outlined text-[14px]">groups</span>
                  {task.team}
                </span>
              ) : null}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-lg">
          <div title={badge.tip} className={`flex items-center gap-xs px-sm py-1 rounded-full border ${badge.bg}`}>
            <span className={`w-2 h-2 rounded-full ${badge.dot}`} />
            <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">{badge.label}</span>
          </div>
          <div className="flex items-center gap-sm">
            {isActive ? (
              <Button
                aria-label="Cancel task"
                className="p-2 rounded-lg hover:bg-error/10 text-error transition-colors cursor-pointer"
                onClick={e => { e.stopPropagation(); onCancel() }}
              >
                <span className="material-symbols-outlined" aria-hidden="true">stop_circle</span>
              </Button>
            ) : null}
            <Button
              className={`text-on-primary px-md py-sm rounded-lg font-label-md flex items-center gap-xs hover:brightness-110 active:scale-95 transition-all cursor-pointer ${isRunning ? 'bg-tertiary' : 'bg-primary'}`}
              onClick={e => { e.stopPropagation(); onRunNow() }}
              disabled={isRunning}
            >
              {isRunning ? (
                <>
                  <span className="material-symbols-outlined text-[18px]">check_circle</span>
                  Success
                </>
              ) : (
                <>
                  <span className="material-symbols-outlined text-[18px]">play_arrow</span>
                  Run Now
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

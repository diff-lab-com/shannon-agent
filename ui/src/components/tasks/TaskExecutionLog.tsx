// Timeline of background task runs (the "Task Execution Log" section).
//
// MD3 tokens. Vertical timeline with colored dots per status. Cancel button
// for running tasks.

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import type { BackgroundTaskInfo } from '@/types'
import { statusBadge, formatTime } from './shared'

interface TaskExecutionLogProps {
  tasks: BackgroundTaskInfo[]
  onCancel: (id: string) => void
}

export default function TaskExecutionLog({ tasks, onCancel }: TaskExecutionLogProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  if (tasks.length === 0) return null
  return (
    <div className="pt-lg">
      <h4 className="font-label-md text-label-md text-outline uppercase tracking-[0.1em] mb-md pl-xs">{t('tasks.taskExecutionLog.title')}</h4>
      <div className="relative pl-8 border-l border-outline-variant/30 space-y-lg ml-md">
        {tasks.map(bt => {
          const badge = statusBadge(bt.status)
          return (
            <div key={bt.task_id} className="relative">
              <div className={`absolute -left-[41px] top-1 w-4 h-4 rounded-full border-2 bg-surface-container-lowest z-10 ${bt.status === 'running' ? 'border-primary animate-pulse' : bt.status === 'completed' ? 'border-tertiary' : bt.status === 'failed' ? 'border-error' : 'border-outline-variant'}`} />
              <div className="flex justify-between items-start mb-1">
                <div>
                  <p className={`font-label-sm text-label-sm mb-1 ${badge.bg.includes('primary') ? 'text-primary' : badge.bg.includes('tertiary') ? 'text-tertiary' : badge.bg.includes('error') ? 'text-error' : 'text-on-surface-variant'}`}>
                    {formatTime(bt.started_at)} — {badge.label.toUpperCase()}
                  </p>
                  <p className="text-on-surface-variant text-body-sm italic">{bt.prompt}</p>
                  {bt.output ? <pre className="mt-sm text-body-sm text-on-surface bg-surface-container-low p-sm rounded-lg max-h-[120px] overflow-auto">{bt.output}</pre> : null}
                </div>
                {bt.status === 'running' ? (
                  <Button
                    aria-label="Cancel background task"
                    className="p-2 rounded-lg hover:bg-error/10 text-error cursor-pointer"
                    onClick={() => onCancel(bt.task_id)}
                  >
                    <span className="material-symbols-outlined" aria-hidden="true">stop_circle</span>
                  </Button>
                ) : null}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
}

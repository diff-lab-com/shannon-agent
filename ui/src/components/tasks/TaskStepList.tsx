// Live task step list (P1.2).
//
// Renders the stream of task-step + task-retry events captured by
// useTaskStreaming. Used inside task-detail panels to show real-time
// progress (which step is running, which failed, how many retries).

import { useIntl } from 'react-intl'
import { useTaskStreaming } from '@/hooks/useTaskStreaming'

interface TaskStepListProps {
  taskId: string
  /** When false, the hook will stop tracking this task. */
  active?: boolean
}

const STATUS_ICON: Record<'started' | 'completed' | 'failed', string> = {
  started: 'play_circle',
  completed: 'check_circle',
  failed: 'cancel',
}

const STATUS_COLOR: Record<'started' | 'completed' | 'failed', string> = {
  started: 'text-primary',
  completed: 'text-tertiary',
  failed: 'text-error',
}

export default function TaskStepList({ taskId, active = true }: TaskStepListProps) {
  const intl = useIntl()
  const { streams } = useTaskStreaming(active ? [taskId] : [])

  const state = streams.get(taskId)
  if (!state || (state.steps.length === 0 && state.retries.length === 0)) {
    return (
      <p className="font-body-sm text-on-surface-variant italic">
        {intl.formatMessage({ id: 'tasks.steps.empty' })}
      </p>
    )
  }

  return (
    <div className="flex flex-col gap-xs">
      {state.latestRetry && (
        <div className="flex items-center gap-sm p-sm bg-warning/10 border border-warning/20 rounded-lg font-label-sm text-warning">
          <span className="material-symbols-outlined text-[16px]">replay</span>
          <span>
            {intl.formatMessage(
              { id: 'tasks.steps.retrying' },
              {
                attempt: state.latestRetry.attempt,
                max: state.latestRetry.maxAttempts,
              },
            )}
          </span>
          <span className="opacity-70 ml-auto truncate">
            {state.latestRetry.lastError}
          </span>
        </div>
      )}
      <ol className="flex flex-col gap-xs">
        {state.steps.map((step, idx) => (
          <li
            key={`${step.stepIndex}-${idx}`}
            className="flex items-start gap-sm p-sm bg-surface-container-low rounded-lg"
          >
            <span className={`material-symbols-outlined text-[18px] ${STATUS_COLOR[step.status]}`}>
              {step.status === 'started' ? 'progress_activity' : STATUS_ICON[step.status]}
            </span>
            <div className="flex-1 min-w-0">
              <div className="font-label-md text-on-surface truncate flex items-center gap-xs">
                <span>{step.stepLabel}</span>
                {step.stepTotal > 0 && (
                  <span className="font-label-sm text-on-surface-variant">
                    {step.stepIndex}/{step.stepTotal}
                  </span>
                )}
              </div>
              {step.error && (
                <p className="font-body-sm text-error mt-xs opacity-80">{step.error}</p>
              )}
            </div>
            <time className="font-label-sm text-outline shrink-0">
              {new Date(step.timestampMs).toLocaleTimeString()}
            </time>
          </li>
        ))}
      </ol>
    </div>
  )
}

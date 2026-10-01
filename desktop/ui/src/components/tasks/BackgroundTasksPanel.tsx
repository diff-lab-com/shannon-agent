// BackgroundTasksPanel — P1-3 (G2b): surface in-flight background tasks on
// the Runs tab.
//
// created-then-invisible, half ①: background tasks used to be visible only
// in the dev-only Pipelines tab, so a Simple-mode user who started one saw
// nothing. This panel renders the in-flight slice of the catalog's
// `backgroundTasks` (name / status / elapsed / stop) in every mode. The
// catalog already refreshes on the backend's `background-tasks-updated`
// event, so start/cancel/finish keep the list live without polling; a slow
// local tick only keeps the elapsed column honest between events.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { useCatalog } from '@/context/CatalogContext'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import type { BackgroundTaskInfo } from '@/types'

/** Compact elapsed label for a started-at epoch-ms timestamp ("2:05", "1:02:03"). */
export function formatElapsed(startedAtMs: number, nowMs: number): string {
  const totalSeconds = Math.max(0, Math.floor((nowMs - startedAtMs) / 1000))
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  const pad = (n: number) => String(n).padStart(2, '0')
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(seconds)}`
    : `${minutes}:${pad(seconds)}`
}

/** The in-flight slice: only tasks the backend still reports as running. */
export function runningBackgroundTasks(tasks: BackgroundTaskInfo[]): BackgroundTaskInfo[] {
  return tasks.filter(t => t.status === 'running')
}

export default function BackgroundTasksPanel() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const { backgroundTasks } = useCatalog()
  const [stoppingId, setStoppingId] = useState<string | null>(null)
  // Slow tick so the elapsed column advances between backend events.
  const [, setTick] = useState(0)
  useEffect(() => {
    const timer = setInterval(() => setTick(n => n + 1), 5_000)
    return () => clearInterval(timer)
  }, [])

  const running = runningBackgroundTasks(backgroundTasks)
  if (running.length === 0) return null

  const stop = async (taskId: string) => {
    if (stoppingId) return
    setStoppingId(taskId)
    try {
      await api.cancelBackgroundTask(taskId)
      toast.success(t('tasks.toast.cancelled'))
    } catch (e) {
      toastError(t('tasks.backgroundPanel.stopFailed'), e)
    } finally {
      setStoppingId(null)
    }
  }

  return (
    <section
      aria-label={t('tasks.backgroundPanel.title')}
      data-testid="background-tasks-panel"
      className="bg-surface-container-lowest border border-primary/20 rounded-xl p-md mb-lg"
    >
      <div className="flex items-center gap-sm mb-sm">
        <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">bolt</span>
        <h3 className="font-body-md font-bold text-on-surface m-0">
          {t('tasks.backgroundPanel.title')}
        </h3>
        <span className="font-label-sm text-label-xs text-on-surface-variant">
          {intl.formatMessage({ id: 'tasks.backgroundPanel.count' }, { count: running.length })}
        </span>
      </div>
      <ul className="flex flex-col gap-xs m-0 p-0 list-none">
        {running.map(task => (
          <li
            key={task.task_id}
            data-testid="background-task-row"
            className="flex items-center gap-md px-sm py-xs rounded-lg bg-surface-container-low/60"
          >
            <span className="material-symbols-outlined icon-sm text-primary" aria-hidden="true">autorenew</span>
            <span className="font-label-md text-on-surface flex-1 min-w-0 truncate" title={task.prompt}>
              {task.prompt}
            </span>
            <span className="font-label-sm text-label-xs text-primary whitespace-nowrap">
              {t('tasks.backgroundPanel.statusRunning')}
            </span>
            <span
              className="font-label-sm text-label-xs text-on-surface-variant whitespace-nowrap tabular-nums"
              aria-label={t('tasks.backgroundPanel.elapsedAria')}
              title={new Date(task.started_at).toLocaleTimeString()}
            >
              {formatElapsed(task.started_at, Date.now())}
            </span>
            <Button
              variant="outline"
              type="button"
              size="sm"
              disabled={stoppingId !== null}
              aria-busy={stoppingId === task.task_id || undefined}
              onClick={() => void stop(task.task_id)}
              data-testid={`background-task-stop-${task.task_id}`}
              aria-label={t('tasks.backgroundPanel.stopAria')}
              className="rounded-lg px-sm py-xs"
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">stop_circle</span>
              {stoppingId === task.task_id ? '…' : t('tasks.backgroundPanel.stop')}
            </Button>
          </li>
        ))}
      </ul>
    </section>
  )
}

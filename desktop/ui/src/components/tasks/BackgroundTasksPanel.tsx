// BackgroundTasksPanel — P1-3 (G2b): surface in-flight background tasks on
// the Runs tab; R2-P1-5 (W2-5): keep recently *finished* ones visible too.
//
// created-then-invisible, half ①: background tasks used to be visible only
// in the dev-only Pipelines tab, so a Simple-mode user who started one saw
// nothing. This panel renders the in-flight slice of the catalog's
// `backgroundTasks` (name / status / elapsed / stop) in every mode. The
// catalog already refreshes on the backend's `background-tasks-updated`
// event, so start/cancel/finish keep the list live without polling; a slow
// local tick only keeps the elapsed column honest between events.
//
// created-then-invisible, half ②: before W2-5 a finished task evaporated
// from the UI the moment its status flipped (the panel filtered to
// `running`). The backend's `get_background_tasks` returns the whole
// in-memory table (terminal rows included; history is per-session and
// intentionally not persisted across restarts), so the panel now shows a
// "recent" slice of terminal rows — completed / failed / cancelled, with
// duration and the failure summary — capped at the newest
// MAX_TERMINAL_ROWS. Failed tasks additionally land a Triage inbox item
// (backend, `source = background_task`) for a cross-page entry; successes
// stay panel-only to keep the inbox noise-free.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { useCatalog } from '@/context/CatalogContext'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import type { BackgroundTaskInfo } from '@/types'

/** Newest terminal rows the panel keeps visible (in-memory slice only). */
export const MAX_TERMINAL_ROWS = 10

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

/** Terminal status vocabulary mirrored from the backend's `BackgroundTaskMeta`. */
export function isTerminalTaskStatus(status: string): boolean {
  return status === 'completed' || status === 'failed' || status === 'cancelled'
}

/**
 * The recent-terminal slice: newest first, capped at `max` rows so a long
 * session can't grow the panel without bound. Ordering keys off
 * `completed_at` (set by the backend's finalize/cancel paths); rows without
 * one sort last defensively.
 */
export function terminalBackgroundTasks(
  tasks: BackgroundTaskInfo[],
  max: number = MAX_TERMINAL_ROWS,
): BackgroundTaskInfo[] {
  return tasks
    .filter(t => isTerminalTaskStatus(t.status))
    .sort((a, b) => (b.completed_at ?? 0) - (a.completed_at ?? 0))
    .slice(0, max)
}

/** Icon + i18n key per terminal status (text label keeps status off color-only). */
function terminalStatusMeta(status: string): { icon: string; labelId: string; className: string } {
  switch (status) {
    case 'failed':
      return { icon: 'error', labelId: 'tasks.backgroundPanel.statusFailed', className: 'text-error' }
    case 'cancelled':
      return { icon: 'cancel', labelId: 'tasks.backgroundPanel.statusCancelled', className: 'text-on-surface-variant' }
    default:
      return { icon: 'check_circle', labelId: 'tasks.backgroundPanel.statusCompleted', className: 'text-on-surface-variant' }
  }
}

/** Last non-empty line of the output — where stream failures put their summary. */
function errorHeadline(output: string): string {
  return output
    .split('\n')
    .reverse()
    .map(l => l.trim())
    .find(l => l.length > 0) ?? ''
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
  const terminal = terminalBackgroundTasks(backgroundTasks)
  if (running.length === 0 && terminal.length === 0) return null

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
      {running.length > 0 && (
        <>
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
        </>
      )}
      {terminal.length > 0 && (
        <>
          <h4 className="font-label-md font-bold text-on-surface-variant m-0 mt-sm mb-xs">
            {t('tasks.backgroundPanel.recent')}
          </h4>
          <ul className="flex flex-col gap-xs m-0 p-0 list-none" data-testid="background-tasks-terminal">
            {terminal.map(task => {
              const meta = terminalStatusMeta(task.status)
              return (
                <li
                  key={task.task_id}
                  data-testid="background-task-terminal-row"
                  className="flex items-center gap-md px-sm py-xs rounded-lg bg-surface-container-low/40"
                >
                  <span className={`material-symbols-outlined icon-sm ${meta.className}`} aria-hidden="true">
                    {meta.icon}
                  </span>
                  <span className="flex flex-col flex-1 min-w-0">
                    <span className="font-label-md text-on-surface truncate" title={task.prompt}>
                      {task.prompt}
                    </span>
                    {task.status === 'failed' && errorHeadline(task.output) && (
                      <span
                        className="font-label-sm text-label-xs text-error truncate"
                        title={task.output}
                        aria-label={t('tasks.backgroundPanel.errorSummaryAria')}
                      >
                        {errorHeadline(task.output)}
                      </span>
                    )}
                  </span>
                  <span className={`font-label-sm text-label-xs whitespace-nowrap ${meta.className}`}>
                    {t(meta.labelId)}
                  </span>
                  {task.completed_at != null && (
                    <span
                      className="font-label-sm text-label-xs text-on-surface-variant whitespace-nowrap tabular-nums"
                      aria-label={t('tasks.backgroundPanel.durationAria')}
                    >
                      {formatElapsed(task.started_at, task.completed_at)}
                    </span>
                  )}
                </li>
              )
            })}
          </ul>
        </>
      )}
    </section>
  )
}

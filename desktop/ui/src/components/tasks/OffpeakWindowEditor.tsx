// OffpeakWindowEditor — P2-5.
//
// Editor + status for a routine's off-peak execution window
// (`policy.execution_window`). The window uses inclusive wall-clock hours in
// the given timezone; start > end wraps past midnight (e.g. 22→6). Clearing
// the toggle removes the window — the routine then executes immediately when
// due (legacy behavior).
//
// Also surfaces the queued state: when the routine's most recent run is
// `queued` (due but outside the window), the current status line reads
// "Queued (off-peak 22:00–06:00)".

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import type { ExecutionPolicy, ExecutionWindow, ScheduledRoutine, TaskExecution } from '@/types'

interface OffpeakWindowEditorProps {
  routine: ScheduledRoutine
  onUpdated?: (routine: ScheduledRoutine) => void
}

const clampHour = (v: number): number => Math.min(23, Math.max(0, Math.round(v) || 0))

const pad2 = (h: number): string => String(h).padStart(2, '0')

/** "22:00–06:00" window label from a window object. */
export function windowLabel(w: ExecutionWindow): string {
  return `${pad2(w.start_hour)}:00–${pad2(w.end_hour)}:00`
}

export default function OffpeakWindowEditor({ routine, onUpdated }: OffpeakWindowEditorProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const initial = routine.policy?.execution_window ?? null
  const [enabled, setEnabled] = useState(initial !== null)
  const [startHour, setStartHour] = useState(initial?.start_hour ?? 22)
  const [endHour, setEndHour] = useState(initial?.end_hour ?? 6)
  const [timezone, setTimezone] = useState(initial?.timezone ?? '')
  const [saving, setSaving] = useState(false)

  const localTimeZone = useMemo(() => {
    try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC' } catch { return 'UTC' }
  }, [])

  // Queued visibility: the routine's most recent execution record decides
  // whether we render the "Queued (off-peak …)" status.
  const [lastRun, setLastRun] = useState<TaskExecution | null>(null)
  useEffect(() => {
    let cancelled = false
    api.listTaskExecutions(routine.id, 1)
      .then(rows => { if (!cancelled) setLastRun(rows[0] ?? null) })
      .catch(() => { if (!cancelled) setLastRun(null) })
    return () => { cancelled = true }
  }, [routine.id, routine.last_run_id])

  const isQueued = lastRun?.status === 'queued'
  const dirty = useMemo(() => {
    const next: ExecutionWindow | null = enabled
      ? { start_hour: clampHour(startHour), end_hour: clampHour(endHour), timezone: timezone.trim() || null }
      : null
    if (next === null && initial === null) return false
    if (next === null || initial === null) return true
    return (
      next.start_hour !== initial.start_hour ||
      next.end_hour !== initial.end_hour ||
      (next.timezone ?? null) !== (initial.timezone ?? null)
    )
  }, [enabled, startHour, endHour, timezone, initial])

  const save = async () => {
    setSaving(true)
    try {
      const execution_window: ExecutionWindow | null = enabled
        ? { start_hour: clampHour(startHour), end_hour: clampHour(endHour), timezone: timezone.trim() || null }
        : null
      // `update_scheduled_task` replaces the policy wholesale — send the
      // full object with only execution_window changed.
      const policy: ExecutionPolicy = {
        max_retries: 0,
        timeout_secs: 0,
        worktree: null,
        notify_on_failure: false,
        budget_usd: null,
        auto_archive_when_empty: true,
        ...routine.policy,
        execution_window,
      }
      const updated = await api.updateScheduledTask({ id: routine.id, policy })
      toast.success(t('tasks.offpeakEditor.saved'))
      onUpdated?.(updated)
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.offpeakEditor.saveFailed')
      toast.error(msg)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="rounded-xl border border-secondary/20 bg-secondary/5 p-md flex flex-col gap-sm">
      <div className="flex items-center justify-between gap-md">
        <span className="font-label-md text-on-surface font-semibold">
          {t('tasks.offpeakEditor.title')}
        </span>
        {initial ? (
          <span
            className="inline-flex items-center gap-1 px-sm py-0.5 rounded-full border border-secondary/30 bg-secondary-container text-on-secondary-container text-[11px] font-bold"
            title={t('tasks.offpeakEditor.badgeTip', { window: windowLabel(initial) })}
          >
            <span className="material-symbols-outlined icon-xs" aria-hidden="true">bedtime</span>
            {intl.formatMessage(
              { id: 'tasks.offpeakEditor.badge' },
              { window: windowLabel(initial) },
            )}
          </span>
        ) : null}
      </div>

      {isQueued ? (
        <div
          role="status"
          className="font-label-sm text-[12px] text-secondary flex items-center gap-xs"
        >
          <span className="material-symbols-outlined icon-sm" aria-hidden="true">hourglass_top</span>
          {intl.formatMessage(
            { id: 'tasks.offpeakEditor.queuedStatus' },
            { window: initial ? windowLabel(initial) : '' },
          )}
        </div>
      ) : null}

      <label className="flex items-center gap-sm cursor-pointer">
        <input
          type="checkbox"
          className="w-4 h-4 accent-primary cursor-pointer"
          checked={enabled}
          onChange={e => setEnabled(e.target.checked)}
          aria-label={t('tasks.offpeakEditor.toggleAria')}
        />
        <span className="font-label-md text-on-surface">{t('tasks.offpeakEditor.enabled')}</span>
      </label>
      <p className="font-label-sm text-[11px] text-on-surface-variant leading-tight">
        {t('tasks.offpeakEditor.hint')}
      </p>

      {enabled ? (
        <div className="grid grid-cols-2 gap-sm">
          <label className="flex flex-col gap-xs">
            <span className="font-label-sm text-[11px] text-on-surface-variant">
              {t('tasks.offpeakEditor.startHour')}
            </span>
            <input
              type="number"
              min={0}
              max={23}
              value={startHour}
              onChange={e => setStartHour(clampHour(Number(e.target.value)))}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
              aria-label={t('tasks.offpeakEditor.startHour')}
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-sm text-[11px] text-on-surface-variant">
              {t('tasks.offpeakEditor.endHour')}
            </span>
            <input
              type="number"
              min={0}
              max={23}
              value={endHour}
              onChange={e => setEndHour(clampHour(Number(e.target.value)))}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
              aria-label={t('tasks.offpeakEditor.endHour')}
            />
          </label>
          <label className="flex flex-col gap-xs col-span-2">
            <span className="font-label-sm text-[11px] text-on-surface-variant">
              {t('tasks.offpeakEditor.timezone')}
            </span>
            <input
              type="text"
              placeholder={localTimeZone}
              value={timezone}
              onChange={e => setTimezone(e.target.value)}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
              aria-label={t('tasks.offpeakEditor.timezone')}
            />
            <span className="font-label-sm text-[10px] text-on-surface-variant">
              {intl.formatMessage({ id: 'tasks.offpeakEditor.timezoneHint' }, { zone: localTimeZone })}
            </span>
          </label>
        </div>
      ) : null}

      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          onClick={save}
          disabled={!dirty || saving}
          aria-label={t('tasks.offpeakEditor.saveAria')}
          className="rounded-lg"
        >
          <span className="material-symbols-outlined text-[18px]" aria-hidden="true">save</span>
          {saving ? t('tasks.offpeakEditor.saving') : t('tasks.offpeakEditor.save')}
        </Button>
      </div>
    </div>
  )
}

// RoutineBasicsEditor — P1-1 (G2b): edit a routine's name, prompt and
// trigger from the detail drawer.
//
// The create flow owns ScheduleForm; editing reuses its field vocabulary
// (same labels, same live cron preview) but persists through the
// field-patching `update_scheduled_task` command instead — only the changed
// fields are sent, mirroring how DependsOnEditor / OffpeakWindowEditor save.
// On failure the local edits are kept (nothing resets), so the user can
// retry or copy their work out.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'
import type { ScheduledRoutine, TriggerType } from '@/types'

interface RoutineBasicsEditorProps {
  routine: ScheduledRoutine
  onUpdated?: () => void
}

const TRIGGER_OPTIONS: { value: TriggerType; icon: string }[] = [
  { value: 'interval', icon: 'timer' },
  { value: 'cron', icon: 'schedule' },
  { value: 'webhook', icon: 'webhook' },
  { value: 'event', icon: 'bolt' },
]

export default function RoutineBasicsEditor({ routine, onUpdated }: RoutineBasicsEditorProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [name, setName] = useState(routine.name)
  const [prompt, setPrompt] = useState(routine.prompt)
  const [triggerType, setTriggerType] = useState<TriggerType>(routine.trigger_type)
  const [intervalSecs, setIntervalSecs] = useState(routine.interval_secs || 3600)
  const [cronExpr, setCronExpr] = useState(routine.cron_expr ?? '0 9 * * *')
  const [saving, setSaving] = useState(false)

  // Live cron validity — same debounce + preview command as ScheduleForm.
  const [cronPreview, setCronPreview] = useState<{ valid: boolean; error?: string } | null>(null)
  useEffect(() => {
    if (triggerType !== 'cron' || !cronExpr.trim()) {
      setCronPreview(null)
      return
    }
    let cancelled = false
    setCronPreview(null)
    const timer = setTimeout(() => {
      api.previewCron(cronExpr.trim())
        .then(p => { if (!cancelled) setCronPreview(p) })
        .catch(e => { if (!cancelled) setCronPreview({ valid: false, error: String(e) }) })
    }, 350)
    return () => { cancelled = true; clearTimeout(timer) }
  }, [cronExpr, triggerType])

  const dirty =
    name.trim() !== routine.name ||
    prompt.trim() !== routine.prompt ||
    triggerType !== routine.trigger_type ||
    (triggerType === 'interval' && intervalSecs !== routine.interval_secs) ||
    (triggerType === 'cron' && cronExpr.trim() !== (routine.cron_expr ?? ''))

  const valid =
    name.trim().length > 0 &&
    prompt.trim().length > 0 &&
    (
      triggerType === 'webhook' ||
      triggerType === 'event' ||
      (triggerType === 'interval' && intervalSecs > 0) ||
      (triggerType === 'cron' && cronExpr.trim().length > 0 && cronPreview?.valid === true)
    )

  const save = async () => {
    if (saving || !dirty || !valid) return
    setSaving(true)
    try {
      await api.updateScheduledTask({
        id: routine.id,
        name: name.trim(),
        prompt: prompt.trim(),
        trigger_type: triggerType,
        ...(triggerType === 'interval' ? { interval_secs: intervalSecs } : {}),
        ...(triggerType === 'cron' ? { cron_expr: cronExpr.trim() } : {}),
      })
      toast.success(t('tasks.routineBasicsEditor.saved'))
      onUpdated?.()
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.routineBasicsEditor.saveFailed')
      toast.error(msg)
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="rounded-xl border border-primary/20 bg-primary/5 p-md flex flex-col gap-sm">
      <span className="font-label-md text-on-surface font-semibold">
        {t('tasks.routineBasicsEditor.title')}
      </span>

      <label className="flex flex-col gap-xs">
        <span className="font-label-sm text-label-xs text-on-surface-variant">
          {t('tasks.scheduleForm.nameLabel')}
        </span>
        <input
          type="text"
          value={name}
          onChange={e => setName(e.target.value)}
          data-testid="routine-basics-name"
          className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
      </label>

      <label className="flex flex-col gap-xs">
        <span className="font-label-sm text-label-xs text-on-surface-variant">
          {t('tasks.scheduleForm.promptLabel')}
        </span>
        <textarea
          className="w-full h-20 p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
          value={prompt}
          onChange={e => setPrompt(e.target.value)}
          data-testid="routine-basics-prompt"
        />
      </label>

      <fieldset className="flex flex-col gap-xs">
        <legend className="font-label-sm text-label-xs text-on-surface-variant mb-xs">
          {t('tasks.scheduleForm.triggerType')}
        </legend>
        <div className="grid grid-cols-2 gap-sm" role="radiogroup" aria-label={t('tasks.scheduleForm.triggerType')}>
          {TRIGGER_OPTIONS.map(opt => {
            const selected = triggerType === opt.value
            return (
              <Button
                key={opt.value}
                variant="outline"
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => setTriggerType(opt.value)}
                className={cn('h-auto px-sm py-xs rounded-lg gap-xs',
                  selected
                    ? 'border-primary bg-primary/10 text-on-surface hover:bg-primary/10'
                    : 'border-outline-variant/30 bg-surface-container-low text-on-surface-variant hover:bg-surface-container-low/60'
                )}
              >
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">{opt.icon}</span>
                <span className="font-label-sm font-bold">{t(`tasks.scheduleForm.type.${opt.value}`)}</span>
              </Button>
            )
          })}
        </div>
      </fieldset>

      {triggerType === 'interval' ? (
        <label className="flex flex-col gap-xs">
          <span className="font-label-sm text-label-xs text-on-surface-variant">
            {t('tasks.scheduleForm.intervalSeconds')}
          </span>
          <input
            type="number"
            min={1}
            value={intervalSecs}
            onChange={e => setIntervalSecs(Math.max(1, Number(e.target.value) || 0))}
            data-testid="routine-basics-interval"
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
          <span className="font-label-sm text-label-2xs text-on-surface-variant">
            {intl.formatMessage({ id: 'tasks.scheduleForm.intervalHint' }, { mins: Math.round(intervalSecs / 60), hrs: Math.round(intervalSecs / 3600) })}
          </span>
        </label>
      ) : null}

      {triggerType === 'cron' ? (
        <div className="flex flex-col gap-xs">
          <label className="flex flex-col gap-xs">
            <span className="font-label-sm text-label-xs text-on-surface-variant">
              {t('tasks.scheduleForm.cronExpression')}
            </span>
            <input
              type="text"
              value={cronExpr}
              onChange={e => setCronExpr(e.target.value)}
              data-testid="routine-basics-cron"
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          {cronPreview ? (
            cronPreview.valid ? (
              <div className="font-label-sm text-label-xs text-primary flex items-center gap-xs">
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">check_circle</span>
                {t('tasks.scheduleForm.validCron')}
              </div>
            ) : (
              <div className="font-label-sm text-label-xs text-error flex items-center gap-xs" role="alert">
                <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
                {cronPreview.error ?? t('tasks.scheduleForm.invalidCron')}
              </div>
            )
          ) : (
            <span className="font-label-sm text-label-xs text-on-surface-variant">
              {t('tasks.scheduleForm.checking')}
            </span>
          )}
        </div>
      ) : null}

      <div className="flex justify-end">
        <Button
          type="button"
          size="sm"
          onClick={() => void save()}
          disabled={!dirty || !valid || saving}
          aria-busy={saving || undefined}
          data-testid="routine-basics-save"
          className="rounded-lg"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">save</span>
          {saving ? t('tasks.routineBasicsEditor.saving') : t('tasks.routineBasicsEditor.save')}
        </Button>
      </div>
    </div>
  )
}

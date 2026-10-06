// RoutineBasicsEditor — P1-1 (G2b): edit a routine's name, prompt and
// trigger from the detail drawer.
//
// The create flow owns ScheduleForm; editing reuses its field vocabulary
// (same labels, same live cron preview) but persists through the
// field-patching `update_scheduled_task` command instead — only the changed
// fields are sent, mirroring how DependsOnEditor / OffpeakWindowEditor save.
// On failure the local edits are kept (nothing resets), so the user can
// retry or copy their work out.
//
// W3-1: saving is a two-step flow, mirroring creation — "Save" opens a
// structured review of the edited routine and an explicit Activate issues
// the update. "Back to edit" returns with every local edit intact.

import { useEffect, useRef, useState, type ReactNode } from 'react'
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
  // W3-1: the edit path confirms too — `save` only opens the review step;
  // Activate is the sole path to `update_scheduled_task`.
  const [step, setStep] = useState<'edit' | 'review'>('edit')
  const reviewHeadingRef = useRef<HTMLHeadingElement>(null)

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

  // W3-1: focus lands on the review heading when the confirm step opens, so
  // keyboard and screen-reader users meet the summary before any button.
  useEffect(() => {
    if (step === 'review') reviewHeadingRef.current?.focus()
  }, [step])

  return (
    <div className="rounded-xl border border-primary/20 bg-primary/5 p-md flex flex-col gap-sm">
      {step === 'review' ? (
        /* W3-1: the edit path confirms too — this preview is built from the
            same live state Activate submits to update_scheduled_task. */
        <section
          aria-labelledby="routine-review-title"
          aria-describedby="routine-review-intro"
          data-testid="routine-basics-review"
          className="flex flex-col gap-sm"
        >
          <h3
            id="routine-review-title"
            ref={reviewHeadingRef}
            tabIndex={-1}
            className="font-label-md text-on-surface font-semibold focus:outline-none"
          >
            {t('tasks.scheduleForm.reviewTitle')}
          </h3>
          <p id="routine-review-intro" className="font-label-sm text-label-sm text-on-surface-variant">
            {t('tasks.scheduleForm.reviewIntro')}
          </p>
          <dl className="flex flex-col gap-sm">
            <EditorReviewRow label={t('tasks.scheduleForm.reviewSection.name')}>
              <span className="font-body-sm font-medium text-on-surface break-words">{name.trim()}</span>
            </EditorReviewRow>
            <EditorReviewRow label={t('tasks.scheduleForm.reviewSection.trigger')}>
              <div className="flex flex-col gap-xs">
                <span className="font-body-sm font-medium text-on-surface">
                  {t(`tasks.scheduleForm.type.${triggerType}`)}
                </span>
                {triggerType === 'interval' && (
                  <span className="font-label-sm text-label-xs text-on-surface-variant">
                    {intervalSecs}s ·{' '}
                    {intl.formatMessage({ id: 'tasks.scheduleForm.intervalHint' }, { mins: Math.round(intervalSecs / 60), hrs: Math.round(intervalSecs / 3600) })}
                  </span>
                )}
                {triggerType === 'cron' && (
                  <>
                    <span className="font-body-sm font-mono text-on-surface break-all">{cronExpr.trim()}</span>
                    {cronPreview?.valid && (
                      <span className="font-label-sm text-label-xs text-on-surface-variant">
                        {t('tasks.scheduleForm.validCron')}
                      </span>
                    )}
                  </>
                )}
              </div>
            </EditorReviewRow>
            <EditorReviewRow label={t('tasks.scheduleForm.reviewSection.prompt')}>
              <span className="font-body-sm text-on-surface whitespace-pre-wrap break-words">{prompt.trim()}</span>
            </EditorReviewRow>
          </dl>
        </section>
      ) : (
        <>
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

        </>
      )}

      {step === 'edit' ? (
      <div className="flex justify-end">
        {/* W3-1: "Save" opens the structured review step; it never persists
            by itself — Activate below issues the update. */}
        <Button
          type="button"
          size="sm"
          onClick={() => setStep('review')}
          disabled={!dirty || !valid || saving}
          aria-busy={saving || undefined}
          data-testid="routine-basics-save"
          className="rounded-lg"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">save</span>
          {t('tasks.routineBasicsEditor.save')}
        </Button>
      </div>
      ) : (
      <div className="flex justify-end gap-sm">
        {/* All edits live in this component's state — going back keeps them. */}
        <Button
          type="button"
          size="sm"
          variant="ghost"
          onClick={() => setStep('edit')}
          data-testid="routine-basics-back"
          className="rounded-lg"
        >
          {t('tasks.scheduleForm.backToEdit')}
        </Button>
        <Button
          type="button"
          size="sm"
          onClick={() => void save()}
          disabled={saving}
          aria-busy={saving || undefined}
          data-testid="routine-basics-activate"
          className="rounded-lg"
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">bolt</span>
          {saving ? t('tasks.routineBasicsEditor.saving') : t('tasks.scheduleForm.activate')}
        </Button>
      </div>
      )}
    </div>
  )
}

/** W3-1: one label/value row of the editor's structured review grid. */
function EditorReviewRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(6rem,auto)_1fr] gap-sm items-start">
      <dt className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider pt-0.5">{label}</dt>
      <dd className="min-w-0 flex flex-col gap-xs">{children}</dd>
    </div>
  )
}

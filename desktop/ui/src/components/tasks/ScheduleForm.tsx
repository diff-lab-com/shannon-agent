// ScheduleForm — creates a scheduled routine via Tauri create_scheduled_task.
//
// Covers Phase D P2.4 (webhook trigger) and P2.6 (retry policy exposure)
// in one unified form. All four trigger types are selectable; policy fields
// are optional with MD3-styled inputs. Live cron preview uses preview_cron.
//
// W3-1: creation is a two-step flow — the form fills first, then a
// structured review step ("Shannon will run this exactly as shown") must be
// confirmed with an explicit Activate before `onSubmit` ever fires. A
// routine is a recurring cost, so nothing is created without the user
// having seen the full timing/prompt/notification/policy preview, built
// from the very same form state that produces the payload.

import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import ScheduleTemplates from './ScheduleTemplates'
import CostEstimateHint from './CostEstimateHint'
import { weekdayName, DEFAULT_POLICY } from './shared'
import { parseNlCron, type CronDescription } from '@/lib/nl-cron'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'
import type {
  TriggerType,
  ExecutionPolicy,
  CreateTaskPayload,
  CronPreview,
} from '@/types'

interface ScheduleFormProps {
  onSubmit: (payload: CreateTaskPayload) => void | Promise<unknown>
  onCancel: () => void
}

type TriggerOption = {
  value: TriggerType
  icon: string
}

/** W3-1: the two form steps — fill, then review-and-activate. */
type FormStep = 'edit' | 'review'

const clampHour = (v: number): number => Math.min(23, Math.max(0, Math.round(v) || 0))

// B6-36: labels/hints used to be hardcoded English; resolve them per trigger
// type through the locale files at render time instead.
const TRIGGER_OPTIONS: TriggerOption[] = [
  { value: 'interval', icon: 'timer' },
  { value: 'cron', icon: 'schedule' },
  { value: 'webhook', icon: 'webhook' },
  { value: 'event', icon: 'bolt' },
]

export default function ScheduleForm({ onSubmit, onCancel }: ScheduleFormProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

  const [name, setName] = useState('')
  const [prompt, setPrompt] = useState('')
  const [triggerType, setTriggerType] = useState<TriggerType>('interval')
  const [intervalSecs, setIntervalSecs] = useState(3600)
  const [cronExpr, setCronExpr] = useState('0 9 * * *')
  const [maxFires, setMaxFires] = useState<number | ''>('')
  const [showPolicy, setShowPolicy] = useState(false)
  const [policy, setPolicy] = useState<ExecutionPolicy>(DEFAULT_POLICY)
  // P2-5: off-peak execution window. Disabled by default — a routine
  // without a window executes immediately when due (legacy behavior).
  const [offpeakEnabled, setOffpeakEnabled] = useState(false)
  const [windowStart, setWindowStart] = useState(22)
  const [windowEnd, setWindowEnd] = useState(6)
  const [windowTz, setWindowTz] = useState('')
  const localTimeZone = useMemo(() => {
    try { return Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC' } catch { return 'UTC' }
  }, [])
  const [cronPreview, setCronPreview] = useState<CronPreview | null>(null)
  const [cronLoading, setCronLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [nlInput, setNlInput] = useState('')
  const [nlError, setNlError] = useState<string | null>(null)
  const [nlMatch, setNlMatch] = useState<CronDescription | null>(null)
  // B3 P1-24: create is awaited before the busy flag drops — double clicking
  // used to schedule the same routine twice (and routines re-fire on their
  // cadence, so a duplicate is a recurring cost, not a one-off).
  const [submitting, setSubmitting] = useState(false)
  // W3-1: nothing is created straight from the form — `submit` only moves to
  // the structured review step; the Activate button there is the sole path
  // to `onSubmit`.
  const [step, setStep] = useState<FormStep>('edit')
  const reviewHeadingRef = useRef<HTMLHeadingElement>(null)

  // B6' routing: copy the run-finished notification to the configured
  // webhook. Default off; the "no webhook configured" hint only renders on
  // a CONFIRMED negative probe — not while the probe is in flight.
  const [notifyWebhook, setNotifyWebhook] = useState(false)
  const [webhookConfigured, setWebhookConfigured] = useState<boolean | null>(null)
  useEffect(() => {
    let cancelled = false
    api.getWebhookConfig()
      .then(cfg => { if (!cancelled) setWebhookConfigured(Boolean(cfg?.url?.trim())) })
      .catch(() => { if (!cancelled) setWebhookConfigured(false) })
    return () => { cancelled = true }
  }, [])

  // Live cron preview (debounced via requestIdleCallback-free simple effect)
  useEffect(() => {
    if (triggerType !== 'cron' || !cronExpr.trim()) {
      setCronPreview(null)
      return
    }
    let cancelled = false
    setCronLoading(true)
    const timer = setTimeout(() => {
      api.previewCron(cronExpr.trim())
        .then(p => { if (!cancelled) setCronPreview(p) })
        .catch(e => { if (!cancelled) setCronPreview({ expression: cronExpr, valid: false, error: String(e), next_fires: [] }) })
        .finally(() => { if (!cancelled) setCronLoading(false) })
    }, 350)
    return () => { cancelled = true; clearTimeout(timer) }
  }, [cronExpr, triggerType])

  const valid = name.trim() && prompt.trim() && (
    triggerType === 'webhook' ||
    triggerType === 'event' ||
    (triggerType === 'interval' && intervalSecs > 0) ||
    (triggerType === 'cron' && cronExpr.trim() && cronPreview?.valid)
  )

  // P2-5: clamp the window hours into 0..=23 and null out the timezone when
  // the user left it empty (= machine-local timezone, per the contract).
  const buildPolicy = (): ExecutionPolicy => ({
    ...policy,
    execution_window: offpeakEnabled
      ? {
          start_hour: clampHour(windowStart),
          end_hour: clampHour(windowEnd),
          timezone: windowTz.trim() ? windowTz.trim() : null,
        }
      : null,
  })

  // W3-1: the review step renders from live state and Activate submits the
  // exact payload this builds — one source of truth, so the preview can
  // never describe a different routine than the one that gets created.
  const buildPayload = (): CreateTaskPayload => ({
    name: name.trim(),
    prompt: prompt.trim(),
    trigger_type: triggerType,
    ...(triggerType === 'interval' ? { interval_secs: intervalSecs } : {}),
    ...(triggerType === 'cron' ? { cron_expr: cronExpr.trim() } : {}),
    ...(maxFires !== '' ? { max_fires: maxFires } : {}),
    policy: buildPolicy(),
    // B6' routing — always explicit (false = no webhook copy) so the
    // backend never guesses the default.
    notify_webhook: notifyWebhook,
  })

  // W3-1 focus management: landing on the review step moves keyboard/screen-
  // reader focus to its heading, so the confirmation is announced before any
  // button is reached.
  useEffect(() => {
    if (step === 'review') reviewHeadingRef.current?.focus()
  }, [step])

  // Edit-step CTA: validates and enters the review step — it never creates
  // anything by itself.
  const beginReview = () => {
    if (submitting) return
    if (!valid) {
      setError(t('tasks.scheduleForm.requiredFields'))
      return
    }
    setError(null)
    setStep('review')
  }

  const activate = async () => {
    if (submitting) return
    setError(null)
    const payload: CreateTaskPayload = buildPayload()
    setSubmitting(true)
    try {
      await onSubmit(payload)
    } finally {
      setSubmitting(false)
    }
  }

  const applyTemplate = (t: { fields: { name?: string; prompt?: string; trigger_type?: TriggerType; interval_secs?: number; cron_expr?: string } }) => {
    if (t.fields.name !== undefined) setName(t.fields.name)
    if (t.fields.prompt !== undefined) setPrompt(t.fields.prompt)
    if (t.fields.trigger_type) setTriggerType(t.fields.trigger_type)
    if (t.fields.interval_secs !== undefined) setIntervalSecs(t.fields.interval_secs)
    if (t.fields.cron_expr !== undefined) setCronExpr(t.fields.cron_expr)
  }

  const tryParseNl = () => {
    setNlError(null)
    setNlMatch(null)
    const parsed = parseNlCron(nlInput)
    if (!parsed) {
      setNlError(t('tasks.scheduleForm.parseError'))
      return
    }
    setTriggerType('cron')
    setCronExpr(parsed.expression)
    setNlMatch(parsed.description)
  }

  // Render a parsed cron descriptor, localizing the weekday (weekly schedules
  // carry dayOfWeek) into the {day} value before formatting.
  const renderCronDesc = (desc: CronDescription) => {
    const values: Record<string, string | number> = { ...desc.values }
    if (desc.dayOfWeek != null) values.day = weekdayName(intl.locale, desc.dayOfWeek)
    return intl.formatMessage({ id: desc.id }, values)
  }

  return (
    <div className="bg-surface-container-lowest border border-primary/30 rounded-xl p-lg mb-lg flex flex-col gap-md shadow-e1">
      {step === 'review' ? (
        /* W3-1: structured review step. Every value below is read from the
            same live form state that buildPayload() submits — one source of
            truth, so the preview can never describe a different routine
            than the one Activate creates. Nothing is created until the
            explicit Activate; "Back to edit" returns with all state intact. */
        <section
          aria-labelledby="schedule-review-title"
          aria-describedby="schedule-review-intro"
          data-testid="schedule-review"
          className="flex flex-col gap-sm"
        >
          <h3
            id="schedule-review-title"
            ref={reviewHeadingRef}
            tabIndex={-1}
            className="font-body-lg font-bold text-on-surface focus:outline-none"
          >
            {t('tasks.scheduleForm.reviewTitle')}
          </h3>
          <p id="schedule-review-intro" className="font-label-md text-on-surface-variant">
            {t('tasks.scheduleForm.reviewIntro')}
          </p>

          <dl className="flex flex-col gap-sm mt-xs">
            <ReviewRow label={t('tasks.scheduleForm.reviewSection.name')}>
              <span className="font-body-sm font-medium text-on-surface break-words">{name.trim()}</span>
            </ReviewRow>

            <ReviewRow label={t('tasks.scheduleForm.reviewSection.trigger')}>
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
                    {nlMatch && (
                      <span className="font-label-sm text-label-xs text-tertiary flex items-center gap-xs">
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">check_circle</span>
                        {t('tasks.scheduleForm.parsed')} {renderCronDesc(nlMatch)}
                      </span>
                    )}
                    {cronPreview?.valid && (
                      <span className="font-label-sm text-label-xs text-on-surface-variant">
                        {t('tasks.scheduleForm.next')}{' '}
                        {cronPreview.next_fires.slice(0, 3).map(n => new Date(n * 1000).toLocaleString()).join(' · ')}
                      </span>
                    )}
                  </>
                )}
                {triggerType === 'webhook' && (
                  <span className="font-label-sm text-label-xs text-on-surface-variant">{t('tasks.scheduleForm.typeHint.webhook')}</span>
                )}
                {triggerType === 'event' && (
                  <span className="font-label-sm text-label-xs text-on-surface-variant">{t('tasks.scheduleForm.typeHint.event')}</span>
                )}
                {maxFires !== '' && (
                  <span className="font-label-sm text-label-xs text-on-surface-variant">
                    {t('tasks.scheduleForm.maxFires')}: {maxFires}
                  </span>
                )}
              </div>
            </ReviewRow>

            <ReviewRow label={t('tasks.scheduleForm.reviewSection.prompt')}>
              <span className="font-body-sm text-on-surface whitespace-pre-wrap break-words">{prompt.trim()}</span>
            </ReviewRow>

            <ReviewRow label={t('tasks.scheduleForm.reviewSection.notifications')}>
              <div className="flex flex-col gap-xs">
                <span className="font-body-sm text-on-surface">
                  {t('office.routing.notifyWebhook')}: {notifyWebhook ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}
                </span>
                {notifyWebhook && webhookConfigured === false && (
                  <span className="font-label-sm text-label-xs text-on-surface-variant">{t('office.routing.webhookNone')}</span>
                )}
                <span className="font-body-sm text-on-surface">
                  {t('tasks.scheduleForm.notifyOnFailure')}: {policy.notify_on_failure ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}
                </span>
                <span className="font-label-sm text-label-xs text-on-surface-variant">
                  {policy.notify_on_failure
                    ? t('tasks.scheduleForm.reviewNotifyOnFailureOn')
                    : t('tasks.scheduleForm.reviewNotifyOnFailureOff')}
                </span>
              </div>
            </ReviewRow>

            <ReviewRow label={t('tasks.scheduleForm.reviewSection.policy')}>
              <div className="flex flex-col gap-xs font-label-sm text-label-sm text-on-surface">
                <span>{t('tasks.scheduleForm.maxRetries')}: {policy.max_retries}</span>
                <span>{t('tasks.scheduleForm.timeout')}: {policy.timeout_secs}s</span>
                <span>
                  {t('tasks.scheduleForm.budget')}:{' '}
                  {policy.budget_usd != null ? `$${policy.budget_usd}` : t('tasks.scheduleForm.budgetPlaceholder')}
                </span>
                <span className="break-all">
                  {t('tasks.scheduleForm.worktreePath')}: {policy.worktree?.trim() ? policy.worktree : t('tasks.routineDetailDrawer.none')}
                </span>
                <span>
                  {t('tasks.scheduleForm.autoArchive')}: {policy.auto_archive_when_empty ? t('tasks.routineDetailDrawer.yes') : t('tasks.routineDetailDrawer.no')}
                </span>
                {offpeakEnabled && (
                  <span>
                    {t('tasks.scheduleForm.offpeak.toggle')}: {clampHour(windowStart)}–{clampHour(windowEnd)} ·{' '}
                    {windowTz.trim() ? windowTz.trim() : localTimeZone}
                  </span>
                )}
              </div>
            </ReviewRow>
          </dl>
        </section>
      ) : (
        <>
      <div className="flex items-center justify-between">
        <h3 className="font-body-lg font-bold text-on-surface">{t('tasks.scheduleForm.title')}</h3>
        <Button
          variant="ghost"
          size="sm"
          type="button"
          className="font-label-sm text-primary hover:bg-primary/10 rounded-sm px-sm py-xs gap-xs"
          onClick={() => setShowPolicy(!showPolicy)}
          aria-expanded={showPolicy}
          aria-controls="schedule-policy"
        >
          <span className="material-symbols-outlined icon-sm">{showPolicy ? 'remove' : 'settings'}</span>
          {showPolicy ? t('tasks.scheduleForm.hidePolicy') : t('tasks.scheduleForm.policyOptions')}
        </Button>
      </div>

      <ScheduleTemplates onApply={applyTemplate} />

      {/* Natural-language input — surfaced at the top so users can describe
          the schedule in plain English ("daily at 9am", "weekdays at 8:30")
          and have the cron expression filled automatically. Parsing is
          best-effort; unmatched input falls through to the manual fields
          below. */}
      <div className="flex flex-col gap-xs p-md bg-primary/5 border border-primary/20 rounded-lg">
        <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.naturalLanguage')}</span>
        <div className="flex gap-xs items-end">
          <input
            type="text"
            aria-label={t('tasks.scheduleForm.nlAria')}
            placeholder={t('tasks.scheduleForm.nlPlaceholder')}
            value={nlInput}
            onChange={e => { setNlInput(e.target.value); setNlError(null); setNlMatch(null) }}
            onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); tryParseNl() } }}
            className="flex-1 bg-surface-container-lowest rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
          <Button
            type="button"
            onClick={tryParseNl}
            disabled={!nlInput.trim()}
            className="px-md py-sm rounded-lg font-label-md text-label-sm hover:bg-primary/90"
          >
            {t('tasks.scheduleForm.parse')}
          </Button>
        </div>
        {nlError ? (
          <div className="font-label-sm text-label-xs text-error flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm">error</span>
            {nlError}
          </div>
        ) : null}
        {nlMatch ? (
          <div className="font-label-sm text-label-xs text-tertiary flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm">check_circle</span>
            {t('tasks.scheduleForm.parsed')} {renderCronDesc(nlMatch)}
          </div>
        ) : null}
      </div>

      <label className="flex flex-col gap-xs">
        <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.nameLabel')}</span>
        <input
          type="text"
          placeholder={t('tasks.scheduleForm.namePlaceholder')}
          value={name}
          onChange={e => setName(e.target.value)}
          className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
      </label>

      <label className="flex flex-col gap-xs">
        <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.promptLabel')}</span>
        <textarea
          className="w-full h-20 p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
          placeholder={t('tasks.scheduleForm.promptPlaceholder')}
          value={prompt}
          onChange={e => setPrompt(e.target.value)}
        />
      </label>

      <fieldset className="flex flex-col gap-xs">
        <legend className="font-label-md text-on-surface-variant mb-xs">{t('tasks.scheduleForm.triggerType')}</legend>
        <div className="grid grid-cols-2 md:grid-cols-4 gap-sm" role="radiogroup" aria-label={t('tasks.scheduleForm.triggerType')}>
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
                className={cn('h-auto flex-col items-start gap-xs p-sm rounded-lg text-left whitespace-normal',
                  selected
                    ? 'border-primary bg-primary/10 text-on-surface hover:bg-primary/10'
                    : 'border-outline-variant/30 bg-surface-container-low text-on-surface-variant hover:bg-surface-container-low/60'
                )}
              >
                <span className="flex items-center gap-xs">
                  <span className="material-symbols-outlined icon-sm">{opt.icon}</span>
                  <span className="font-label-md font-bold">{t(`tasks.scheduleForm.type.${opt.value}`)}</span>
                </span>
                <span className="font-label-sm text-label-xs text-on-surface-variant">{t(`tasks.scheduleForm.typeHint.${opt.value}`)}</span>
              </Button>
            )
          })}
        </div>
      </fieldset>

      {triggerType === 'interval' ? (
        <label className="flex flex-col gap-xs">
          <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.intervalSeconds')}</span>
          <input
            type="number"
            min={1}
            value={intervalSecs}
            onChange={e => setIntervalSecs(Math.max(1, Number(e.target.value) || 0))}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
          />
          <span className="font-label-sm text-label-xs text-on-surface-variant">
            {intl.formatMessage({ id: 'tasks.scheduleForm.intervalHint' }, { mins: Math.round(intervalSecs / 60), hrs: Math.round(intervalSecs / 3600) })}
          </span>
        </label>
      ) : null}

      {triggerType === 'cron' ? (
        <div className="flex flex-col gap-xs">
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.cronExpression')}</span>
            <input
              type="text"
              placeholder={t('tasks.scheduleForm.cronPlaceholder')}
              value={cronExpr}
              onChange={e => setCronExpr(e.target.value)}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          {cronLoading ? (
            <span className="font-label-sm text-label-xs text-on-surface-variant">{t('tasks.scheduleForm.checking')}</span>
          ) : cronPreview ? (
            cronPreview.valid ? (
              <div className="font-label-sm text-label-xs text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined icon-sm text-primary">check_circle</span>
                {t('tasks.scheduleForm.next')} {cronPreview.next_fires.slice(0, 3).map(n => new Date(n * 1000).toLocaleString()).join(' · ')}
              </div>
            ) : (
              <div className="font-label-sm text-label-xs text-error flex items-center gap-xs">
                <span className="material-symbols-outlined icon-sm">error</span>
                {cronPreview.error ?? t('tasks.scheduleForm.invalidCron')}
              </div>
            )
          ) : null}
        </div>
      ) : null}

      {triggerType === 'webhook' ? (
        <div className="bg-tertiary/10 border border-tertiary/30 rounded-lg p-md flex gap-sm items-start">
          <span className="material-symbols-outlined icon-md text-on-tertiary">info</span>
          <div className="font-label-sm text-label-sm text-on-surface-variant">
            {t('tasks.scheduleForm.webhookInfo')}
          </div>
        </div>
      ) : null}

      {triggerType === 'event' ? (
        <div className="bg-secondary/10 border border-secondary/30 rounded-lg p-md flex gap-sm items-start">
          <span className="material-symbols-outlined icon-md text-secondary">info</span>
          <div className="font-label-sm text-label-sm text-on-surface-variant">
            {t('tasks.scheduleForm.eventInfo')}
          </div>
        </div>
      ) : null}

      <label className="flex flex-col gap-xs">
        <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.maxFires')}</span>
        <input
          type="number"
          min={1}
          placeholder={t('tasks.scheduleForm.maxFiresPlaceholder')}
          value={maxFires}
          onChange={e => setMaxFires(e.target.value === '' ? '' : Math.max(1, Number(e.target.value)))}
          className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
      </label>

      {/* B6' routing — copy the run-finished notification to the configured
          webhook. The setup hint only shows on a confirmed "no webhook"
          probe (see getWebhookConfig above). */}
      <div className="flex flex-col gap-xs">
        <label className="flex items-center gap-sm cursor-pointer">
          <input
            type="checkbox"
            checked={notifyWebhook}
            onChange={e => setNotifyWebhook(e.target.checked)}
            data-testid="notify-webhook-checkbox"
            className="cursor-pointer"
          />
          <span className="font-label-md text-on-surface">{t('office.routing.notifyWebhook')}</span>
        </label>
        {webhookConfigured === false && (
          <span className="font-label-sm text-label-xs text-on-surface-variant">
            {t('office.routing.webhookNone')}
          </span>
        )}
      </div>

      {showPolicy ? (
        <div id="schedule-policy" className="grid grid-cols-1 md:grid-cols-2 gap-md p-md bg-surface-container-low/60 rounded-lg border border-outline-variant/20">
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.maxRetries')}</span>
            <input
              type="number"
              min={0}
              value={policy.max_retries}
              onChange={e => setPolicy({ ...policy, max_retries: Math.max(0, Number(e.target.value) || 0) })}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
            <span className="font-label-sm text-label-xs text-on-surface-variant">{t('tasks.scheduleForm.maxRetriesHint')}</span>
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.timeout')}</span>
            <input
              type="number"
              min={1}
              value={policy.timeout_secs}
              onChange={e => setPolicy({ ...policy, timeout_secs: Math.max(1, Number(e.target.value) || 0) })}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.budget')}</span>
            <input
              type="number"
              min={0}
              step="0.01"
              placeholder={t('tasks.scheduleForm.budgetPlaceholder')}
              value={policy.budget_usd ?? ''}
              onChange={e => setPolicy({ ...policy, budget_usd: e.target.value === '' ? null : Math.max(0, Number(e.target.value)) })}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          <label className="flex flex-col gap-xs">
            <span className="font-label-md text-on-surface-variant">{t('tasks.scheduleForm.worktreePath')}</span>
            <input
              type="text"
              placeholder={t('tasks.scheduleForm.worktreePlaceholder')}
              value={policy.worktree ?? ''}
              onChange={e => setPolicy({ ...policy, worktree: e.target.value || null })}
              className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
            />
          </label>
          <div className="flex flex-col gap-xs md:col-span-2">
            <label className="flex items-center gap-sm cursor-pointer">
              <input
                type="checkbox"
                checked={policy.notify_on_failure}
                onChange={e => setPolicy({ ...policy, notify_on_failure: e.target.checked })}
                className="cursor-pointer"
              />
              <span className="font-label-md text-on-surface">{t('tasks.scheduleForm.notifyOnFailure')}</span>
            </label>
            {/* R7-③ boundary note: the auto-pause alert is deliberately NOT
                governed by the switch — an unattended opt-out must still
                learn when the system switches a routine off. */}
            <span className="font-label-sm text-on-surface-variant pl-lg">
              {t('tasks.scheduleForm.notifyOnFailureHint')}
            </span>
          </div>
          <label className="flex items-center gap-sm md:col-span-2 cursor-pointer">
            <input
              type="checkbox"
              checked={policy.auto_archive_when_empty}
              onChange={e => setPolicy({ ...policy, auto_archive_when_empty: e.target.checked })}
              className="cursor-pointer"
            />
            <span className="font-label-md text-on-surface">{t('tasks.scheduleForm.autoArchive')}</span>
          </label>

          {/* P2-5: off-peak execution window. When enabled, a due routine
              outside the window is queued (status "Queued") and runs at the
              first due check inside the window. Hours are inclusive; a
              start > end wraps past midnight (e.g. 22→6). */}
          <div className="md:col-span-2 flex flex-col gap-xs p-sm bg-secondary/5 border border-secondary/20 rounded-lg">
            <label className="flex items-center gap-sm cursor-pointer">
              <input
                type="checkbox"
                checked={offpeakEnabled}
                onChange={e => setOffpeakEnabled(e.target.checked)}
                className="cursor-pointer"
                aria-label={t('tasks.scheduleForm.offpeak.toggleAria')}
              />
              <span className="font-label-md text-on-surface font-semibold">
                {t('tasks.scheduleForm.offpeak.toggle')}
              </span>
            </label>
            <span className="font-label-sm text-label-xs text-on-surface-variant">
              {t('tasks.scheduleForm.offpeak.hint')}
            </span>
            {offpeakEnabled ? (
              <div className="grid grid-cols-2 md:grid-cols-3 gap-sm mt-xs">
                <label className="flex flex-col gap-xs">
                  <span className="font-label-sm text-label-xs text-on-surface-variant">
                    {t('tasks.scheduleForm.offpeak.startHour')}
                  </span>
                  <input
                    type="number"
                    min={0}
                    max={23}
                    value={windowStart}
                    onChange={e => setWindowStart(clampHour(Number(e.target.value)))}
                    className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                    aria-label={t('tasks.scheduleForm.offpeak.startHour')}
                  />
                </label>
                <label className="flex flex-col gap-xs">
                  <span className="font-label-sm text-label-xs text-on-surface-variant">
                    {t('tasks.scheduleForm.offpeak.endHour')}
                  </span>
                  <input
                    type="number"
                    min={0}
                    max={23}
                    value={windowEnd}
                    onChange={e => setWindowEnd(clampHour(Number(e.target.value)))}
                    className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                    aria-label={t('tasks.scheduleForm.offpeak.endHour')}
                  />
                </label>
                <label className="flex flex-col gap-xs md:col-span-2 md:grid-cols-0">
                  <span className="font-label-sm text-label-xs text-on-surface-variant">
                    {t('tasks.scheduleForm.offpeak.timezone')}
                  </span>
                  <input
                    type="text"
                    placeholder={localTimeZone}
                    value={windowTz}
                    onChange={e => setWindowTz(e.target.value)}
                    className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm font-mono focus:outline-none focus:ring-2 focus:ring-primary/30"
                    aria-label={t('tasks.scheduleForm.offpeak.timezone')}
                  />
                  <span className="font-label-sm text-label-2xs text-on-surface-variant">
                    {intl.formatMessage({ id: 'tasks.scheduleForm.offpeak.timezoneHint' }, { zone: localTimeZone })}
                  </span>
                </label>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {error ? (
        <div className="font-label-md text-error flex items-center gap-sm">
          <span className="material-symbols-outlined icon-sm">error</span>
          {error}
        </div>
      ) : null}

        </>
      )}

      {/* P2-6 — pre-task cost estimate in the create-confirm area. A new
          routine has no run history of its own, so the estimate is the
          all-routines baseline (taskId omitted). Read-only, never blocks. */}
      {valid && <CostEstimateHint />}

      {step === 'edit' ? (
        <div className="flex justify-end gap-sm">
          <Button
            variant="ghost"
            className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={() => { setName(''); setPrompt(''); setTriggerType('interval'); setIntervalSecs(3600); setCronExpr('0 9 * * *'); setMaxFires(''); setPolicy(DEFAULT_POLICY); setShowPolicy(false); onCancel() }}
          >
            {t('tasks.scheduleForm.cancel')}
          </Button>
          {/* W3-1: the edit-step CTA only opens the review step — a routine
              (a recurring cost) is never created without an explicit
              Activate on the structured preview. */}
          <Button
            className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            onClick={() => void beginReview()}
            disabled={!valid || submitting}
            aria-busy={submitting || undefined}
            data-testid="review-routine"
          >
            {t('tasks.scheduleForm.reviewCta')}
          </Button>
        </div>
      ) : (
        <div className="flex justify-end gap-sm">
          {/* State lives in this component's useState hooks, so going back
              keeps every filled field exactly as entered. */}
          <Button
            variant="ghost"
            className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={() => setStep('edit')}
            data-testid="back-to-edit"
          >
            {t('tasks.scheduleForm.backToEdit')}
          </Button>
          <Button
            className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            onClick={() => void activate()}
            disabled={submitting}
            aria-busy={submitting || undefined}
            data-testid="activate-routine"
          >
            {submitting ? t('tasks.scheduleForm.activating') : t('tasks.scheduleForm.activate')}
          </Button>
        </div>
      )}
    </div>
  )
}

/** W3-1: one label/value row of the structured review grid. */
function ReviewRow({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[minmax(8rem,auto)_1fr] gap-sm items-start">
      <dt className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider pt-0.5">{label}</dt>
      <dd className="min-w-0 flex flex-col gap-xs">{children}</dd>
    </div>
  )
}

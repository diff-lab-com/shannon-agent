// ScheduleForm — creates a scheduled routine via Tauri create_scheduled_task.
//
// Covers Phase D P2.4 (webhook trigger) and P2.6 (retry policy exposure)
// in one unified form. All four trigger types are selectable; policy fields
// are optional with MD3-styled inputs. Live cron preview uses preview_cron.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import ResultRoutingEditor from './ResultRoutingEditor'
import ScheduleTemplates from './ScheduleTemplates'
import { parseNlCron } from '@/lib/nl-cron'
import * as api from '@/lib/tauri-api'
import type {
  TriggerType,
  ExecutionPolicy,
  CreateTaskPayload,
  CronPreview,
} from '@/types'

interface ScheduleFormProps {
  onSubmit: (payload: CreateTaskPayload) => void
  onCancel: () => void
}

type TriggerOption = {
  value: TriggerType
  label: string
  icon: string
  hint: string
}

const TRIGGER_OPTIONS: TriggerOption[] = [
  { value: 'interval', label: 'Interval', icon: 'timer', hint: 'Run every N seconds' },
  { value: 'cron', label: 'Cron', icon: 'schedule', hint: 'Unix cron expression' },
  { value: 'webhook', label: 'Webhook', icon: 'webhook', hint: 'Triggered by HTTP POST' },
  { value: 'event', label: 'Event', icon: 'bolt', hint: 'Triggered by another task' },
]

const DEFAULT_POLICY: ExecutionPolicy = {
  max_retries: 2,
  timeout_secs: 600,
  worktree: null,
  notify_on_failure: true,
  budget_usd: null,
  auto_archive_when_empty: false,
  result_routing: [],
}

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
  const [cronPreview, setCronPreview] = useState<CronPreview | null>(null)
  const [cronLoading, setCronLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [nlInput, setNlInput] = useState('')
  const [nlError, setNlError] = useState<string | null>(null)
  const [nlMatch, setNlMatch] = useState<string | null>(null)

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

  const submit = () => {
    if (!valid) {
      setError(t('tasks.scheduleForm.requiredFields'))
      return
    }
    setError(null)
    const payload: CreateTaskPayload = {
      name: name.trim(),
      prompt: prompt.trim(),
      trigger_type: triggerType,
      ...(triggerType === 'interval' ? { interval_secs: intervalSecs } : {}),
      ...(triggerType === 'cron' ? { cron_expr: cronExpr.trim() } : {}),
      ...(maxFires !== '' ? { max_fires: maxFires } : {}),
      policy,
    }
    onSubmit(payload)
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

  return (
    <div className="bg-surface-container-lowest border border-primary/30 rounded-xl p-lg mb-lg flex flex-col gap-md shadow-sm">
      <div className="flex items-center justify-between">
        <h3 className="font-body-lg font-bold text-on-surface">{t('tasks.scheduleForm.title')}</h3>
        <button
          type="button"
          className="font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          onClick={() => setShowPolicy(!showPolicy)}
          aria-expanded={showPolicy}
          aria-controls="schedule-policy"
        >
          <span className="material-symbols-outlined text-[14px]">{showPolicy ? 'remove' : 'settings'}</span>
          {showPolicy ? t('tasks.scheduleForm.hidePolicy') : t('tasks.scheduleForm.policyOptions')}
        </button>
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
          <button
            type="button"
            onClick={tryParseNl}
            disabled={!nlInput.trim()}
            className="px-md py-sm rounded-lg border border-primary/40 bg-primary text-on-primary font-label-md text-[12px] hover:bg-primary/90 disabled:opacity-40 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
          >
            {t('tasks.scheduleForm.parse')}
          </button>
        </div>
        {nlError ? (
          <div className="font-label-sm text-[11px] text-error flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px]">error</span>
            {nlError}
          </div>
        ) : null}
        {nlMatch ? (
          <div className="font-label-sm text-[11px] text-tertiary flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px]">check_circle</span>
            {t('tasks.scheduleForm.parsed')} {nlMatch}
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
              <button
                key={opt.value}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => setTriggerType(opt.value)}
                className={`flex flex-col items-start gap-xs p-sm rounded-lg border text-left cursor-pointer transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ${
                  selected
                    ? 'border-primary bg-primary/10 text-on-surface'
                    : 'border-outline-variant/30 bg-surface-container-low text-on-surface-variant hover:bg-surface-container-low/60'
                }`}
              >
                <span className="flex items-center gap-xs">
                  <span className="material-symbols-outlined text-[16px]">{opt.icon}</span>
                  <span className="font-label-md font-bold">{opt.label}</span>
                </span>
                <span className="font-label-sm text-[11px] text-on-surface-variant">{opt.hint}</span>
              </button>
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
          <span className="font-label-sm text-[11px] text-on-surface-variant">
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
            <span className="font-label-sm text-[11px] text-on-surface-variant">{t('tasks.scheduleForm.checking')}</span>
          ) : cronPreview ? (
            cronPreview.valid ? (
              <div className="font-label-sm text-[11px] text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px] text-primary">check_circle</span>
                {t('tasks.scheduleForm.next')} {cronPreview.next_fires.slice(0, 3).map(n => new Date(n * 1000).toLocaleString()).join(' · ')}
              </div>
            ) : (
              <div className="font-label-sm text-[11px] text-error flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px]">error</span>
                {cronPreview.error ?? t('tasks.scheduleForm.invalidCron')}
              </div>
            )
          ) : null}
        </div>
      ) : null}

      {triggerType === 'webhook' ? (
        <div className="bg-tertiary/10 border border-tertiary/30 rounded-lg p-md flex gap-sm items-start">
          <span className="material-symbols-outlined text-[18px] text-on-tertiary">info</span>
          <div className="font-label-sm text-[12px] text-on-surface-variant">
            {t('tasks.scheduleForm.webhookInfo')}
          </div>
        </div>
      ) : null}

      {triggerType === 'event' ? (
        <div className="bg-secondary/10 border border-secondary/30 rounded-lg p-md flex gap-sm items-start">
          <span className="material-symbols-outlined text-[18px] text-secondary">info</span>
          <div className="font-label-sm text-[12px] text-on-surface-variant">
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
            <span className="font-label-sm text-[11px] text-on-surface-variant">{t('tasks.scheduleForm.maxRetriesHint')}</span>
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
          <label className="flex items-center gap-sm md:col-span-2 cursor-pointer">
            <input
              type="checkbox"
              checked={policy.notify_on_failure}
              onChange={e => setPolicy({ ...policy, notify_on_failure: e.target.checked })}
              className="cursor-pointer"
            />
            <span className="font-label-md text-on-surface">{t('tasks.scheduleForm.notifyOnFailure')}</span>
          </label>
          <label className="flex items-center gap-sm md:col-span-2 cursor-pointer">
            <input
              type="checkbox"
              checked={policy.auto_archive_when_empty}
              onChange={e => setPolicy({ ...policy, auto_archive_when_empty: e.target.checked })}
              className="cursor-pointer"
            />
            <span className="font-label-md text-on-surface">{t('tasks.scheduleForm.autoArchive')}</span>
          </label>
          <div className="md:col-span-2">
            <ResultRoutingEditor
              value={policy.result_routing ?? []}
              onChange={next => setPolicy({ ...policy, result_routing: next })}
            />
          </div>
        </div>
      ) : null}

      {error ? (
        <div className="font-label-md text-error flex items-center gap-sm">
          <span className="material-symbols-outlined text-[16px]">error</span>
          {error}
        </div>
      ) : null}

      <div className="flex justify-end gap-sm">
        <Button
          variant="ghost"
          className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
          onClick={() => { setName(''); setPrompt(''); setTriggerType('interval'); setIntervalSecs(3600); setCronExpr('0 9 * * *'); setMaxFires(''); setPolicy(DEFAULT_POLICY); setShowPolicy(false); onCancel() }}
        >
          {t('tasks.scheduleForm.cancel')}
        </Button>
        <Button
          className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
          onClick={submit}
          disabled={!valid}
        >
          {t('tasks.scheduleForm.createRoutine')}
        </Button>
      </div>
    </div>
  )
}

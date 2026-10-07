// NlRoutineQuickCreate — the routines tab's "create an automation from one
// sentence" card (design: docs/design/ui-redesign-2026-10/pages/04-tasks.html,
// 自然语言创建 · 结构化预览 → 调整 / 激活).
//
// Flow: one-line input → parse (deterministic, zero-token — see
// ./nlQuickCreate.ts + src/lib/nl-cron.ts) → structured preview of three
// chips: 触发 (localized schedule description + raw cron), 动作 (the action
// half of the sentence, verbatim — the parser does NOT paraphrase it), 通知
// (honestly muted: NL notification routing is not supported yet).
//
// Two exits, per the design's preview→confirm doctrine:
//   激活  — builds the CreateTaskPayload from exactly what the preview showed
//           and hands it to the page's create hook. Failure is NOT silent:
//           the hook toasts the error and this card shows inline guidance to
//           retry via 调整.
//   调整  — hands the parsed values to the full ScheduleForm prefilled, for
//           name/prompt/policy/webhook — everything the one-liner can't carry.
//
// Honesty boundary: the parser reads SCHEDULES only. A delivery target like
// 「发到 Discord」 is never rendered as a notification chip — it stays part
// of the action text (and therefore of the prompt) untouched.
//
// Signature: this card carries the aurora-line edge per the v2 design (the
// one sanctioned decorative element; docs/design/ui-redesign-2026-10 §1).

import { useState } from 'react'
import { useIntl, type PrimitiveType } from 'react-intl'
import { Button } from '@/components/ui/button'
import { weekdayName, DEFAULT_POLICY } from './shared'
import type { CronDescription } from '@/lib/nl-cron'
import { splitNlSchedule, deriveRoutineName, actionSnippet, type NlSplitResult } from './nlQuickCreate'
import type { CreateTaskPayload } from '@/types'

/** What 调整 hands over to ScheduleForm's `initial` prefill. */
export interface NlRoutinePrefill {
  name: string
  prompt: string
  cronExpr: string
}

interface NlRoutineQuickCreateProps {
  /** Direct activation. Resolves true when the routine was created (the
   *  page owns the success toast + list refresh); false keeps the preview
   *  up and shows the adjust-path guidance. */
  onActivate: (payload: CreateTaskPayload) => Promise<boolean>
  /** Open the full ScheduleForm with the parsed values prefilled. */
  onAdjust: (prefill: NlRoutinePrefill) => void
  /** Open the empty ScheduleForm (parse-failure guidance exit). */
  onOpenForm: () => void
}

export default function NlRoutineQuickCreate({ onActivate, onAdjust, onOpenForm }: NlRoutineQuickCreateProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)

  const [input, setInput] = useState('')
  const [split, setSplit] = useState<NlSplitResult | null>(null)
  const [parseFailed, setParseFailed] = useState(false)
  const [activating, setActivating] = useState(false)
  const [activateFailed, setActivateFailed] = useState(false)

  const tryParse = () => {
    setActivateFailed(false)
    const hit = splitNlSchedule(input)
    setSplit(hit)
    setParseFailed(hit === null)
  }

  // Localized restatement of the parsed schedule (weekly rows carry the cron
  // DOW so the caller localizes it into the {day} value — same contract as
  // ScheduleForm's NL preview).
  const describeSchedule = (desc: CronDescription) => {
    const values: Record<string, string | number> = { ...desc.values }
    if (desc.dayOfWeek != null) values.day = weekdayName(intl.locale, desc.dayOfWeek)
    return intl.formatMessage({ id: desc.id }, values)
  }

  // The payload mirrors the preview exactly: the prompt is the action half
  // verbatim (or the whole line when the input was a bare schedule — the
  // 调整 path is the right place to flesh that out).
  const buildPayload = (hit: NlSplitResult): CreateTaskPayload => ({
    name: deriveRoutineName(hit.scheduleText, hit.restText),
    prompt: hit.restText.trim() || input.trim(),
    trigger_type: 'cron',
    cron_expr: hit.parsed.expression,
    policy: { ...DEFAULT_POLICY },
    notify_webhook: false,
  })

  const activate = async () => {
    if (!split || activating) return
    setActivating(true)
    setActivateFailed(false)
    try {
      const ok = await onActivate(buildPayload(split))
      if (ok) {
        setInput('')
        setSplit(null)
        setParseFailed(false)
      } else {
        setActivateFailed(true)
      }
    } finally {
      setActivating(false)
    }
  }

  const adjust = () => {
    if (!split) return
    onAdjust({
      name: deriveRoutineName(split.scheduleText, split.restText),
      prompt: split.restText.trim() || input.trim(),
      cronExpr: split.parsed.expression,
    })
  }

  const actionText = split ? split.restText.trim() || input.trim() : ''

  return (
    <section
      aria-labelledby="nl-quick-create-title"
      data-testid="nl-quick-create"
      className="aurora-line bg-surface-container-lowest border border-outline-variant/10 rounded-xl p-lg shadow-e1 flex flex-col gap-sm"
    >
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined icon-md text-primary" aria-hidden="true">auto_awesome</span>
        <h3 id="nl-quick-create-title" className="font-body-md font-bold text-on-surface">
          {t('tasks.nl.title')}
        </h3>
        {split && (
          <span className="ml-auto inline-flex items-center gap-xs px-xs py-0.5 rounded-full bg-primary-container text-on-primary-container font-label-sm text-label-xs font-bold uppercase tracking-wider">
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">fact_check</span>
            {t('tasks.nl.previewBadge')}
          </span>
        )}
      </div>

      {/* One-line input + parse. Enter parses, mirroring ScheduleForm's NL
          input so the two surfaces behave the same. */}
      <div className="flex gap-xs items-end">
        <input
          type="text"
          aria-label={t('tasks.nl.inputAria')}
          placeholder={t('tasks.nl.placeholder')}
          value={input}
          onChange={e => { setInput(e.target.value); setParseFailed(false); setActivateFailed(false) }}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); tryParse() } }}
          className="flex-1 bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
        />
        <Button
          type="button"
          onClick={tryParse}
          disabled={!input.trim()}
          className="px-md py-sm rounded-lg font-label-md text-label-sm hover:bg-primary/90"
        >
          {t('tasks.nl.parse')}
        </Button>
      </div>

      {parseFailed && (
        <div className="flex flex-col gap-xs" data-testid="nl-parse-failed">
          <div className="font-label-sm text-label-sm text-error flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
            {t('tasks.nl.parseFailed')}
          </div>
          <div>
            <Button
              variant="ghost"
              size="sm"
              className="cursor-pointer inline-flex items-center gap-xs text-primary hover:bg-primary/10"
              onClick={onOpenForm}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">edit_note</span>
              {t('tasks.nl.openForm')}
            </Button>
          </div>
        </div>
      )}

      {split && (
        <div aria-live="polite" data-testid="nl-preview" className="flex flex-col gap-sm">
          <dl className="flex flex-col gap-xs">
            {/* 触发 — localized schedule + the raw cron, so the confirmation
                never hides what will actually be stored. */}
            <div className="grid grid-cols-[minmax(3.5rem,auto)_1fr] gap-sm items-start">
              <dt className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider pt-0.5">
                {t('tasks.nl.trigger')}
              </dt>
              <dd className="min-w-0 flex flex-wrap items-center gap-xs">
                <span className="inline-flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/30 bg-surface-container-low text-on-surface font-label-sm text-label-sm">
                  <span className="material-symbols-outlined icon-sm text-on-surface-variant" aria-hidden="true">schedule</span>
                  {describeSchedule(split.parsed.description)}
                </span>
                <span className="font-mono text-label-xs text-on-surface-variant break-all">{split.parsed.expression}</span>
              </dd>
            </div>
            {/* 动作 — the user's own words, verbatim (truncated for display;
                the full text lands in the routine prompt). */}
            <div className="grid grid-cols-[minmax(3.5rem,auto)_1fr] gap-sm items-start">
              <dt className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider pt-0.5">
                {t('tasks.nl.action')}
              </dt>
              <dd className="min-w-0">
                <span
                  className="inline-flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/30 bg-surface-container-low text-on-surface font-label-sm text-label-sm max-w-full"
                  title={actionText}
                >
                  <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">account_tree</span>
                  <span className="truncate">{actionSnippet(actionText)}</span>
                </span>
              </dd>
            </div>
            {/* 通知 — honest muted state: NL notification routing is not
                supported, so nothing is promised here. */}
            <div className="grid grid-cols-[minmax(3.5rem,auto)_1fr] gap-sm items-start">
              <dt className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider pt-0.5">
                {t('tasks.nl.notify')}
              </dt>
              <dd className="min-w-0">
                <span className="inline-flex items-center gap-xs px-sm py-0.5 rounded-full border border-outline-variant/20 bg-surface-container-low/60 text-on-surface-variant font-label-sm text-label-sm">
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">notifications_off</span>
                  {t('tasks.nl.notifyFallback')}
                </span>
              </dd>
            </div>
          </dl>

          <p className="font-label-sm text-label-xs text-on-surface-variant flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">info</span>
            {t('tasks.nl.hint')}
          </p>

          {activateFailed && (
            <div className="font-label-sm text-label-sm text-error flex items-center gap-xs" data-testid="nl-activate-failed">
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">error</span>
              {t('tasks.nl.createFailed')}
            </div>
          )}

          <div className="flex items-center justify-end gap-sm">
            <Button
              variant="ghost"
              size="sm"
              disabled={!split || activating}
              className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={adjust}
              data-testid="nl-adjust"
            >
              {t('tasks.nl.adjust')}
            </Button>
            <Button
              size="sm"
              disabled={!split || activating}
              aria-busy={activating || undefined}
              className="px-md py-sm rounded-lg font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
              onClick={() => void activate()}
              data-testid="nl-activate"
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">check</span>
              {activating ? t('tasks.nl.activating') : t('tasks.nl.activate')}
            </Button>
          </div>
        </div>
      )}
    </section>
  )
}

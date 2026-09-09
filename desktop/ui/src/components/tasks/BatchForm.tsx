// Inline form for starting a best-of-N batch run (P1-2).
//
// Deliberately a SEPARATE component (not a NewTaskForm mode): the brief
// allows either, and a sibling form keeps the existing background-task form
// untouched. One prompt, N (2..=4) parallel candidate runs — each executed
// unattended in its own git worktree forked from the current session's
// project HEAD (baseSessionId = the Tasks page's current session).

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

export const BATCH_COUNT_CHOICES = [2, 3, 4] as const

interface BatchFormProps {
  /** Current session id — passed as baseSessionId so the batch runs
   * against the session's working directory (the project). */
  sessionId: string | null
  onSubmit: (input: { title: string; prompt: string; count: number; sessionId: string | null }) => void
  onCancel: () => void
}

export default function BatchForm({ sessionId, onSubmit, onCancel }: BatchFormProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const [title, setTitle] = useState('')
  const [prompt, setPrompt] = useState('')
  const [count, setCount] = useState<number>(2)

  const canSubmit = prompt.trim().length > 0

  const submit = () => {
    if (!canSubmit) return
    onSubmit({
      title: title.trim(),
      prompt: prompt.trim(),
      count,
      sessionId,
    })
  }

  return (
    <div
      className="bg-surface-container-lowest border border-tertiary/30 rounded-xl p-lg mb-lg flex flex-col gap-md shadow-sm"
      data-testid="batch-form"
    >
      <div className="flex items-center gap-sm">
        <span className="material-symbols-outlined text-[20px] text-tertiary" aria-hidden="true">
          call_split
        </span>
        <h3 className="font-body-lg font-bold text-on-surface">{t('batch.form.title')}</h3>
      </div>
      <p className="font-label-sm text-on-surface-variant">{t('batch.form.hint')}</p>

      <input
        type="text"
        className="w-full px-sm py-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm focus:outline-none focus:ring-2 focus:ring-tertiary/30"
        placeholder={t('batch.form.titlePlaceholder')}
        aria-label={t('batch.form.titleAria')}
        value={title}
        onChange={e => setTitle(e.target.value)}
      />
      <textarea
        className="w-full h-20 p-sm bg-surface-container-low rounded-lg border border-tertiary/30 text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-tertiary/30"
        placeholder={t('batch.form.promptPlaceholder')}
        aria-label={t('batch.form.promptAria')}
        value={prompt}
        onChange={e => setPrompt(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter' && !e.shiftKey && canSubmit) {
            e.preventDefault()
            submit()
          }
        }}
        autoFocus
      />

      <fieldset className="flex items-center gap-sm">
        <legend className="sr-only">{t('batch.form.countAria')}</legend>
        <span className="font-label-md text-on-surface-variant">{t('batch.form.count')}</span>
        {BATCH_COUNT_CHOICES.map(n => (
          <button
            key={n}
            type="button"
            role="radio"
            aria-checked={count === n}
            onClick={() => setCount(n)}
            className={cn(
              'h-8 w-10 rounded-lg border font-label-md cursor-pointer transition-colors',
              count === n
                ? 'bg-tertiary text-on-tertiary border-tertiary'
                : 'bg-surface-container-low text-on-surface border-outline-variant/40 hover:bg-surface-container',
            )}
          >
            {n}
          </button>
        ))}
      </fieldset>

      <div className="flex items-center justify-end gap-sm">
        <Button
          variant="ghost"
          className="px-md py-sm rounded-lg border border-outline-variant font-label-md cursor-pointer"
          onClick={onCancel}
        >
          {t('batch.form.cancel')}
        </Button>
        <Button
          className="px-md py-sm bg-tertiary text-on-tertiary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
          onClick={submit}
          disabled={!canSubmit}
        >
          <span className="material-symbols-outlined icon-md" aria-hidden="true">
            call_split
          </span>
          {t('batch.form.start', { count })}
        </Button>
      </div>
    </div>
  )
}

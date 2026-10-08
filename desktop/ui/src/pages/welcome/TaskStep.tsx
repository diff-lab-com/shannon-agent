// Step 0 — describe the first task (design 01:122-160). A hero composer is
// the primary surface: typing +「开始 →」hands the text to the chat composer
// (as a DRAFT — the trust contract never auto-sends) and navigates to /chat.
// The template cards below FILL that input (and still select the use case
// that drives the provider + tool recommendations). Extracted from
// Welcome.tsx (T3.1).
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { TASKS, type TaskId } from './constants'

interface TaskStepProps {
  // null until the user picks a template — the model section below stays
  // collapsed so the composer stays the hero of the screen.
  task: TaskId | null
  setTask: (id: TaskId) => void
  /** Text of the hero composer (lifted — Welcome submits it to /chat). */
  draft: string
  setDraft: (text: string) => void
  /** Display label for the model chip (current provider selection). */
  providerLabel: string
  /** A usable provider is already active (env key or saved) — the chip
   *  shows a check instead of the expand caret. */
  providerReady: boolean
  /**「开始 →」— Welcome creates the session + draft + navigates. */
  onStart: () => void
  /** Model chip click → same AddProviderModal the Model step uses. */
  onOpenProvider: () => void
}

export function TaskStep({
  task,
  setTask,
  draft,
  setDraft,
  providerLabel,
  providerReady,
  onStart,
  onOpenProvider,
}: TaskStepProps) {
  const intl = useIntl()
  const placeholder = intl.formatMessage({ id: 'welcome.composer.placeholder' })
  return (
    <section>
      {/* Screen-reader heading — the visible hero is the composer itself
          (design 01 drops the old card title for a composer-first layout). */}
      <h2 className="sr-only">{intl.formatMessage({ id: 'welcome.task.title' })}</h2>

      {/* Hero composer — composer material + aurora line, width-capped like
          the mockup's .hero-composer. */}
      <div
        className="aurora-line rounded-2xl border border-outline-variant/30 bg-surface-container-lowest p-md shadow-e1"
        data-testid="welcome-hero-composer"
      >
        <textarea
          rows={3}
          value={draft}
          placeholder={placeholder}
          aria-label={placeholder}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => {
            // Ctrl/Cmd+Enter mirrors the chat composer's send-from-textarea.
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault()
              onStart()
            }
          }}
          data-testid="welcome-composer-input"
          className="w-full resize-none bg-transparent font-body-md text-on-surface outline-none placeholder:text-on-surface-variant"
        />
        <div className="mt-sm flex items-center gap-sm">
          <button
            type="button"
            onClick={onOpenProvider}
            title={intl.formatMessage({ id: 'welcome.composer.model' })}
            aria-label={intl.formatMessage({ id: 'welcome.composer.model' })}
            data-testid="welcome-composer-model"
            className="inline-flex cursor-pointer items-center gap-xs rounded-full border border-outline-variant/50 bg-surface-container-low px-sm py-xs font-label-sm text-on-surface transition-colors hover:border-primary/50 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary"
          >
            <span className="material-symbols-outlined icon-sm text-primary" aria-hidden="true">auto_awesome</span>
            <span>{providerLabel}</span>
            <span
              className={cn('material-symbols-outlined icon-sm', providerReady ? 'text-success' : 'text-on-surface-variant')}
              aria-hidden="true"
            >
              {providerReady ? 'check_circle' : 'expand_more'}
            </span>
          </button>
          <span className="flex-1" />
          <Button
            onClick={onStart}
            disabled={!draft.trim()}
            data-testid="welcome-composer-start"
            className="px-lg py-sm bg-primary text-on-primary rounded-full font-label-md cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed hover:bg-primary/90 transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
          >
            {intl.formatMessage({ id: 'welcome.composer.start' })}
          </Button>
        </div>
      </div>

      {/* Template cards — concrete first tasks (design 01:141-160). Clicking
          fills the composer; the aria-pressed state records which use case
          drives the recommendations below. */}
      <div className="mt-md grid grid-cols-2 gap-sm sm:grid-cols-4">
        {TASKS.map(t => (
          <button
            key={t.id}
            type="button"
            onClick={() => {
              setTask(t.id)
              setDraft(intl.formatMessage({ id: t.promptKey }))
            }}
            aria-pressed={task === t.id}
            data-testid={`welcome-template-${t.id}`}
            className={cn(
              'h-auto rounded-xl border p-md text-left cursor-pointer transition-all focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary',
              task === t.id
                ? 'border-primary bg-primary-container/5'
                : 'border-outline-variant/50 hover:border-primary/50 hover:bg-surface-container-low',
            )}
          >
            <span className="material-symbols-outlined text-primary" aria-hidden="true">{t.icon}</span>
            <div className="mt-xs font-label-md font-medium text-on-surface">
              {intl.formatMessage({ id: t.labelKey })}
            </div>
            <div className="mt-1 truncate font-label-sm text-on-surface-variant">
              {intl.formatMessage({ id: t.blurbKey })}
            </div>
          </button>
        ))}
      </div>
    </section>
  )
}

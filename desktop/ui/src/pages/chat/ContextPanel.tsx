import { useState } from 'react'
import { toast } from 'sonner'
import type { ToolCall, UsagePayload } from '@/types'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useSessions } from '@/context/SessionContext'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import { pushComposerDraft } from '@/lib/composerBridge'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import BudgetDialog from '@/components/chat/BudgetDialog'

/**
 * P1-⑦ (ZCode delta): the panel's cards without the aside/width chrome —
 * this is what the right dock renders inside its Context tab. Kept separate
 * from the legacy `ContextPanel` wrapper below so the dock owns sizing.
 */
export function ContextPanelContent({ usage, activeToolCalls }: { usage: UsagePayload | null; activeToolCalls: ToolCall[] }) {
  const t = useT()
  const { currentSessionId, sessionSources, addSessionSource, removeSessionSource } = useSessions()
  const { budget, usage: sessionUsage, refresh: refreshBudget } = useSessionBudget(currentSessionId)
  const [budgetOpen, setBudgetOpen] = useState(false)
  // Budget progress (spent/budget) — only rendered while a cap is set.
  const budgetSpent = sessionUsage?.cost_usd ?? 0
  const budgetPct = budget != null && budget > 0 ? Math.min(100, (budgetSpent / budget) * 100) : null
  const budgetBarColor = budgetPct != null && budgetPct >= 100 ? 'bg-error' : budgetPct != null && budgetPct >= 80 ? 'bg-warning' : 'bg-primary'

  return (
    <div className="space-y-xl">
      {/* Token Usage */}
      {usage && (
        <section>
          <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">{t('chat.context.usage')}</h3>
          <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
            <div className="flex justify-between text-body-sm">
              <span className="text-on-surface-variant">{t('chat.context.inputTokens')}</span>
              <span className="font-bold text-on-surface">{usage.input_tokens.toLocaleString()}</span>
            </div>
            <div className="flex justify-between text-body-sm">
              <span className="text-on-surface-variant">{t('chat.context.outputTokens')}</span>
              <span className="font-bold text-on-surface">{usage.output_tokens.toLocaleString()}</span>
            </div>
            <div className="flex justify-between text-body-sm">
              <span className="text-on-surface-variant">{t('chat.context.cost')}</span>
              <span className="font-bold text-primary">${usage.cost_usd.toFixed(4)}</span>
            </div>
            {(() => {
              const total = usage.input_tokens + usage.output_tokens
              const max = usage.max_tokens
              if (!max) return null
              const pct = Math.min(100, (total / max) * 100)
              const barColor = pct > 80 ? 'bg-error' : pct > 50 ? 'bg-secondary' : 'bg-primary'
              return (
                <div className="pt-sm border-t border-outline-variant/10">
                  <div className="flex justify-between text-label-sm text-on-surface-variant mb-xs">
                    <span>{t('chat.context.window')}</span>
                    <span className="font-bold">{pct.toFixed(0)}%</span>
                  </div>
                  <div className="w-full h-1.5 bg-surface-container-high rounded-full overflow-hidden">
                    <div className={cn("h-full rounded-full transition-all duration-(--duration-slower)", barColor)} style={{ width: `${pct}%` }} />
                  </div>
                  <p className="text-label-sm text-on-surface-variant mt-xs">{total.toLocaleString()} / {max.toLocaleString()}</p>
                </div>
              )
            })()}
          </div>
        </section>
      )}

      {/* P0-4: six-category context composition + cache hit rate */}
      <ContextBreakdownCard sessionId={currentSessionId} usageTick={usage} />

      {/* P0-4: session budget */}
      <section aria-label={t('budget.section.title')}>
        <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">{t('budget.section.title')}</h3>
        <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
          {budget != null && budget > 0 ? (
            <>
              <div className="flex justify-between text-body-sm">
                <span className="text-on-surface-variant">{budgetSpent.toFixed(4)} / ${budget.toFixed(2)}</span>
                <span className="font-bold text-on-surface tabular-nums">{budgetPct?.toFixed(0)}%</span>
              </div>
              <div className="w-full h-1.5 bg-surface-container-high rounded-full overflow-hidden">
                <div className={cn('h-full rounded-full transition-all duration-(--duration-slower)', budgetBarColor)} style={{ width: `${budgetPct ?? 0}%` }} />
              </div>
            </>
          ) : (
            <p className="text-body-sm text-on-surface-variant">{t('budget.dialog.label')}</p>
          )}
          <Button
            variant="outline"
            className="w-full px-md py-xs rounded-xl font-label-md border-outline-variant/30 bg-surface-container-lowest/60 hover:bg-surface-container-low"
            onClick={() => setBudgetOpen(true)}
            disabled={!currentSessionId}
          >
            <span className="material-symbols-outlined icon-sm mr-xs" aria-hidden="true">payments</span>
            {t('budget.menu.set')}
          </Button>
        </div>
      </section>

      {/* office Wave 3 C4: session-scoped source scratchpad. Draft-board
          semantics — in-memory, per session, one click cites into the
          composer draft; NOT wired into the send pipeline. */}
      <SessionSourcesSection
        sessionId={currentSessionId}
        // Optional chain: the slice is new — test harnesses (and any stale
        // provider) may hand back a partial session context.
        sources={currentSessionId ? sessionSources?.[currentSessionId] ?? [] : []}
        onAdd={item => { if (currentSessionId) addSessionSource?.(currentSessionId, item) }}
        onRemove={item => { if (currentSessionId) removeSessionSource?.(currentSessionId, item) }}
      />

      {/* Active Tool Calls */}
      {activeToolCalls.length > 0 && (
        <section>
          <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">
            {t('chat.context.activeTools')}
            <Badge size="sm" variant="primary" className="ml-xs">{activeToolCalls.length}</Badge>
          </h3>
          <div className="space-y-sm">
            {activeToolCalls.map(tc => (
              <div key={tc.tool_use_id} className="p-sm bg-surface-container rounded-xl flex items-center gap-sm border border-outline-variant/10">
                <span className={cn("w-2 h-2 rounded-full shrink-0", tc.status === 'running' ? 'bg-secondary animate-pulse' : tc.status === 'error' ? 'bg-error' : 'bg-tertiary')}></span>
                <p className="text-label-md truncate">{tc.tool_name}</p>
              </div>
            ))}
          </div>
        </section>
      )}

      <BudgetDialog
        open={budgetOpen}
        sessionId={currentSessionId}
        budget={budget}
        onClose={() => setBudgetOpen(false)}
        onSaved={() => refreshBudget()}
      />
    </div>
  )
}

// B4 audit: the legacy standalone wrapper (fixed 300px slide-in aside) is
// gone — the chat page hosts `ContextPanelContent` inside RightDock's
// Context tab and nothing rendered the wrapper. Only the content component
// remains.

/* ────────────────  office Wave 3 C4: Session sources  ──────────────── */

/**
 * The "Session sources" scratchpad block. Add accepts a file path or URL;
 * each row can be cited into the composer draft as a single
 * `[Source] <item>` line (pushComposerDraft — never sent) or removed.
 * v1 is intentionally a draft board: in-memory per session, cleared on
 * refresh, no send_message injection.
 */
export function SessionSourcesSection({
  sessionId,
  sources,
  onAdd,
  onRemove,
}: {
  sessionId: string | null
  sources: string[]
  onAdd: (item: string) => void
  onRemove: (item: string) => void
}) {
  const t = useT()
  const [draft, setDraft] = useState('')

  const submit = () => {
    if (!draft.trim()) return
    onAdd(draft)
    setDraft('')
  }

  const cite = (item: string) => {
    pushComposerDraft(`[Source] ${item}`)
    toast.success(t('office.sources.added'))
  }

  const isUrl = (item: string) => /^https?:\/\//i.test(item)

  return (
    <section aria-label={t('office.sources.panelTitle')} data-testid="session-sources">
      <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">{t('office.sources.panelTitle')}</h3>
      <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
        <form
          className="flex items-center gap-xs"
          onSubmit={e => {
            e.preventDefault()
            submit()
          }}
        >
          <Input
            value={draft}
            onChange={e => setDraft(e.target.value)}
            placeholder={t('office.sources.addPlaceholder')}
            aria-label={t('office.sources.addPlaceholder')}
            data-testid="session-source-input"
            disabled={!sessionId}
            className="flex-1 min-w-0 h-8 rounded-lg bg-surface-container-lowest/70"
          />
          <Button
            type="submit"
            variant="outline"
            size="sm"
            data-testid="session-source-add"
            disabled={!sessionId || !draft.trim()}
            className="shrink-0 px-sm py-xs rounded-lg font-label-md border-outline-variant/30 bg-surface-container-lowest/60 hover:bg-surface-container-low"
          >
            {t('office.sources.add')}
          </Button>
        </form>
        {sources.length === 0 ? (
          <p className="text-label-sm text-on-surface-variant" data-testid="session-sources-empty">
            {t('office.sources.empty')}
          </p>
        ) : (
          <ul className="space-y-xs">
            {sources.map(item => (
              <li
                key={item}
                data-testid="session-source-item"
                className="flex items-center gap-xs px-xs py-[3px] rounded-lg bg-surface-container-lowest/60 border border-outline-variant/10"
              >
                <span
                  className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0"
                  aria-hidden="true"
                >
                  {isUrl(item) ? 'link' : 'draft'}
                </span>
                <span className="flex-1 min-w-0 truncate font-label-sm text-on-surface" title={item}>
                  {item}
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={() => cite(item)}
                  title={t('office.sources.addToChat')}
                  aria-label={`${t('office.sources.addToChat')}: ${item}`}
                  data-testid="session-source-cite"
                  className="shrink-0 gap-xs px-xs py-[2px] text-tertiary hover:bg-tertiary-container/40"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">format_quote</span>
                  <span className="hidden xl:inline">{t('office.sources.addToChat')}</span>
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => onRemove(item)}
                  aria-label={`${t('extensions.datasources.remove')}: ${item}`}
                  data-testid="session-source-remove"
                  className="shrink-0 text-on-surface-variant hover:text-error hover:bg-surface-container"
                >
                  <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
                </Button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}

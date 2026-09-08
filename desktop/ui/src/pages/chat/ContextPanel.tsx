import { useState } from 'react'
import type { ToolCall, UsagePayload } from '@/types'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import { useSessions } from '@/context/SessionContext'
import { useSessionBudget } from '@/hooks/useSessionBudget'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import BudgetDialog from '@/components/chat/BudgetDialog'

interface ContextPanelProps {
  open: boolean
  usage: UsagePayload | null
  activeToolCalls: ToolCall[]
}

export default function ContextPanel({ open, usage, activeToolCalls }: ContextPanelProps) {
  const t = useT()
  const { currentSessionId } = useSessions()
  const { budget, usage: sessionUsage, refresh: refreshBudget } = useSessionBudget(currentSessionId)
  const [budgetOpen, setBudgetOpen] = useState(false)

  // Budget progress (spent/budget) — only rendered while a cap is set.
  const budgetSpent = sessionUsage?.cost_usd ?? 0
  const budgetPct = budget != null && budget > 0 ? Math.min(100, (budgetSpent / budget) * 100) : null
  const budgetBarColor = budgetPct != null && budgetPct >= 100 ? 'bg-error' : budgetPct != null && budgetPct >= 80 ? 'bg-warning' : 'bg-primary'

  return (
    <aside
      aria-label={t('chat.context.aria')}
      className="glass-panel shrink-0 overflow-y-auto p-lg border-l border-outline-variant/10 bg-surface-container-lowest/50 transition-all duration-300 ease-in-out"
      style={{
        width: open ? 300 : 0,
        padding: open ? undefined : 0,
        borderWidth: open ? undefined : 0,
        opacity: open ? 1 : 0,
      }}
    >
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
                      <div className={cn("h-full rounded-full transition-all duration-500", barColor)} style={{ width: `${pct}%` }} />
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
                  <div className={cn('h-full rounded-full transition-all duration-500', budgetBarColor)} style={{ width: `${budgetPct ?? 0}%` }} />
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
    </aside>
  )
}
// B2 follow-up — Tasks page sub-agent panel.
//
// Live registry view of `agent_spawn` results. Mirrors `BatchRunPanel`'s
// outer shell (a `section` with an icon heading + per-card list) but
// reads from `useSubagents` so it reflects the agent-teams coordinator
// directly. Hidden entirely when the user has not enabled agent teams —
// the list is empty in that case and the panel stays quiet.
//
// The card is intentionally small: the lead's chat timeline already
// renders the same spawn via `SubagentBlock`, this panel is a system-wide
// inventory so the user can see all live sub-agents in one place.

import { useIntl } from 'react-intl'
import { cn } from '@/lib/utils'
import { useSubagents } from '@/hooks/subagents'

function statusBadge(status: string): { dot: string; bg: string; labelId: string } {
  switch (status) {
    case 'running':
      return { dot: 'bg-primary animate-pulse', bg: 'bg-primary/10 border-primary/20', labelId: 'subagent.status.running' }
    case 'completed':
      return { dot: 'bg-success', bg: 'bg-success/10 border-success/20', labelId: 'subagent.status.completed' }
    case 'failed':
      return { dot: 'bg-error', bg: 'bg-error/10 border-error/20', labelId: 'subagent.status.failed' }
    default:
      return { dot: 'bg-on-surface-variant', bg: 'bg-surface-container border-outline-variant/30', labelId: 'subagent.status.idle' }
  }
}

export default function SubagentPanel() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)
  const { agents, loading } = useSubagents()

  if (agents.length === 0 && !loading) return null

  return (
    <section aria-labelledby="subagents-heading" className="mb-lg" data-testid="subagent-panel">
      <h2
        id="subagents-heading"
        className="font-label-lg font-bold text-on-surface mb-sm flex items-center gap-xs"
      >
        <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">
          account_tree
        </span>
        {t('subagent.panel.heading')}
      </h2>
      <div className="space-y-sm">
        {agents.map(a => {
          const badge = statusBadge(a.status)
          return (
            <div
              key={a.id}
              className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm bg-surface-container-lowest/80"
              data-testid="subagent-card"
              data-status={a.status}
            >
              <div className="flex items-start justify-between gap-sm">
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-xs">
                    <span
                      className={cn('size-2 rounded-full shrink-0', badge.dot)}
                      aria-hidden="true"
                    />
                    <span className="font-label-md font-semibold text-on-surface truncate">
                      {a.name}
                    </span>
                  </div>
                  <div className="flex flex-wrap items-center gap-sm mt-1 font-label-sm text-on-surface-variant">
                    <span className="font-mono text-[11px] px-1.5 py-0.5 rounded bg-surface-container shrink-0">
                      {a.model}
                    </span>
                    {a.team && (
                      <span className="font-label-xs px-1.5 py-0.5 rounded bg-surface-container">
                        {a.team}
                      </span>
                    )}
                    <span className="text-label-xs">
                      {t('subagent.turns', { used: a.turnsUsed, max: a.maxTurns })}
                    </span>
                  </div>
                </div>
                <span
                  className={cn(
                    'font-label-xs uppercase tracking-wide px-2 py-0.5 rounded border shrink-0',
                    badge.bg,
                  )}
                >
                  {t(badge.labelId)}
                </span>
              </div>
            </div>
          )
        })}
      </div>
    </section>
  )
}
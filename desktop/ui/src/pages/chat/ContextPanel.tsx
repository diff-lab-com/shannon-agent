import type { ToolCall, UsagePayload } from '@/types'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'

interface ContextPanelProps {
  open: boolean
  usage: UsagePayload | null
  activeToolCalls: ToolCall[]
}

export default function ContextPanel({ open, usage, activeToolCalls }: ContextPanelProps) {
  const t = useT()
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
      </div>
    </aside>
  )
}
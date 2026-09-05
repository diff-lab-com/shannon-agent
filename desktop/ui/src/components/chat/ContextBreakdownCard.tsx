// ContextBreakdownCard — P0-4 "Context composition" section of the chat
// ContextPanel.
//
// Loads the engine's six-category estimate (`get_session_context_breakdown`)
// and the session's cumulative usage (`get_session_usage`) and renders:
//   - a six-segment stacked bar (theme-semantic token colors) with a
//     keyboard-reachable per-category legend (tokens + share),
//   - the total and its share of the (possibly unknown) context window,
//   - the cache hit rate computed client-side as
//     `cache_read / (cache_read + input)` — "no data" on a zero denominator,
//   - the session's cumulative cost.
//
// Re-estimates whenever the streaming `usage` prop changes (each engine
// Usage event) so the card tracks the live conversation.

import { useEffect, useState } from 'react'
import { useT } from '@/i18n'
import { cn } from '@/lib/utils'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown, ContextBreakdownCategory } from '@/types'

/** Theme-semantic segment color per category key (stable, distinct). */
const CATEGORY_COLOR: Record<ContextBreakdownCategory['key'], string> = {
  system: 'bg-primary',
  tools: 'bg-secondary',
  skills: 'bg-tertiary',
  memory: 'bg-primary/40',
  mcp: 'bg-tertiary/40',
  conversation: 'bg-on-surface-variant/40',
}

const CATEGORY_DOT: Record<ContextBreakdownCategory['key'], string> = {
  system: 'bg-primary',
  tools: 'bg-secondary',
  skills: 'bg-tertiary',
  memory: 'bg-primary/60',
  mcp: 'bg-tertiary/60',
  conversation: 'bg-on-surface-variant/60',
}

function fmtTokens(n: number): string {
  return new Intl.NumberFormat(undefined, { notation: 'compact', maximumFractionDigits: 1 }).format(n)
}

export interface ContextBreakdownCardProps {
  sessionId: string | null
  /** Latest streaming Usage payload — a change triggers re-estimation. */
  usageTick: unknown
}

export default function ContextBreakdownCard({ sessionId, usageTick }: ContextBreakdownCardProps) {
  const t = useT()
  const [breakdown, setBreakdown] = useState<ContextBreakdown | null>(null)
  const [summary, setSummary] = useState<api.SessionUsageSummary | null>(null)

  useEffect(() => {
    let cancelled = false
    if (!sessionId) {
      setBreakdown(null)
      setSummary(null)
      return
    }
    void api.getSessionContextBreakdown(sessionId)
      .then(b => { if (!cancelled) setBreakdown(b) })
      .catch(() => { if (!cancelled) setBreakdown(null) })
    void api.getSessionUsage(sessionId)
      .then(u => { if (!cancelled) setSummary(u) })
      .catch(() => { if (!cancelled) setSummary(null) })
    return () => { cancelled = true }
  }, [sessionId, usageTick])

  const total = breakdown?.totalTokens ?? 0
  const window = breakdown?.contextWindow ?? null
  const windowPct = window && window > 0 ? Math.min(100, (total / window) * 100) : null

  // Frozen formula: cache_read / (cache_read + input), 0 denominator → none.
  const cacheDenominator = (summary?.cache_read_tokens ?? 0) + (summary?.input_tokens ?? 0)
  const cacheHitRate = summary && cacheDenominator > 0
    ? (summary.cache_read_tokens / cacheDenominator) * 100
    : null

  return (
    <section aria-label={t('chat.context.breakdown.title')}>
      <h3 className="font-label-md text-on-surface uppercase tracking-wider opacity-60 mb-md">
        {t('chat.context.breakdown.title')}
      </h3>
      <div className="p-md bg-surface-container rounded-xl border border-outline-variant/10 space-y-sm">
        {/* Stacked bar */}
        {total > 0 ? (
          <div
            role="img"
            aria-label={t('chat.context.breakdown.title')}
            className="flex w-full h-2 rounded-full overflow-hidden bg-surface-container-high"
          >
            {breakdown!.categories.map(c => (
              c.tokens > 0 && (
                <div
                  key={c.key}
                  className={cn('h-full', CATEGORY_COLOR[c.key])}
                  style={{ width: `${(c.tokens / total) * 100}%` }}
                />
              )
            ))}
          </div>
        ) : (
          <div className="w-full h-2 rounded-full bg-surface-container-high" />
        )}

        {/* Per-category legend — keyboard reachable rows */}
        <ul className="space-y-xs">
          {breakdown?.categories.map(c => {
            const share = total > 0 ? (c.tokens / total) * 100 : 0
            const shareText = share > 0 ? `${share.toFixed(0)}%` : '—'
            return (
              <li
                key={c.key}
                tabIndex={0}
                className="flex items-center gap-sm rounded-md px-xs py-[2px] text-body-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                aria-label={`${t(`chat.context.breakdown.cat.${c.key}`)}: ${c.tokens.toLocaleString()} (${shareText})`}
              >
                <span className={cn('w-2 h-2 rounded-full shrink-0', CATEGORY_DOT[c.key])} aria-hidden="true" />
                <span className="flex-1 text-on-surface-variant truncate">
                  {t(`chat.context.breakdown.cat.${c.key}`)}
                </span>
                <span className="font-mono text-label-sm text-on-surface tabular-nums">
                  {fmtTokens(c.tokens)}
                </span>
                <span className="font-mono text-label-sm text-outline-variant w-8 text-right tabular-nums">
                  {shareText}
                </span>
              </li>
            )
          })}
        </ul>

        {/* Total + window occupancy */}
        <div className="pt-sm border-t border-outline-variant/10 text-label-sm text-on-surface-variant">
          <div className="flex justify-between">
            <span>{t('chat.context.breakdown.total')}</span>
            <span className="font-mono font-bold text-on-surface tabular-nums">{fmtTokens(total)}</span>
          </div>
          <p className="mt-xs">
            {windowPct != null
              ? t('chat.context.breakdown.ofWindow', { pct: windowPct.toFixed(1), window: fmtTokens(window!) })
              : t('chat.context.breakdown.unknownWindow')}
          </p>
        </div>

        {/* Cache hit rate + session cost */}
        <div className="pt-sm border-t border-outline-variant/10 space-y-xs">
          <div className="flex justify-between text-body-sm">
            <span className="text-on-surface-variant">{t('chat.context.cacheHitRate')}</span>
            {cacheHitRate != null ? (
              <span className="font-bold text-primary tabular-nums">{cacheHitRate.toFixed(1)}%</span>
            ) : (
              <span className="text-on-surface-variant">{t('chat.context.cacheHitRate.none')}</span>
            )}
          </div>
          <div className="flex justify-between text-body-sm">
            <span className="text-on-surface-variant">{t('chat.context.sessionCost')}</span>
            <span className="font-bold text-primary tabular-nums">
              ${(summary?.cost_usd ?? 0).toFixed(4)}
            </span>
          </div>
        </div>
      </div>
    </section>
  )
}

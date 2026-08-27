// TurnTimeline — §4.14 panel visualizing the inside of a session's turns.
//
// Data comes from the `trace_timeline` Tauri command (an L0 projection built
// by `project_turn_timeline` in shannon-core): per-turn windows with their
// tool waterfall rows plus a running token/cost curve sampled at each
// `turn/end`. This component is pure rendering — it never reads events.jsonl
// itself and re-derives nothing.
//
// Layout:
//   - sticky summary header (model · turns · tools · tokens · cost)
//   - cumulative curve card (SVG polyline over `cumulative`)
//   - one card per turn: reason badge, usage chips, tool waterfall rows
//
// Icons follow the Material Symbols policy (`<Icon>` wrapper); all
// user-visible strings come from i18n (`timeline.*`, en + zh-CN together).

import { useEffect, useMemo, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { getTraceTimeline } from '@/lib/tauri-api'
import type { TimelineCumulativePoint, TimelineTurn, TurnTimeline } from '@/types'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { CardSkeleton } from '@/components/SkeletonLoader'
import ErrorState from '@/components/ui/error-state'
import { Icon } from '@/components/ui/icon'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'

/** Percentage span floor so sub-second calls stay clickable-looking. */
const MIN_ROW_WIDTH_PCT = 2

function formatDuration(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return '—'
  if (ms < 1000) return `${ms}ms`
  const s = ms / 1000
  return `${s.toFixed(s < 10 ? 1 : 0)}s`
}

function formatTime(tsNs: number): string {
  return new Date(tsNs / 1e6).toLocaleTimeString([], {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

const nf = new Intl.NumberFormat()
const cf = new Intl.NumberFormat(undefined, {
  style: 'currency',
  currency: 'USD',
})

export interface TurnTimelineProps {
  /** Session id override (embeddable reuse); defaults to the route param. */
  sessionId?: string
}

export default function TurnTimeline({ sessionId }: TurnTimelineProps) {
  const t = useT()
  const navigate = useNavigate()
  const routeId = useParams().id ?? ''
  const id = sessionId ?? routeId

  const [timeline, setTimeline] = useState<TurnTimeline | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!id) {
      setError('missing-session')
      setLoading(false)
      return
    }
    let cancelled = false
    setLoading(true)
    setError(null)
    getTraceTimeline(id)
      .then(tl => {
        if (!cancelled) setTimeline(tl)
      })
      .catch((e: unknown) => {
        console.warn('trace_timeline failed:', e)
        if (!cancelled) setError(String(e))
      })
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
  }, [id])

  // Session-wide window: every bar position is relative to this range.
  const span = timeline && timeline.ended_ts_ns > timeline.started_ts_ns
    ? timeline.ended_ts_ns - timeline.started_ts_ns
    : 1

  const totalTools = useMemo(
    () => timeline?.turns.reduce((acc, tu) => acc + tu.tools.length, 0) ?? 0,
    [timeline],
  )
  const totalOutputTokens = useMemo(
    () =>
      timeline?.cumulative[timeline.cumulative.length - 1]?.output_tokens_total ??
      timeline?.turns.reduce((a, tu) => a + tu.output_tokens, 0) ??
      0,
    [timeline],
  )
  const totalCost = [...(timeline?.cumulative ?? [])]
    .reverse()
    .find(p => p.cost_total_usd != null)?.cost_total_usd

  if (loading) {
    return (
      <div className="p-6 space-y-3" aria-busy="true">
        <CardSkeleton />
        <CardSkeleton />
      </div>
    )
  }

  if (error || !timeline) {
    return (
      <div className="p-6">
        <ErrorState
          icon="error"
          title={t('timeline.error.title')}
          description={t('timeline.error.hint')}
          action={{ label: t('timeline.back'), onClick: () => navigate('/chat') }}
        />
      </div>
    )
  }

  return (
    <div className="h-full flex flex-col" data-testid="turn-timeline">
      {/* Summary header */}
      <div className="flex items-center gap-2 px-4 pt-4 pb-2 shrink-0">
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => navigate('/chat')}
          aria-label={t('timeline.back')}
        >
          <Icon name="arrow_back" />
        </Button>
        <div className="min-w-0">
          <h1 className="font-title-lg text-lg font-semibold text-on-surface truncate">
            {t('timeline.title')}
          </h1>
          <p className="font-label-sm text-label-sm text-on-surface-variant truncate">
            {t('timeline.subtitle', {
              model: timeline.model ?? t('timeline.model.unknown'),
            })}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-1.5 shrink-0" role="list" aria-label={t('timeline.summary.aria')}>
          <SummaryChip icon="schema" label={t('timeline.stat.turns', { count: timeline.turns.length })} />
          <SummaryChip icon="build" label={t('timeline.stat.tools', { count: totalTools })} />
          <SummaryChip icon="token" label={nf.format(totalOutputTokens)} />
          {totalCost != null && (
            <SummaryChip icon="payments" label={cf.format(totalCost)} />
          )}
        </div>
      </div>

      <ScrollArea className="flex-1 min-h-0 px-4 pb-6">
        <div className="max-w-3xl mx-auto space-y-4">
          {/* Cumulative token/cost curve */}
          {timeline.cumulative.length > 0 && (
            <Card>
              <CardHeader className="pb-0">
                <CardTitle className="text-sm font-medium flex items-center gap-1.5">
                  <Icon name="monitoring" size="sm" />
                  {t('timeline.curve.title')}
                </CardTitle>
              </CardHeader>
              <CardContent className="pt-2">
                <CumulativeCurve cumulative={timeline.cumulative} />
              </CardContent>
            </Card>
          )}

          {timeline.turns.length === 0 ? (
            <EmptyTurns />
          ) : (
            timeline.turns.map(turn => (
              <TurnCard
                key={`${turn.turn}-${turn.start_ts_ns}`}
                turn={turn}
                startedTs={timeline.started_ts_ns}
                spanNs={span}
              />
            ))
          )}
        </div>
      </ScrollArea>
    </div>
  )
}

function SummaryChip({ icon, label }: { icon: string; label: string }) {
  return (
    <span
      role="listitem"
      className="inline-flex items-center gap-1 rounded-full bg-surface-container-low px-2 py-1 font-label-sm text-label-sm text-on-surface-variant border border-outline-variant/30"
    >
      <Icon name={icon} size="xs" />
      {label}
    </span>
  )
}

function EmptyTurns() {
  const t = useT()
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-outline-variant/40 py-14 text-center">
      <Icon name="timeline" size="xl" className="text-on-surface-variant/50 mb-2" />
      <p className="font-label-md text-on-surface">{t('timeline.empty.title')}</p>
      <p className="font-label-sm text-on-surface-variant mt-1">
        {t('timeline.empty.hint')}
      </p>
    </div>
  )
}

/**
 * SVG polyline over the token accumulation samples (and a faint cost line
 * when costs exist). X = sample ts across the session span, Y = value.
 */
function CumulativeCurve({
  cumulative,
}: {
  cumulative: TimelineCumulativePoint[]
}) {
  const t = useT()
  const W = 560
  const H = 96
  const xs = cumulative.map(p => p.ts_ns)
  const x0 = Math.min(...xs)
  const x1 = Math.max(...xs)
  const yMax = Math.max(...cumulative.map(p => p.output_tokens_total), 1)
  const point = (i: number, v: number, vMax: number): [number, number] => [
    ((xs[i] - x0) / Math.max(x1 - x0, 1)) * (W - 8) + 4,
    H - 6 - (v / vMax) * (H - 16),
  ]
  const tokenPath = cumulative
    .map((p, i) => point(i, p.output_tokens_total, yMax).join(','))
    .join(' ')
  const costKnown = cumulative.some(p => p.cost_total_usd != null)

  return (
    <figure>
      <svg
        viewBox={`0 0 ${W} ${H}`}
        className="w-full h-24"
        role="img"
        aria-label={t('timeline.curve.aria')}
      >
        <polyline
          points={tokenPath}
          fill="none"
          stroke="currentColor"
          strokeWidth={2}
          className="text-primary"
        />
        {costKnown && (
          <polyline
            points={cumulative
              .map((p, i) => point(i, p.cost_total_usd ?? 0, yMax).join(','))
              .join(' ')}
            fill="none"
            strokeWidth={1.5}
            strokeDasharray="4 3"
            className="text-on-surface-variant/60"
          />
        )}
      </svg>
      <figcaption className="mt-1 flex items-center justify-between font-label-xs text-xs text-on-surface-variant">
        <span>{formatTime(cumulative[0]?.ts_ns ?? 0)}</span>
        <span>{t('timeline.curve.tokens', { count: yMax })}</span>
        <span>{formatTime(x1)}</span>
      </figcaption>
    </figure>
  )
}

function TurnCard({
  turn,
  startedTs,
  spanNs,
}: {
  turn: TimelineTurn
  startedTs: number
  spanNs: number
}) {
  const t = useT()
  const reasonKey = `timeline.reason.${turn.reason ?? 'unknown'}`
  // Reason labels are enumerated in i18n; fall back to the raw reason.
  const reasonLabel = turn.reason ? t(reasonKey, {}) : ''

  return (
    <Card data-testid={`timeline-turn-${turn.turn}`}>
      <CardHeader className="pb-2">
        <div className="flex items-center gap-2 flex-wrap">
          <CardTitle className="text-sm font-semibold">
            {t('timeline.turn.label', { n: turn.turn })}
          </CardTitle>
          {turn.reason && (
            <span
              className={cn(
                'inline-flex items-center gap-1 rounded-full px-2 py-0.5 font-label-xs text-xs border',
                turn.reason === 'completed' &&
                  'bg-primary/10 text-primary border-primary/30',
                turn.reason !== 'completed' &&
                  'bg-error/10 text-error border-error/30',
              )}
            >
              {reasonLabel}
            </span>
          )}
          <span className="ml-auto font-label-xs text-xs text-on-surface-variant">
            {formatTime(turn.start_ts_ns)} → {formatTime(turn.end_ts_ns)}
          </span>
        </div>
        <div className="flex items-center gap-3 font-label-xs text-xs text-on-surface-variant pt-1">
          <span className="inline-flex items-center gap-1">
            <Icon name="login" size="xs" />↓ {nf.format(turn.input_tokens)}
          </span>
          <span className="inline-flex items-center gap-1">
            <Icon name="logout" size="xs" />↑ {nf.format(turn.output_tokens)}
          </span>
          <span className="inline-flex items-center gap-1">
            <Icon name="cached" size="xs" />
            ↻ {nf.format(turn.cache_read_tokens)}/{nf.format(turn.cache_creation_tokens)}
          </span>
          {turn.cost_usd != null && (
            <span className="inline-flex items-center gap-1">
              <Icon name="payments" size="xs" />
              {cf.format(turn.cost_usd)}
            </span>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-1.5 pt-0">
        {turn.tools.length === 0 ? (
          <p className="font-label-sm text-label-sm text-on-surface-variant italic">
            {t('timeline.turn.noTools')}
          </p>
        ) : (
          turn.tools.map(tool => {
            const leftPct =
              ((tool.start_ts_ns - startedTs) / spanNs) * 100
            const widthPct =
              Math.max(((tool.end_ts_ns - tool.start_ts_ns) / spanNs) * 100, MIN_ROW_WIDTH_PCT)
            return (
              <div key={tool.tool_use_id} className="group relative h-7">
                <div className="absolute inset-x-0 top-1/2 -translate-y-1/2 h-px bg-outline-variant/40" />
                <div
                  className={cn(
                    'absolute top-1/2 -translate-y-1/2 h-5 rounded-md flex items-center gap-1 px-1.5 overflow-hidden whitespace-nowrap',
                    tool.is_error
                      ? 'bg-error/15 border border-error/40'
                      : 'bg-secondary-container/70',
                  )}
                  style={{ left: `${Math.max(leftPct, 0)}%`, width: `${widthPct}%` }}
                  title={`${tool.tool_name}${tool.duration_ms ? ` · ${formatDuration(tool.duration_ms)}` : ''}`}
                >
                  {tool.is_error && (
                    <Icon name="error" size="xs" className="text-error shrink-0" />
                  )}
                  <span className="font-label-xs text-[11px] text-on-surface truncate">
                    {tool.tool_name}
                  </span>
                  <span className="ml-auto font-label-xs text-[11px] text-on-surface-variant pl-1 shrink-0">
                    {formatDuration(tool.duration_ms)}
                  </span>
                </div>
              </div>
            )
          })
        )}
      </CardContent>
    </Card>
  )
}

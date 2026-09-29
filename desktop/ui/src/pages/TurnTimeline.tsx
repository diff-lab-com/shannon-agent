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
import { save } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { getTraceTimeline, saveTextFile } from '@/lib/tauri-api'
import { timelineToHtml } from '@/lib/timelineExport'
import type { TimelineCumulativePoint, TimelineTurn, TurnTimeline } from '@/types'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { CardSkeleton } from '@/components/SkeletonLoader'
import ErrorState from '@/components/ui/error-state'
import { Icon } from '@/components/ui/icon'
import { cn } from '@/lib/utils'
import { toastError } from '@/lib/errorToast'
import { useT } from '@/i18n'
import { useIntl } from 'react-intl'

/** Percentage span floor so sub-second calls stay clickable-looking. */
const MIN_ROW_WIDTH_PCT = 2

// §7-28 (P1-27 adjacent, timeline side): turn-end reasons split into three
// visual tones. Only genuine failures read as errors — `interrupted` /
// `max-turns` are neutral stopping conditions, not crashes.
type ReasonTone = 'success' | 'neutral' | 'error'

const REASON_TONES: Record<string, ReasonTone> = {
  'completed': 'success',
  'failed': 'error',
  'timeout': 'error',
  'budget-exceeded': 'error',
  'interrupted': 'neutral',
  'max-turns': 'neutral',
  'unknown': 'neutral',
}

function reasonTone(reason: string): ReasonTone {
  return REASON_TONES[reason] ?? 'neutral'
}

function formatDuration(ms: number | null | undefined): string {
  if (!ms || ms <= 0) return '—'
  if (ms < 1000) return `${ms}ms`
  const s = ms / 1000
  return `${s.toFixed(s < 10 ? 1 : 0)}s`
}

// §7-28: timestamps format in the APP's locale (passed in by the render
// tree), not whatever the OS happens to be set to.
function formatTime(tsNs: number, locale: string): string {
  return new Date(tsNs / 1e6).toLocaleTimeString(locale, {
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  })
}

/* ────────────────────  office Wave 3 C6: HTML export  ──────────────────── */

/**
 * C6: serialize the already-loaded timeline projection into a self-contained
 * HTML document — inline styles only, no scripts, fonts or network calls.
 * One section per turn (timestamp range + reason status color + usage chips
 * + tool waterfall rows as a plain list). Exported for tests.
 */
const COST_FORMAT = new Intl.NumberFormat(undefined, {
  style: 'currency',
  currency: 'USD',
})

const nf = new Intl.NumberFormat()

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

  // C6: export the loaded projection to a self-contained HTML file at a
  // user-chosen path (save dialog + saveTextFile). Cancelling the dialog
  // (null) backs out silently; a write failure toasts the cause.
  const [exporting, setExporting] = useState(false)
  const handleExportHtml = async () => {
    if (!timeline) return
    setExporting(true)
    try {
      const path = await save({
        defaultPath: `timeline-${timeline.session_id || id || 'session'}.html`,
        filters: [{ name: 'HTML', extensions: ['html'] }],
      })
      if (!path) return
      await saveTextFile(path, timelineToHtml(timeline))
      toast.success(t('office.timeline.exported'))
    } catch (err) {
      toastError(t('chat.artifact.exportFailed'), err)
    } finally {
      setExporting(false)
    }
  }

  if (loading) {
    return (
      <div className="p-lg space-y-3" aria-busy="true">
        <CardSkeleton />
        <CardSkeleton />
      </div>
    )
  }

  if (error || !timeline) {
    return (
      <div className="p-lg">
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
      <div className="flex items-center gap-sm px-md pt-md pb-sm shrink-0">
        <Button
          variant="ghost"
          size="icon-sm"
          onClick={() => navigate('/chat')}
          aria-label={t('timeline.back')}
        >
          <Icon name="arrow_back" />
        </Button>
        {/* Page title ("时间线") renders in the persistent Header via
            TITLE_MAP (`header.title.timeline`) — this row keeps only the
            back affordance, the model subtitle and the summary chips. */}
        <div className="min-w-0">
          <p className="font-label-sm text-label-sm text-on-surface-variant truncate">
            {t('timeline.subtitle', {
              model: timeline.model ?? t('timeline.model.unknown'),
            })}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-1.5 shrink-0" role="list" aria-label={t('timeline.summary.aria')}>
          {/* C6: self-contained HTML export of the loaded projection. */}
          <Button
            variant="ghost"
            size="sm"
            data-testid="timeline-export-html"
            aria-label={t('office.timeline.exportHtml')}
            title={t('office.timeline.exportHtml')}
            disabled={exporting}
            onClick={() => void handleExportHtml()}
            className="text-on-surface-variant hover:text-primary hover:bg-surface-container"
          >
            <Icon name="download" size="sm" />
          </Button>
          <SummaryChip icon="schema" label={t('timeline.stat.turns', { count: timeline.turns.length })} />
          <SummaryChip icon="build" label={t('timeline.stat.tools', { count: totalTools })} />
          <SummaryChip icon="token" label={nf.format(totalOutputTokens)} />
          {totalCost != null && (
            <SummaryChip icon="payments" label={COST_FORMAT.format(totalCost)} />
          )}
        </div>
      </div>

      <ScrollArea className="flex-1 min-h-0 px-md pb-lg">
        <div className="max-w-narrow mx-auto space-y-4">
          {/* Cumulative token/cost curve */}
          {timeline.cumulative.length > 0 && (
            <Card>
              <CardHeader className="pb-0">
                <CardTitle className="text-sm font-medium flex items-center gap-1.5">
                  <Icon name="monitoring" size="sm" />
                  {t('timeline.curve.title')}
                </CardTitle>
              </CardHeader>
              <CardContent className="pt-sm">
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
      className="inline-flex items-center gap-xs rounded-full bg-surface-container-low px-sm py-xs font-label-sm text-label-sm text-on-surface-variant border border-outline-variant/30"
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
      <Icon name="timeline" size="xl" className="text-on-surface-variant/50 mb-sm" />
      <p className="font-label-md text-on-surface">{t('timeline.empty.title')}</p>
      <p className="font-label-sm text-on-surface-variant mt-xs">
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
  const intl = useIntl()
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
  // Cost is plotted against its own 0..costMax scale — sharing the token
  // axis would flatten a few cents against tens of thousands of tokens,
  // drawing a meaningless line glued to the bottom edge.
  const costMax = Math.max(...cumulative.map(p => p.cost_total_usd ?? 0), 0)
  const showCost = cumulative.some(p => p.cost_total_usd != null) && costMax > 0

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
        {showCost && (
          <polyline
            points={cumulative
              .map((p, i) => point(i, p.cost_total_usd ?? 0, costMax).join(','))
              .join(' ')}
            fill="none"
            strokeWidth={1.5}
            strokeDasharray="4 3"
            className="text-on-surface-variant/60"
          />
        )}
      </svg>
      <figcaption className="mt-xs flex items-center justify-between font-label-xs text-xs text-on-surface-variant">
        <span>{formatTime(cumulative[0]?.ts_ns ?? 0, intl.locale)}</span>
        <span>
          {t('timeline.curve.tokens', { count: yMax })}
          <span aria-hidden="true" className="mx-xs">·</span>
          {t('timeline.curve.samples', { count: cumulative.length })}
          {showCost && (
            <>
              <span aria-hidden="true" className="mx-xs">·</span>
              {t('timeline.curve.cost', { cost: COST_FORMAT.format(costMax) })}
            </>
          )}
        </span>
        <span>{formatTime(x1, intl.locale)}</span>
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
  const intl = useIntl()
  // §7-28: enumerated reasons resolve through i18n; anything the backend
  // adds later renders as its raw reason text instead of a literal i18n key
  // like "timeline.reason.new-thing".
  const reasonKnown = turn.reason != null && turn.reason in REASON_TONES
  const reasonLabel = turn.reason
    ? reasonKnown
      ? t(`timeline.reason.${turn.reason}`)
      : turn.reason
    : ''
  const tone = turn.reason ? reasonTone(turn.reason) : null

  return (
    <Card data-testid={`timeline-turn-${turn.turn}`}>
      <CardHeader className="pb-sm">
        <div className="flex items-center gap-sm flex-wrap">
          <CardTitle className="text-sm font-semibold">
            {t('timeline.turn.label', { n: turn.turn })}
          </CardTitle>
          {turn.reason && (
            <span
              className={cn(
                'inline-flex items-center gap-xs rounded-full px-sm py-0.5 font-label-xs text-xs border',
                tone === 'success' && 'bg-primary/10 text-primary border-primary/30',
                tone === 'error' && 'bg-error/10 text-error border-error/30',
                tone === 'neutral' && 'bg-surface-container-high text-on-surface-variant border-outline-variant/30',
              )}
            >
              {reasonLabel}
            </span>
          )}
          <span className="ml-auto font-label-xs text-xs text-on-surface-variant">
            {formatTime(turn.start_ts_ns, intl.locale)} → {formatTime(turn.end_ts_ns, intl.locale)}
          </span>
        </div>
        <div className="flex items-center gap-3 font-label-xs text-xs text-on-surface-variant pt-xs">
          <span className="inline-flex items-center gap-xs">
            <Icon name="login" size="xs" />↓ {nf.format(turn.input_tokens)}
          </span>
          <span className="inline-flex items-center gap-xs">
            <Icon name="logout" size="xs" />↑ {nf.format(turn.output_tokens)}
          </span>
          <span className="inline-flex items-center gap-xs">
            <Icon name="cached" size="xs" />
            ↻ {nf.format(turn.cache_read_tokens)}/{nf.format(turn.cache_creation_tokens)}
          </span>
          {turn.cost_usd != null && (
            <span className="inline-flex items-center gap-xs">
              <Icon name="payments" size="xs" />
              {COST_FORMAT.format(turn.cost_usd)}
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
            // A bar narrower than ~15% of the row cannot fit its label
            // ("Grep · 1.2s" ≈ 70px on a ~700px card) — overflow-hidden used
            // to clip it into an unreadable sliver. Short bars render the
            // label just outside the pill instead; when that would run off
            // the right edge, flip to the left of the bar start.
            const barLeft = Math.max(leftPct, 0)
            const fitsInside = widthPct >= 15
            const label = `${tool.tool_name}${tool.duration_ms ? ` · ${formatDuration(tool.duration_ms)}` : ''}`
            const outsideLeft = barLeft + widthPct + 0.5
            const flipLeft = outsideLeft + 18 > 100 && barLeft > 30
            return (
              <div key={tool.tool_use_id} className="group relative h-7">
                <div className="absolute inset-x-0 top-1/2 -translate-y-1/2 h-px bg-outline-variant/40" />
                <div
                  className={cn(
                    'absolute top-1/2 -translate-y-1/2 h-5 rounded-md flex items-center gap-xs px-1.5 overflow-hidden whitespace-nowrap',
                    tool.is_error
                      ? 'bg-error/15 border border-error/40'
                      : 'bg-secondary-container/70',
                  )}
                  style={{ left: `${barLeft}%`, width: `${widthPct}%` }}
                  title={label}
                >
                  {tool.is_error && (
                    <Icon name="error" size="xs" className="text-error shrink-0" />
                  )}
                  {fitsInside && (
                    <>
                      <span className="font-label-xs text-label-xs text-on-surface truncate">
                        {tool.tool_name}
                      </span>
                      <span className="ml-auto font-label-xs text-label-xs text-on-surface-variant pl-xs shrink-0">
                        {formatDuration(tool.duration_ms)}
                      </span>
                    </>
                  )}
                </div>
                {!fitsInside && (
                  <span
                    className={cn(
                      'absolute top-1/2 -translate-y-1/2 whitespace-nowrap font-label-xs text-label-xs',
                      tool.is_error ? 'text-error' : 'text-on-surface',
                    )}
                    style={
                      flipLeft
                        ? { right: `${100 - barLeft + 0.5}%` }
                        : { left: `${outsideLeft}%` }
                    }
                  >
                    <span>{tool.tool_name}</span>
                    {tool.duration_ms && (
                      <span className="pl-xs text-on-surface-variant">
                        · {formatDuration(tool.duration_ms)}
                      </span>
                    )}
                  </span>
                )}
              </div>
            )
          })
        )}
      </CardContent>
    </Card>
  )
}

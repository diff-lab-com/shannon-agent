// Perf page — developer panel for tracing JSON analysis.
//
// INPUT: newline-delimited JSON produced by `SHANNON_LOG_FORMAT=json` (see
// main.rs E5 setup). Paste the log output into the textarea and hit Analyze.
//
// OUTPUT: three sections.
//   1. Command latency: per-command count, p50, p95, max (in ms).
//   2. Slow Top 10: slowest individual spans by busy time.
//   3. Tool call counts: events tagged with `tool_name`, grouped by name.
//
// WHY: the tracing-subscriber JSON exporter writes to stderr; this page lets
// you load captured output for offline analysis. No live streaming — the
// log file is the source of truth, by design.

import { useMemo, useState } from 'react'
import EmptyState from '@/components/ui/empty-state'

// ─── Types ─────────────────────────────────────────────────────────────────

interface TracingEvent {
  timestamp?: string
  level?: string
  fields?: Record<string, unknown>
  target?: string
  spans?: Array<{ name: string; attributes?: Record<string, unknown> }>
}

interface CommandStat {
  name: string
  count: number
  p50: number
  p95: number
  max: number
}

interface SlowSpan {
  name: string
  durationMs: number
  attributes?: Record<string, unknown>
  timestamp?: string
}

// ─── Parsing helpers ───────────────────────────────────────────────────────

// tracing-subscriber emits human durations like "123µs", "1.2ms", "3s".
// Parse into milliseconds.
export function parseDurationMs(raw: string | undefined): number {
  if (!raw) return 0
  const s = raw.trim()
  const m = s.match(/^([\d.]+)(ns|µs|us|ms|s)?$/)
  if (!m) return 0
  const n = parseFloat(m[1])
  const unit = m[2] ?? 'ms'
  switch (unit) {
    case 'ns': return n / 1_000_000
    case 'µs':
    case 'us': return n / 1_000
    case 'ms': return n
    case 's': return n * 1_000
    default: return n
  }
}

export function percentile(sorted: number[], p: number): number {
  if (sorted.length === 0) return 0
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)
  return sorted[Math.max(0, idx)]
}

// Only consider events that mark a span close: tracing-subscriber emits these
// with fields.message === 'close' when FmtSpan::CLOSE is enabled (our default).
function isSpanClose(ev: TracingEvent): boolean {
  return ev.fields?.message === 'close' || ev.fields?.message === 'close;'
}

function innermostSpan(ev: TracingEvent): { name: string; attributes?: Record<string, unknown> } | null {
  if (!ev.spans || ev.spans.length === 0) return null
  return ev.spans[ev.spans.length - 1]
}

export interface PerfAnalysis {
  commands: CommandStat[]
  slowTop10: SlowSpan[]
  toolCounts: Array<{ name: string; count: number }>
  totalEvents: number
  parseErrors: number
}

export function analyzeTracingJson(text: string): PerfAnalysis {
  const lines = text.split(/\r?\n/).filter(l => l.trim().startsWith('{'))
  const closeEvents: Array<{ ev: TracingEvent; span: NonNullable<ReturnType<typeof innermostSpan>>; durationMs: number }> = []
  const toolCounter = new Map<string, number>()
  let parseErrors = 0

  for (const line of lines) {
    let parsed: TracingEvent
    try {
      parsed = JSON.parse(line) as TracingEvent
    } catch {
      parseErrors++
      continue
    }
    // Tool call counting — any event (close or otherwise) with a tool_name field.
    const toolName = parsed.fields?.tool_name ?? parsed.fields?.tool
    if (typeof toolName === 'string') {
      toolCounter.set(toolName, (toolCounter.get(toolName) ?? 0) + 1)
    }
    if (isSpanClose(parsed)) {
      const span = innermostSpan(parsed)
      if (!span) continue
      const busy = typeof parsed.fields?.['time.busy'] === 'string'
        ? parseDurationMs(parsed.fields['time.busy'] as string)
        : 0
      const idle = typeof parsed.fields?.['time.idle'] === 'string'
        ? parseDurationMs(parsed.fields['time.idle'] as string)
        : 0
      closeEvents.push({ ev: parsed, span, durationMs: busy || idle })
    }
  }

  // Group close events by span name → per-command stats.
  const byName = new Map<string, number[]>()
  for (const ce of closeEvents) {
    const arr = byName.get(ce.span.name) ?? []
    arr.push(ce.durationMs)
    byName.set(ce.span.name, arr)
  }
  const commands: CommandStat[] = Array.from(byName.entries()).map(([name, durs]) => {
    const sorted = [...durs].sort((a, b) => a - b)
    return {
      name,
      count: sorted.length,
      p50: percentile(sorted, 50),
      p95: percentile(sorted, 95),
      max: sorted[sorted.length - 1],
    }
  }).sort((a, b) => b.max - a.max)

  const slowTop10: SlowSpan[] = closeEvents
    .slice()
    .sort((a, b) => b.durationMs - a.durationMs)
    .slice(0, 10)
    .map(ce => ({
      name: ce.span.name,
      durationMs: ce.durationMs,
      attributes: ce.span.attributes,
      timestamp: ce.ev.timestamp,
    }))

  const toolCounts = Array.from(toolCounter.entries())
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count)

  return {
    commands,
    slowTop10,
    toolCounts,
    totalEvents: lines.length - parseErrors,
    parseErrors,
  }
}

// ─── View ──────────────────────────────────────────────────────────────────

function Histogram({ commands }: { commands: CommandStat[] }) {
  // Buckets by p95 latency: <10ms / 10-100ms / 100ms-1s / >1s.
  const buckets = useMemo(() => {
    const b = { fast: 0, ok: 0, slow: 0, critical: 0 }
    for (const c of commands) {
      if (c.p95 < 10) b.fast++
      else if (c.p95 < 100) b.ok++
      else if (c.p95 < 1000) b.slow++
      else b.critical++
    }
    return b
  }, [commands])
  const maxBucket = Math.max(1, buckets.fast, buckets.ok, buckets.slow, buckets.critical)
  const rows = [
    { label: '< 10ms', color: 'bg-tertiary', count: buckets.fast },
    { label: '10ms–100ms', color: 'bg-primary', count: buckets.ok },
    { label: '100ms–1s', color: 'bg-warning', count: buckets.slow },
    { label: '> 1s', color: 'bg-error', count: buckets.critical },
  ]
  return (
    <div className="space-y-sm">
      {rows.map(r => (
        <div key={r.label} className="flex items-center gap-sm">
          <span className="font-label-sm text-label-sm text-on-surface-variant w-32 shrink-0">{r.label}</span>
          <div className="flex-1 h-3 bg-surface-container-low rounded-full overflow-hidden">
            <div className={`h-full ${r.color} transition-all`} style={{ width: `${(r.count / maxBucket) * 100}%` }} />
          </div>
          <span className="font-label-sm text-label-sm text-on-surface font-mono w-8 text-right">{r.count}</span>
        </div>
      ))}
    </div>
  )
}

const SAMPLE_HINT = `Run the desktop app with SHANNON_LOG_FORMAT=json and paste captured stderr here.
Each line must be a single JSON object emitted by tracing-subscriber.`

export default function Perf() {
  const [text, setText] = useState('')
  const [analyzed, setAnalyzed] = useState<PerfAnalysis | null>(null)
  const [error, setError] = useState<string | null>(null)

  const onAnalyze = () => {
    if (!text.trim()) {
      setAnalyzed(null)
      setError('Paste at least one line of tracing JSON.')
      return
    }
    try {
      setError(null)
      setAnalyzed(analyzeTracingJson(text))
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to analyze JSON')
      setAnalyzed(null)
    }
  }

  const onClear = () => {
    setText('')
    setAnalyzed(null)
    setError(null)
  }

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        <header className="mb-xl">
          <h2 className="font-headline-lg text-headline-lg text-on-surface flex items-center gap-sm">
            <span className="material-symbols-outlined text-primary">bar_chart</span>
            Performance
          </h2>
          <p className="text-on-surface-variant mt-xs text-body-sm">
            Developer panel: analyze tracing JSON captured from <code className="px-xs py-0.5 rounded bg-surface-container-low font-mono text-label-sm">SHANNON_LOG_FORMAT=json</code>.
          </p>
        </header>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-lg mb-lg">
          <section aria-label="Trace input" className="flex flex-col">
            <label htmlFor="perf-textarea" className="font-label-md text-on-surface mb-sm">
              Trace JSON (newline-delimited)
            </label>
            <textarea
              id="perf-textarea"
              value={text}
              onChange={e => setText(e.target.value)}
              placeholder={SAMPLE_HINT}
              aria-describedby="perf-hint"
              className="flex-1 min-h-[260px] font-mono text-label-sm p-md rounded-xl border border-outline-variant/30 bg-surface-container-lowest focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 resize-y"
            />
            <p id="perf-hint" className="font-label-sm text-on-surface-variant mt-xs">
              {SAMPLE_HINT}
            </p>
            <div className="flex items-center gap-sm mt-md">
              <Button onClick={onAnalyze}>Analyze</Button>
              <Button variant="ghost" onClick={onClear}>Clear</Button>
              {error && <span className="text-error font-label-md text-label-md" role="alert">{error}</span>}
            </div>
          </section>

          {!analyzed ? (
            <EmptyState
              icon="analytics"
              title="No analysis yet"
              description="Paste tracing JSON on the left and click Analyze."
            />
          ) : (
            <section aria-label="Summary" className="p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <h3 className="font-label-md text-on-surface font-bold uppercase tracking-wider mb-md">Summary</h3>
              <dl className="grid grid-cols-2 gap-md">
                <div>
                  <dt className="font-label-sm text-on-surface-variant">Events parsed</dt>
                  <dd className="font-headline-md text-headline-md text-on-surface" data-testid="events-parsed">{analyzed.totalEvents}</dd>
                </div>
                <div>
                  <dt className="font-label-sm text-on-surface-variant">Unique commands</dt>
                  <dd className="font-headline-md text-headline-md text-on-surface" data-testid="unique-commands">{analyzed.commands.length}</dd>
                </div>
                <div>
                  <dt className="font-label-sm text-on-surface-variant">Tool call types</dt>
                  <dd className="font-headline-md text-headline-md text-on-surface" data-testid="tool-types">{analyzed.toolCounts.length}</dd>
                </div>
                <div>
                  <dt className="font-label-sm text-on-surface-variant">Parse errors</dt>
                  <dd className="font-headline-md text-headline-md text-on-surface" data-testid="parse-errors">{analyzed.parseErrors}</dd>
                </div>
              </dl>
            </section>
          )}
        </div>

        {analyzed && (
          <>
            <section aria-label="Latency histogram" className="mb-lg p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <h3 className="font-label-md text-on-surface font-bold uppercase tracking-wider mb-md">
                Latency histogram (by p95 bucket)
              </h3>
              {analyzed.commands.length === 0 ? (
                <p className="font-label-sm text-on-surface-variant">No span-close events found. Check that the trace includes <code>close</code> markers (FmtSpan::CLOSE).</p>
              ) : (
                <Histogram commands={analyzed.commands} />
              )}
            </section>

            <section aria-label="Slow Top 10" className="mb-lg p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <h3 className="font-label-md text-on-surface font-bold uppercase tracking-wider mb-md">Slow Top 10 spans</h3>
              {analyzed.slowTop10.length === 0 ? (
                <p className="font-label-sm text-on-surface-variant">No span-close events.</p>
              ) : (
                <ol className="space-y-sm" data-testid="slow-top">
                  {analyzed.slowTop10.map((s, i) => (
                    <li key={`${s.name}-${i}`} className="flex items-center gap-md p-sm rounded-lg bg-surface-container-low">
                      <span className="font-mono font-bold text-label-sm text-on-surface-variant w-8 text-right">{i + 1}.</span>
                      <span className="font-label-md text-on-surface flex-1 truncate">{s.name}</span>
                      {s.attributes?.command ? (
                        <span className="font-label-sm text-label-sm text-on-surface-variant font-mono">{String(s.attributes.command)}</span>
                      ) : null}
                      <span className="font-mono font-bold text-label-md text-primary" data-testid="slow-duration">{s.durationMs.toFixed(2)}ms</span>
                    </li>
                  ))}
                </ol>
              )}
            </section>

            <section aria-label="Tool call counts" className="p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <h3 className="font-label-md text-on-surface font-bold uppercase tracking-wider mb-md">Tool call counts</h3>
              {analyzed.toolCounts.length === 0 ? (
                <p className="font-label-sm text-on-surface-variant">No events with <code>tool_name</code> fields.</p>
              ) : (
                <ul className="grid grid-cols-2 md:grid-cols-3 gap-sm" data-testid="tool-counts">
                  {analyzed.toolCounts.map(t => (
                    <li key={t.name} className="flex items-center justify-between p-sm rounded-lg bg-surface-container-low">
                      <span className="font-label-md text-on-surface truncate font-mono">{t.name}</span>
                      <span className="font-mono font-bold text-label-md text-primary ml-sm" data-testid="tool-count-value">{t.count}</span>
                    </li>
                  ))}
                </ul>
              )}
            </section>
          </>
        )}
      </div>
    </div>
  )
}

// Local Button — keep Perf page self-contained without extra imports if layout drifts.
function Button({ children, onClick, variant = 'primary' }: { children: React.ReactNode; onClick?: () => void; variant?: 'primary' | 'ghost' }) {
  const base = 'px-md py-sm rounded-xl font-label-md text-label-md cursor-pointer transition-colors'
  const cls = variant === 'primary'
    ? `${base} bg-primary text-on-primary hover:shadow-lg hover:shadow-primary/30`
    : `${base} bg-surface-container-low text-on-surface-variant hover:text-primary`
  return (
    <button type="button" onClick={onClick} className={cls}>{children}</button>
  )
}

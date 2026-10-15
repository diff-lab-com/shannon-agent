// Usage statistics page — local token / cache / cost ledger.
//
// Reads `~/.shannon/usage.jsonl` (appended on every engine Usage event in
// `commands.rs::send_message`) via the `get_usage_stats` command and
// aggregates by model / provider / day. Billing commands removed (R2 F4);
// the local usage ledger is the source of record.
//
// 2026-09 review: charts are the default surface — they answer "where did
// my tokens go this week?" at a glance. The audit (table) view sits next
// to it, reserved for precise reconciliation ("the exact cost row 12
// minutes ago").

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { save as saveDialog } from '@tauri-apps/plugin-dialog'
import { toast } from 'sonner'
import { useT } from '@/i18n'
import LoadingState from '@/components/ui/loading-state'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import StatCard from '@/components/ui/stat-card'
import { cn } from '@/lib/utils'
import { useSessions } from '@/context/SessionContext'
import type { UsageStats, UsageBucket, SessionUsageRow } from '@/types'
import CurrentSessionCostPanel from '@/components/usage/CurrentSessionCostPanel'
import UsageBudgetCard from '@/components/usage/UsageBudgetCard'
import { useUsageGovernance } from '@/hooks/useUsageGovernance'
import { BarChart, DonutChart, type BarSeriesDef } from '@/components/usage/BarChart'
import { DataTable } from '@/components/ui/data-table'
import type { ColumnDef } from '@tanstack/react-table'

const RANGES = [7, 30, 90] as const
type DisplayMode = 'overview' | 'audit'

// 设计 06: how many per-session rows the OVERVIEW preview shows before the
// 「审计模式查看全部」 hint kicks in (the audit view keeps the full table).
const SESSION_PREVIEW_ROWS = 8

function fmtTokens(locale: string, n: number): string {
  // Audit §P2-6 (round 6): explicit compactThreshold prevents zh-CN edge
  // cases where values just below the 万 boundary get rendered with a
  // confusing decimal point (e.g. 4250 → "43.5" under some Intl builds).
  // Force the abbreviated form only for ≥10k and fall back to plain
  // thousands grouping otherwise.
  if (Math.abs(n) >= 10_000) {
    return new Intl.NumberFormat(locale, {
      notation: 'compact',
      compactDisplay: 'short',
      maximumFractionDigits: 1,
    }).format(n)
  }
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 1,
  }).format(n)
}

function fmtTokensFull(locale: string, n: number): string {
  return new Intl.NumberFormat(locale).format(n)
}

function fmtCost(locale: string, n: number): string {
  return `$${new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 4,
  }).format(n)}`
}

/** Local-calendar day key (`YYYY-MM-DD`) N days from today — the same
 *  labelling the backend uses for by_day buckets (commands_usage.rs
 *  day_label buckets on local days). */
function localDayKey(offsetDays: number): string {
  const d = new Date()
  d.setDate(d.getDate() - offsetDays)
  const pad = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`
}

// M5a (设计 06:158): CSV escaping — quote a cell when it contains a comma,
// quote, or newline, doubling embedded quotes (same rule as the artifact
// table download in DocumentRenderer).
function csvCell(v: string): string {
  return /[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v
}

/**
 * M5a — build the usage export as ONE CSV with a leading `section` column
 * (by_day buckets + per-session detail for the active filter range). A
 * single table keeps it trivially parseable — no per-section headers, no
 * second file.
 */
function buildUsageCsv(byDay: UsageBucket[], bySession: SessionUsageRow[]): string {
  const header = 'section,label,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,cost_usd,requests'
  const lines: string[] = [header]
  for (const b of byDay) {
    lines.push([
      'by_day', b.label, b.input_tokens, b.output_tokens,
      b.cache_creation_tokens, b.cache_read_tokens, b.cost_usd, b.requests,
    ].map(x => csvCell(String(x))).join(','))
  }
  for (const r of bySession) {
    lines.push([
      'by_session', r.title ?? r.sessionId, r.inputTokens, r.outputTokens,
      r.cacheCreationTokens, r.cacheReadTokens, r.costUsd, r.requests,
    ].map(x => csvCell(String(x))).join(','))
  }
  return lines.join('\n')
}

/** Card wrapper for a chart — title + subtitle on top, the SVG below. */
function ChartCard({
  title,
  subtitle,
  icon,
  headerExtra,
  children,
  empty,
}: {
  title: string
  subtitle?: string
  icon: string
  /** 设计 06 (审查 R1): the metric toggle rides the card header row. */
  headerExtra?: React.ReactNode
  children: React.ReactNode
  empty?: boolean
}) {
  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 overflow-hidden">
      <div className="flex items-center gap-xs px-lg py-md border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary">{icon}</span>
        <h2 className="font-label-md font-bold text-on-surface">{title}</h2>
        {headerExtra && <div className="ml-auto shrink-0">{headerExtra}</div>}
      </div>
      <div className="p-lg">
        {empty ? (
          <p className="text-label-sm text-on-surface-variant text-center py-lg">{subtitle}</p>
        ) : (
          <>
            {subtitle && (
              <p className="font-label-sm text-label-sm text-on-surface-variant mb-md">{subtitle}</p>
            )}
            {children}
          </>
        )}
      </div>
    </div>
  )
}

/** Audit-mode table — exact numbers, no aggregation. */
function BucketTable({
  title,
  icon,
  labelTitle,
  buckets,
  locale,
  emptyTitle,
  emptyLabel,
}: {
  title: string
  icon: string
  labelTitle: string
  buckets: UsageBucket[]
  locale: string
  emptyTitle: string
  emptyLabel: string
}) {
  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 overflow-hidden">
      <div className="flex items-center gap-xs px-lg py-md border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary">{icon}</span>
        <h2 className="font-label-md font-bold text-on-surface">{title}</h2>
      </div>
      {buckets.length === 0 ? (
        <EmptyState icon="monitoring" title={emptyTitle} description={emptyLabel} />
      ) : (
        <div className="overflow-x-auto p-sm">
          <BucketDataTable labelTitle={labelTitle} buckets={buckets} locale={locale} emptyLabel={emptyLabel} />
        </div>
      )}
    </div>
  )
}

function BucketDataTable({ labelTitle, buckets, locale, emptyLabel }: {
  labelTitle: string
  buckets: UsageBucket[]
  locale: string
  emptyLabel: string
}) {
  const tB = useT()
  const columns: ColumnDef<UsageBucket, unknown>[] = [
    {
      accessorKey: 'label',
      header: labelTitle,
      enableSorting: false,
      cell: ({ getValue }) => (
        <span className="font-label-md text-on-surface truncate max-w-[220px] block">{getValue() as string}</span>
      ),
    },
    {
      id: 'tokens',
      header: tB('usage.col.tokens'),
      accessorFn: b => b.input_tokens + b.output_tokens,
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtTokensFull(locale, getValue() as number)}</span>
      ),
    },
    {
      id: 'cache',
      header: tB('usage.col.cache'),
      accessorFn: b => b.cache_creation_tokens + b.cache_read_tokens,
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtTokensFull(locale, getValue() as number)}</span>
      ),
    },
    {
      accessorKey: 'cost_usd',
      header: tB('usage.col.cost'),
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtCost(locale, getValue() as number)}</span>
      ),
    },
    { accessorKey: 'requests', header: tB('usage.col.reqs') },
  ]
  return <DataTable columns={columns} data={buckets} emptyMessage={emptyLabel} />
}

function SessionTable({ rows, locale, emptyTitle, emptyLabel }: {
  rows: SessionUsageRow[]
  locale: string
  emptyTitle: string
  emptyLabel: string
}) {
  const t = useT()
  const fmtDay = (ms: number) =>
    new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(new Date(ms))

  const columns: ColumnDef<SessionUsageRow, unknown>[] = [
    {
      accessorKey: 'title',
      header: t('usage.col.session'),
      enableSorting: false,
      cell: ({ row }) => (
        <span className="font-label-md text-on-surface truncate max-w-[220px] block">
          {row.original.title ?? `${row.original.sessionId.slice(0, 8)}…`}
        </span>
      ),
      meta: { align: 'left', pad: 'lg' },
    },
    {
      id: 'tokens',
      header: t('usage.col.tokens'),
      accessorFn: r => r.inputTokens + r.outputTokens,
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtTokensFull(locale, getValue() as number)}</span>
      ),
    },
    {
      id: 'cache',
      header: t('usage.col.cache'),
      accessorFn: r => r.cacheCreationTokens + r.cacheReadTokens,
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtTokensFull(locale, getValue() as number)}</span>
      ),
    },
    {
      accessorKey: 'costUsd',
      header: t('usage.col.cost'),
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtCost(locale, getValue() as number)}</span>
      ),
    },
    { accessorKey: 'requests', header: t('usage.col.reqs') },
    {
      id: 'lastUsed',
      header: t('usage.col.lastUsed'),
      accessorFn: r => r.lastUsedAtMs,
      cell: ({ getValue }) => (
        <span className="font-mono text-label-sm text-on-surface-variant">{fmtDay(getValue() as number)}</span>
      ),
    },
  ]

  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 overflow-hidden">
      <div className="flex items-center gap-xs px-lg py-md border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary">forum</span>
        <h2 className="font-label-md font-bold text-on-surface">{t('usage.section.bySession')}</h2>
      </div>
      {rows.length === 0 ? (
        <EmptyState icon="bar_chart" title={emptyTitle} description={emptyLabel} />
      ) : (
        <div className="overflow-x-auto p-sm">
          <DataTable columns={columns} data={rows} emptyMessage={emptyLabel} />
        </div>
      )}
    </div>
  )
}

// B6-36: the backend (commands_usage.rs) attributes unattributable spend —
// scheduled-routine runs — to model/provider buckets labelled with the
// SCHEDULED_LABEL constant ("Scheduled tasks"). Front-end mapping (per
// decision in the review): translate that literal wherever a bucket label
// renders, without touching the wire format or the backend ACL.
const SCHEDULED_LABEL = 'Scheduled tasks'

function useBucketLabel() {
  const intl = useIntl()
  return useCallback(
    (label: string) =>
      label === SCHEDULED_LABEL
        ? intl.formatMessage({ id: 'usage.scheduledTasks' })
        : label,
    [intl],
  )
}

export default function Usage() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  // B6-36: "Scheduled tasks" bucket label → translated (frontend mapping).
  const bucketLabel = useBucketLabel()
  const [days, setDays] = useState<number>(30)
  const [stats, setStats] = useState<UsageStats | null>(null)
  const [loading, setLoading] = useState(true)
  // 2026-09: two-mode page — overview (charts, the default) and audit
  // (precise tables). The mode toggle sits next to the time-range picker
  // so the action stays close to the primary surface.
  const [mode, setMode] = useState<DisplayMode>('overview')
  // 设计 06 (审查 R1 §06): the daily bar chart carries a Token / 成本 metric
  // toggle — cost_usd is already on every by_day bucket.
  const [chartMetric, setChartMetric] = useState<'tokens' | 'cost'>('tokens')
  // Per-session rows: audit mode shows them all; overview mode now surfaces
  // the top 8 as a preview (设计 06: 按会话明细回到一等公民). Loaded on days
  // change regardless of mode — cheap IPC, no polling.
  const [sessionRows, setSessionRows] = useState<SessionUsageRow[] | null>(null)
  // P2-1 — usage governance: budget % + threshold state (the sidebar's data
  // source; here it drives the banner and the budget card).
  const { governance, refresh: refreshGovernance } = useUsageGovernance()

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    api
      .getUsageStats(days)
      .then((s) => {
        if (!cancelled) setStats(s)
      })
      .catch((e) => toastError(t('usage.load.failed'), e))
      .finally(() => {
        if (!cancelled) setLoading(false)
      })
    return () => {
      cancelled = true
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [days])

  // Per-session rows load on their own effect (not gated on mode, not tied
  // to the page-level `loading` — the overview must stay responsive). The
  // audit tables and the overview's top-8 preview read the same rows.
  useEffect(() => {
    let cancelled = false
    api
      .getUsageBySession(days)
      .then(rows => { if (!cancelled) setSessionRows(rows) })
      .catch(e => toastError(t('usage.load.failed'), e))
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [days])

  // Build the per-day bar series once per metric: tokens stack input/output,
  // cost is a single series (by_day carries only the day's total cost_usd).
  const dailyBars = useMemo(() => {
    if (!stats) return []
    return stats.by_day.map(b => ({
      label: b.label.slice(5), // strip year
      series: [
        { key: 'input', value: b.input_tokens },
        { key: 'output', value: b.output_tokens },
      ],
    }))
  }, [stats])

  const dailyCostBars = useMemo(() => {
    if (!stats) return []
    return stats.by_day.map(b => ({
      label: b.label.slice(5), // strip year
      series: [{ key: 'cost', value: b.cost_usd }],
    }))
  }, [stats])

  // 设计 06 KPI: today's spend — the by_day bucket labelled with the LOCAL
  // calendar date (the backend buckets on local days, commands_usage.rs
  // day_label). A day with no ledger records has no bucket → $0.00 is the
  // honest reading, not a placeholder.
  const todayCostUsd = useMemo(() => {
    if (!stats) return null
    return stats.by_day.find(b => b.label === localDayKey(0))?.cost_usd ?? 0
  }, [stats])

  // M5b (设计 06 KPI 第二行「较昨日 ±N%」): yesterday's by_day bucket, same
  // local-day labelling. null bucket → 0 spend, but the ratio is only shown
  // when a yesterday bucket actually exists (else "—" — no fabricated %).
  const yesterdayCostUsd = useMemo(() => {
    if (!stats) return null
    return stats.by_day.find(b => b.label === localDayKey(1))?.cost_usd ?? 0
  }, [stats])

  const vsYesterdayHint = useMemo(() => {
    if (todayCostUsd == null || yesterdayCostUsd == null || yesterdayCostUsd <= 0) {
      return intl.formatMessage({ id: 'usage.stat.vsYesterday.none' })
    }
    const pct = Math.round(((todayCostUsd - yesterdayCostUsd) / yesterdayCostUsd) * 100)
    const signed = `${pct >= 0 ? '+' : ''}${pct}%`
    return intl.formatMessage({ id: 'usage.stat.vsYesterday' }, { pct: signed })
  }, [todayCostUsd, yesterdayCostUsd, intl])

  // 设计 06 KPI 四卡之四 (M4): context peak. T2 (R3 §4.1 峰值已用): once ANY
  // session reports the real used numerator (SessionInfo
  // .peak_context_used_tokens), the card shows the PEAK USED with the
  // 「（Z%）」 occupancy against that session's window in the hint — the same
  // 口径 as SessionUsageDialog's 峰值 row; sessions without the field simply
  // don't compete (no mixed 口径). When NO session has one (old indexes), the
  // card keeps its original window-peak reading with the holding session as
  // the hint; "—" when neither exists anywhere (honesty contract).
  const { sessions } = useSessions()
  const contextPeak = useMemo(() => {
    let usedBest: { tokens: number; window: number | null; title: string } | null = null
    let windowBest: { tokens: number; title: string } | null = null
    for (const s of sessions ?? []) {
      const u = s.peak_context_used_tokens
      if (u != null && u > 0 && (usedBest == null || u > usedBest.tokens)) {
        usedBest = { tokens: u, window: s.max_context_tokens ?? null, title: s.title }
      }
      const p = s.max_context_tokens
      if (p != null && p > 0 && (windowBest == null || p > windowBest.tokens)) {
        windowBest = { tokens: p, title: s.title }
      }
    }
    if (usedBest != null) return usedBest
    return windowBest != null ? { ...windowBest, window: null } : null
  }, [sessions])

  // The hint: session title, plus the （Z%） occupancy when the winning
  // session's window budget is known (the brief's 峰值 X / 预算 Y（Z%） form
  // — X rides as the card's value). Plain derivation (no hook — the page's
  // `t` is a per-render closure): intl.formatMessage carries the VALUES,
  // the page-local `t` helper is a values-less one-liner.
  const contextPeakHint = contextPeak == null
    ? t('usage.stat.contextPeak.none')
    : contextPeak.window != null && contextPeak.window > 0
      ? intl.formatMessage(
          { id: 'usage.stat.contextPeak.pct' },
          {
            title: contextPeak.title,
            pct: Math.min(100, Math.round((contextPeak.tokens / contextPeak.window) * 100)),
          },
        )
      : contextPeak.title

  // 设计 06 KPI: cache hit rate — the same frozen formula the chat context
  // card uses (cache_read / (cache_read + input)); no denominator → null,
  // rendered honestly as "no input tokens yet".
  const cacheHitRate = useMemo(() => {
    if (!stats) return null
    const denom = stats.totals.cache_read_tokens + stats.totals.input_tokens
    return denom > 0 ? (stats.totals.cache_read_tokens / denom) * 100 : null
  }, [stats])

  // Model-level donut: who got the tokens?
  const modelSegments = useMemo(() => {
    if (!stats) return []
    return stats.by_model
      .map(m => ({
        key: m.label,
        label: bucketLabel(m.label),
        value: m.input_tokens + m.output_tokens,
      }))
      .filter(s => s.value > 0)
      .sort((a, b) => b.value - a.value)
      .slice(0, 5)
  }, [stats, bucketLabel])

  // B6-36: series labels ("Input"/"Output") come from the locale, not literals.
  const tokenSeries: BarSeriesDef[] = useMemo(() => [
    { key: 'input', label: intl.formatMessage({ id: 'usage.chart.series.input' }), colorClass: 'text-primary' },
    { key: 'output', label: intl.formatMessage({ id: 'usage.chart.series.output' }), colorClass: 'text-secondary' },
  ], [intl])

  // 设计 06: cost metric — a single series, so the legend reads one entry.
  const costSeries: BarSeriesDef[] = useMemo(() => [
    { key: 'cost', label: intl.formatMessage({ id: 'usage.chart.series.cost' }), colorClass: 'text-tertiary' },
  ], [intl])

  const totalTokens = stats
    ? stats.totals.input_tokens + stats.totals.output_tokens
    : 0

  const hasData = (stats != null && stats.totals.requests > 0)

  // M5a (设计 06:158 页头「导出 CSV」): export the ACTIVE filter range's
  // by_day buckets + per-session detail as one CSV file. Destination via
  // the native save dialog, the same pattern as GeneralPane's
  // exportAllSessions (plugin-dialog save + save_text_file backend write).
  const [exportingCsv, setExportingCsv] = useState(false)
  const handleExportCsv = async () => {
    if (!stats || exportingCsv) return
    setExportingCsv(true)
    try {
      const csv = buildUsageCsv(stats.by_day, sessionRows ?? [])
      const target = await saveDialog({
        defaultPath: `shannon-usage-${localDayKey(0)}.csv`,
        filters: [{ name: 'CSV', extensions: ['csv'] }],
      })
      if (!target) return // user cancelled
      await api.saveTextFile(target, csv)
      toast.success(intl.formatMessage({ id: 'usage.export.csv.success' }), { description: target })
    } catch (e) {
      toastError(intl.formatMessage({ id: 'usage.export.csv.failed' }), e)
    } finally {
      setExportingCsv(false)
    }
  }

  return (
    <div className="p-lg max-w-medium mx-auto w-full flex-1 min-h-0 overflow-y-auto pb-16">
      {/* R3 wording audit: sr-only h1 keeps the page self-identifying
          for AT/axe without re-adopting a visible title (Tasks.tsx pattern). */}
      <h1 className="sr-only">{t('header.title.usage')}</h1>
      <p className="text-on-surface-variant font-body-md mb-lg">{t('usage.subtitle')}</p>

      {/* P2-1 — threshold banner. Derived from the live percent (not the
          once-per-month notification markers), so it stays up while the
          state persists and disappears the month the budget resets. */}
      {governance?.thresholdReached && governance.budgetUsd != null && (
        <div
          role="alert"
          data-testid="usage-budget-banner"
          className={cn(
            'flex items-start gap-sm rounded-xl border p-md mb-lg',
            governance.thresholdReached === '100'
              ? 'bg-error-container border-error/30 text-on-error-container'
              : 'bg-warning-container border-warning/30 text-on-warning-container',
          )}
        >
          <span className="material-symbols-outlined icon-md shrink-0" aria-hidden="true">
            {governance.thresholdReached === '100' ? 'error' : 'warning'}
          </span>
          <p className="font-label-md text-body-sm font-semibold">
            {governance.thresholdReached === '100'
              ? intl.formatMessage(
                  { id: 'usage.governance.banner100' },
                  {
                    spent: fmtCost(intl.locale, governance.monthCostUsd),
                    budget: fmtCost(intl.locale, governance.budgetUsd),
                  },
                )
              : intl.formatMessage(
                  { id: 'usage.governance.banner80' },
                  {
                    percent: Math.round(governance.percent ?? 0),
                    spent: fmtCost(intl.locale, governance.monthCostUsd),
                    budget: fmtCost(intl.locale, governance.budgetUsd),
                  },
                )}
          </p>
        </div>
      )}

      <div className="flex items-center gap-xs mb-lg flex-wrap">
        {/* 2026-09: two-mode toggle — Overview (charts) is the default;
            Audit (tables) sits next to it for precise reconciliation.
            B6-37: these are toggle buttons (aria-pressed), not a tablist —
            the two buttons were always independently focusable/clickable and
            never implemented the tab keyboard pattern (no roving focus, no
            aria-controls/tabpanels), so the tab roles misannounced them. */}
        <div className="flex items-center gap-xs mr-md p-xs bg-surface-container-low/60 rounded-full border border-outline-variant/20">
          <button
            type="button"
            aria-pressed={mode === 'overview'}
            onClick={() => setMode('overview')}
            className={cn(
              'px-md py-xs rounded-full font-label-md text-label-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
              mode === 'overview'
                ? 'bg-primary text-on-primary font-bold'
                : 'text-on-surface-variant hover:text-primary',
            )}
          >
            <span className="material-symbols-outlined icon-sm align-middle mr-xs" aria-hidden="true">monitoring</span>
            {t('usage.view.overview')}
          </button>
          <button
            type="button"
            aria-pressed={mode === 'audit'}
            title={t('usage.view.audit.aria')}
            onClick={() => setMode('audit')}
            className={cn(
              'px-md py-xs rounded-full font-label-md text-label-md transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
              mode === 'audit'
                ? 'bg-primary text-on-primary font-bold'
                : 'text-on-surface-variant hover:text-primary',
            )}
          >
            <span className="material-symbols-outlined icon-sm align-middle mr-xs" aria-hidden="true">table</span>
            {t('usage.view.audit')}
          </button>
        </div>
        {RANGES.map((r) => (
          <Button
            key={r}
            type="button"
            variant="outline"
            onClick={() => setDays(r)}
            aria-pressed={days === r}
            className={cn(
              'px-md py-xs rounded-full font-label-md text-label-md border transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
              days === r
                ? 'bg-primary text-on-primary border-primary font-bold'
                : 'bg-surface-container-lowest/60 text-on-surface-variant border-outline-variant/30 hover:bg-surface-container-low hover:text-primary',
            )}
          >
            {intl.formatMessage({ id: 'usage.range' }, { days: r })}
          </Button>
        ))}
        {/* M5a (设计 06:158): page-head export action — the current filter
            range's by_day + per-session detail as one CSV via the native
            save dialog. */}
        <Button
          type="button"
          variant="outline"
          disabled={loading || stats == null || exportingCsv}
          data-testid="usage-export-csv"
          onClick={() => void handleExportCsv()}
          className="px-md py-xs rounded-full font-label-md text-label-md border border-outline-variant/30 bg-surface-container-lowest/60 text-on-surface-variant hover:bg-surface-container-low hover:text-primary transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ml-auto"
        >
          <span className="material-symbols-outlined icon-sm mr-xs" aria-hidden="true">download</span>
          {t('usage.export.csv')}
        </Button>
      </div>

      {/* P2-1 — monthly-budget card: the % bar visualization + the budget
            input (configure('monthly_budget_usd')). Sits above the stats so
            the spend context is the first thing the page answers. */}
      {governance && (
        <div className="mb-lg">
          <UsageBudgetCard governance={governance} onSaved={refreshGovernance} />
        </div>
      )}

      <CurrentSessionCostPanel />
      {loading ? (
        <LoadingState size="lg" />
      ) : !hasData ? (
        <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 px-lg py-3xl text-center">
          <EmptyState
            icon="bar_chart"
            title={t('usage.empty.title')}
            description={t('usage.empty')}
          />
        </div>
      ) : mode === 'audit' ? (
        <div className="space-y-lg">
          {/* Audit view — precise numbers per bucket. Reserved for users
              who need to reconcile a specific row with their provider's
              billing dashboard. */}
          <BucketTable
            title={t('usage.section.byModel')}
            icon="smart_toy"
            labelTitle={t('usage.col.model')}
            buckets={stats!.by_model.map(b => ({ ...b, label: bucketLabel(b.label) }))}
            locale={intl.locale}
            emptyTitle={t('usage.empty.title')}
            emptyLabel={t('usage.empty')}
          />
          <BucketTable
            title={t('usage.section.byProvider')}
            icon="cloud"
            labelTitle={t('usage.col.provider')}
            buckets={stats!.by_provider.map(b => ({ ...b, label: bucketLabel(b.label) }))}
            locale={intl.locale}
            emptyTitle={t('usage.empty.title')}
            emptyLabel={t('usage.empty')}
          />
          <BucketTable
            title={t('usage.section.byDay')}
            icon="calendar_month"
            labelTitle={t('usage.col.date')}
            buckets={stats!.by_day}
            locale={intl.locale}
            emptyTitle={t('usage.empty.title')}
            emptyLabel={t('usage.empty')}
          />
          <SessionTable
            rows={sessionRows ?? []}
            locale={intl.locale}
            emptyTitle={t('usage.empty.title')}
            emptyLabel={t('usage.empty')}
          />
        </div>
      ) : (
        <div className="space-y-lg">
          {/* Totals — 设计 06 KPI row: the four ledger totals plus the two
              derived cards the design names (今日消耗 from the local-day
              bucket, 缓存命中率 from the frozen cache formula) and the
              fourth design card 上下文峰值 (M4, session-index maximum).
              2×4 grid to match the design's four-across KPI band. */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
            <StatCard
              icon="token"
              label={t('usage.stat.tokens')}
              value={fmtTokens(intl.locale, stats!.totals.input_tokens + stats!.totals.output_tokens)}
            />
            <StatCard
              icon="cached"
              label={t('usage.stat.cache')}
              value={fmtTokens(intl.locale, stats!.totals.cache_creation_tokens + stats!.totals.cache_read_tokens)}
              hint={t('usage.stat.cacheHint')}
            />
            <StatCard
              icon="payments"
              label={t('usage.stat.cost')}
              value={fmtCost(intl.locale, stats!.totals.cost_usd)}
            />
            <StatCard
              icon="chat_bubble"
              label={t('usage.stat.requests')}
              value={String(stats!.totals.requests)}
            />
            <StatCard
              icon="today"
              label={t('usage.stat.todayCost')}
              value={fmtCost(intl.locale, todayCostUsd ?? 0)}
              hint={vsYesterdayHint}
              data-testid="usage-stat-today-cost"
            />
            <StatCard
              icon="speed"
              label={t('usage.stat.cacheHitRate')}
              value={cacheHitRate != null ? `${cacheHitRate.toFixed(1)}%` : '—'}
              hint={cacheHitRate != null ? undefined : t('usage.stat.cacheHitRate.none')}
              data-testid="usage-stat-cache-hit-rate"
            />
            <StatCard
              icon="target"
              label={t('usage.stat.contextPeak')}
              value={contextPeak != null ? fmtTokens(intl.locale, contextPeak.tokens) : '—'}
              hint={contextPeakHint}
              data-testid="usage-stat-context-peak"
            />
          </div>

          {/* Charts — primary read. Hover a bar for the exact segment split.
              F-4 (ui-redesign 06): the chart title must match the active
              filter — compose the static label with the selected range
              (reusing the range-button key), e.g. "Daily tokens · 30 days"
              / "每日 token · 近 30 天". 设计 06: the card also carries the
              Token/成本 metric toggle, and the cost metric renames the card
              「每日成本」 to keep title = data. */}
          <ChartCard
            title={`${t(chartMetric === 'cost' ? 'usage.chart.byDay.costTitle' : 'usage.chart.byDay.title')} · ${intl.formatMessage({ id: 'usage.range' }, { days })}`}
            subtitle={
              chartMetric === 'tokens'
                ? t('usage.chart.byDay.subtitle')
                : dailyBars.length === 0
                  ? t('usage.empty')
                  : undefined
            }
            icon="calendar_month"
            empty={dailyBars.length === 0}
            headerExtra={
              <div
                role="group"
                aria-label={t('usage.chart.mode.aria')}
                className="flex items-center rounded-full border border-outline-variant/30 overflow-hidden"
              >
                {(['tokens', 'cost'] as const).map(metric => (
                  <button
                    key={metric}
                    type="button"
                    aria-pressed={chartMetric === metric}
                    onClick={() => setChartMetric(metric)}
                    className={cn(
                      'px-sm py-xs font-label-sm text-label-sm transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 cursor-pointer',
                      chartMetric === metric
                        ? 'bg-primary text-on-primary font-bold'
                        : 'bg-surface-container-lowest/60 text-on-surface-variant hover:text-primary',
                    )}
                  >
                    {t(`usage.chart.mode.${metric}`)}
                  </button>
                ))}
              </div>
            }
          >
            <BarChart
              data={chartMetric === 'cost' ? dailyCostBars : dailyBars}
              series={chartMetric === 'cost' ? costSeries : tokenSeries}
              formatValue={(n) => (chartMetric === 'cost' ? fmtCost(intl.locale, n) : fmtTokens(intl.locale, n))}
            />
          </ChartCard>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-md">
            <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 p-lg md:col-span-1">
              <div className="flex items-center gap-xs text-on-surface-variant mb-md">
                <span className="material-symbols-outlined icon-sm">pie_chart</span>
                <span className="font-label-sm text-label-sm uppercase tracking-wider">{t('usage.chart.byModel.title')}</span>
              </div>
              <div className="flex items-center gap-md">
                <DonutChart
                  segments={modelSegments}
                  total={totalTokens}
                  centerLabel={fmtTokens(intl.locale, totalTokens)}
                  size={140}
                />
                <ul className="flex-1 min-w-0 space-y-1">
                  {modelSegments.map((m, i) => (
                    <li key={m.key} className="flex items-center gap-xs text-label-xs">
                      <span className={cn('inline-block w-2.5 h-2.5 rounded-sm',
                        ['bg-primary', 'bg-secondary', 'bg-tertiary', 'bg-warning', 'bg-error'][i % 5])} />
                      <span className="flex-1 min-w-0 truncate text-on-surface">{m.label}</span>
                      <span className="font-mono tabular-nums text-on-surface-variant">
                        {fmtTokens(intl.locale, m.value)}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            </div>

            <div className="md:col-span-2">
              <BucketTable
                title={t('usage.chart.byProvider.title')}
                icon="cloud"
                labelTitle={t('usage.col.provider')}
                buckets={stats!.by_provider.map(b => ({ ...b, label: bucketLabel(b.label) }))}
                locale={intl.locale}
                emptyTitle={t('usage.empty.title')}
                emptyLabel={t('usage.empty')}
              />
            </div>
          </div>

          {/* 设计 06 (审查 R1 §06): 按会话明细回到总览一等公民 — the top 8
              sessions render right under the charts; the full table stays in
              the Audit view (the hint names where, honestly truncating). */}
          {sessionRows !== null && (
            <div data-testid="usage-session-preview">
              <SessionTable
                rows={sessionRows.slice(0, SESSION_PREVIEW_ROWS)}
                locale={intl.locale}
                emptyTitle={t('usage.empty.title')}
                emptyLabel={t('usage.empty')}
              />
              {sessionRows.length > SESSION_PREVIEW_ROWS && (
                <p
                  data-testid="usage-session-preview-hint"
                  className="font-label-sm text-label-sm text-on-surface-variant text-center mt-sm"
                >
                  {intl.formatMessage({ id: 'usage.session.previewHint' }, { count: SESSION_PREVIEW_ROWS })}
                </p>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

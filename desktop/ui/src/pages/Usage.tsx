// Usage statistics page — local token / cache / cost ledger.
//
// Reads `~/.shannon/usage.jsonl` (appended on every engine Usage event in
// `commands.rs::send_message`) via the `get_usage_stats` command and
// aggregates by model / provider / day. Local-only; no billing backend.
//
// 2026-09 review: charts are the default surface — they answer "where did
// my tokens go this week?" at a glance. The audit (table) view sits next
// to it, reserved for precise reconciliation ("the exact cost row 12
// minutes ago").

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { useT } from '@/i18n'
import LoadingState from '@/components/ui/loading-state'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { cn } from '@/lib/utils'
import type { UsageStats, UsageBucket, SessionUsageRow } from '@/types'
import CurrentSessionCostPanel from '@/components/usage/CurrentSessionCostPanel'
import { BarChart, DonutChart, type BarSeriesDef } from '@/components/usage/BarChart'
import { DataTable } from '@/components/ui/data-table'
import type { ColumnDef } from '@tanstack/react-table'

const RANGES = [7, 30, 90] as const
type DisplayMode = 'overview' | 'audit'

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

function StatCard({
  icon,
  label,
  value,
  hint,
}: {
  icon: string
  label: string
  value: string
  hint?: string
}) {
  return (
    // Audit §P2-4 (round 6): align label and hint on a single horizontal row
    // so cards with a hint line are the same height as cards without one.
    // This keeps the 2-col grid tidy on narrow viewports.
    <div className="bg-surface-container-low rounded-2xl p-lg border border-outline-variant/30 flex flex-col h-full">
      <div className="flex items-center justify-between gap-sm text-on-surface-variant mb-sm min-h-[20px]">
        <span className="flex items-center gap-xs min-w-0">
          <span className="material-symbols-outlined icon-sm">{icon}</span>
          <span className="font-label-sm text-label-sm uppercase tracking-wider truncate">{label}</span>
        </span>
        {hint && (
          <span
            className="font-label-xs text-label-xs text-on-surface-variant/70 truncate max-w-[60%] text-right"
            title={hint}
          >
            {hint}
          </span>
        )}
      </div>
      <div className="font-mono font-headline-md text-[26px] font-bold text-on-surface leading-tight tabular-nums">
        {value}
      </div>
    </div>
  )
}

/** Card wrapper for a chart — title + subtitle on top, the SVG below. */
function ChartCard({
  title,
  subtitle,
  icon,
  children,
  empty,
}: {
  title: string
  subtitle?: string
  icon: string
  children: React.ReactNode
  empty?: boolean
}) {
  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 overflow-hidden">
      <div className="flex items-center gap-xs px-lg py-md border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary">{icon}</span>
        <h2 className="font-label-md font-bold text-on-surface">{title}</h2>
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
    { accessorKey: 'requests', header: 'Reqs' },
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

const TOKEN_SERIES: BarSeriesDef[] = [
  { key: 'input', label: 'Input', colorClass: 'text-primary' },
  { key: 'output', label: 'Output', colorClass: 'text-secondary' },
]

export default function Usage() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [days, setDays] = useState<number>(30)
  const [stats, setStats] = useState<UsageStats | null>(null)
  const [loading, setLoading] = useState(true)
  // 2026-09: two-mode page — overview (charts, the default) and audit
  // (precise tables). The mode toggle sits next to the time-range picker
  // so the action stays close to the primary surface.
  const [mode, setMode] = useState<DisplayMode>('overview')
  const [sessionRows, setSessionRows] = useState<SessionUsageRow[] | null>(null)

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

  // 2026-09: in Audit mode we additionally pull per-session rows so the
  // per-conversation breakdown is available for precise reconciliation.
  // Loaded on demand to keep the default Overview mode responsive.
  useEffect(() => {
    if (mode !== 'audit') return
    let cancelled = false
    setLoading(true)
    api
      .getUsageBySession(days)
      .then(rows => { if (!cancelled) setSessionRows(rows) })
      .catch(e => toastError(t('usage.load.failed'), e))
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [mode, days])

  // Build the per-day bar series (input + output stacked) once.
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

  // Model-level donut: who got the tokens?
  const modelSegments = useMemo(() => {
    if (!stats) return []
    return stats.by_model
      .map(m => ({
        key: m.label,
        label: m.label,
        value: m.input_tokens + m.output_tokens,
      }))
      .filter(s => s.value > 0)
      .sort((a, b) => b.value - a.value)
      .slice(0, 5)
  }, [stats])

  const totalTokens = stats
    ? stats.totals.input_tokens + stats.totals.output_tokens
    : 0

  const hasData = (stats != null && stats.totals.requests > 0)

  return (
    <div className="p-lg max-w-6xl mx-auto">
      <p className="text-on-surface-variant font-body-md mb-lg">{t('usage.subtitle')}</p>

      <div className="flex items-center gap-xs mb-lg flex-wrap">
        {/* 2026-09: two-mode toggle — Overview (charts) is the default;
            Audit (tables) sits next to it for precise reconciliation. */}
        <div
          role="tablist"
          aria-label={t('usage.title')}
          className="flex items-center gap-xs mr-md p-xs bg-surface-container-low/60 rounded-full border border-outline-variant/20"
        >
          <button
            type="button"
            role="tab"
            aria-selected={mode === 'overview'}
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
            role="tab"
            aria-selected={mode === 'audit'}
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
      </div>

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
            buckets={stats!.by_model}
            locale={intl.locale}
            emptyTitle={t('usage.empty.title')}
            emptyLabel={t('usage.empty')}
          />
          <BucketTable
            title={t('usage.section.byProvider')}
            icon="cloud"
            labelTitle={t('usage.col.provider')}
            buckets={stats!.by_provider}
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
          {/* Totals */}
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
          </div>

          {/* Charts — primary read. Hover a bar for the exact segment split. */}
          <ChartCard
            title={t('usage.chart.byDay.title')}
            subtitle={t('usage.chart.byDay.subtitle')}
            icon="calendar_month"
            empty={dailyBars.length === 0}
          >
            <BarChart
              data={dailyBars}
              series={TOKEN_SERIES}
              formatValue={(n) => fmtTokens(intl.locale, n)}
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
                buckets={stats!.by_provider}
                locale={intl.locale}
                emptyTitle={t('usage.empty.title')}
                emptyLabel={t('usage.empty')}
              />
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

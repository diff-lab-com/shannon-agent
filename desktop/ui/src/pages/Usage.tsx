// Usage statistics page — local token / cache / cost ledger.
//
// Reads `~/.shannon/usage.jsonl` (appended on every engine Usage event in
// `commands.rs::send_message`) via the `get_usage_stats` command and
// aggregates by model / provider / day. Local-only; no billing backend.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'
import { cn } from '@/lib/utils'
import type { UsageStats, UsageBucket } from '@/types'

const RANGES = [7, 30, 90] as const

function fmtTokens(locale: string, n: number): string {
  return new Intl.NumberFormat(locale, {
    notation: 'compact',
    maximumFractionDigits: 1,
  }).format(n)
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
    <div className="bg-surface-container-low rounded-2xl p-lg border border-outline-variant/30">
      <div className="flex items-center gap-xs text-on-surface-variant mb-sm">
        <span className="material-symbols-outlined icon-sm">{icon}</span>
        <span className="font-label-sm text-label-sm uppercase tracking-wider">{label}</span>
      </div>
      <div className="font-headline-md text-[26px] font-bold text-on-surface leading-tight">
        {value}
      </div>
      {hint && (
        <div className="font-label-sm text-label-sm text-outline-variant mt-xs">{hint}</div>
      )}
    </div>
  )
}

function BucketTable({
  title,
  icon,
  labelTitle,
  buckets,
  locale,
  emptyLabel,
}: {
  title: string
  icon: string
  labelTitle: string
  buckets: UsageBucket[]
  locale: string
  emptyLabel: string
}) {
  return (
    <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 overflow-hidden">
      <div className="flex items-center gap-xs px-lg py-md border-b border-outline-variant/20">
        <span className="material-symbols-outlined icon-sm text-primary">{icon}</span>
        <h2 className="font-label-md font-bold text-on-surface">{title}</h2>
      </div>
      {buckets.length === 0 ? (
        <div className="px-lg py-lg text-center text-outline-variant font-label-md">
          {emptyLabel}
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-left">
            <thead className="text-outline-variant">
              <tr className="border-b border-outline-variant/20">
                <th className="px-lg py-xs font-label-sm font-medium">{labelTitle}</th>
                <th className="px-md py-xs font-label-sm font-medium text-right">Tokens</th>
                <th className="px-md py-xs font-label-sm font-medium text-right">Cache</th>
                <th className="px-md py-xs font-label-sm font-medium text-right">Cost</th>
                <th className="px-lg py-xs font-label-sm font-medium text-right">Reqs</th>
              </tr>
            </thead>
            <tbody>
              {buckets.map((b) => (
                <tr
                  key={b.label}
                  className="border-b border-outline-variant/10 last:border-0 hover:bg-surface-container/40"
                >
                  <td className="px-lg py-sm font-label-md text-on-surface truncate max-w-[220px]">
                    {b.label}
                  </td>
                  <td className="px-md py-sm text-right font-mono text-label-sm text-on-surface-variant">
                    {fmtTokens(locale, b.input_tokens + b.output_tokens)}
                  </td>
                  <td className="px-md py-sm text-right font-mono text-label-sm text-on-surface-variant">
                    {fmtTokens(locale, b.cache_creation_tokens + b.cache_read_tokens)}
                  </td>
                  <td className="px-md py-sm text-right font-mono text-label-sm text-on-surface-variant">
                    {fmtCost(locale, b.cost_usd)}
                  </td>
                  <td className="px-lg py-sm text-right font-mono text-label-sm text-on-surface-variant">
                    {b.requests}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  )
}

export default function Usage() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [days, setDays] = useState<number>(30)
  const [stats, setStats] = useState<UsageStats | null>(null)
  const [loading, setLoading] = useState(true)

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

  const hasData = !!stats && stats.totals.requests > 0

  return (
    <div className="p-lg max-w-6xl mx-auto">
      <div className="mb-xl">
        <h1 className="font-headline-lg text-[28px] font-bold text-on-surface flex items-center gap-sm">
          <span className="material-symbols-outlined">monitoring</span>
          {t('usage.title')}
        </h1>
        <p className="text-on-surface-variant font-body-md mt-xs">{t('usage.subtitle')}</p>
      </div>

      <div className="flex items-center gap-xs mb-lg">
        {RANGES.map((r) => (
          <button
            key={r}
            type="button"
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
          </button>
        ))}
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-3xl text-on-surface-variant">
          <span className="material-symbols-outlined animate-spin">progress_activity</span>
        </div>
      ) : !hasData ? (
        <div className="bg-surface-container-low rounded-2xl border border-outline-variant/30 px-lg py-3xl text-center">
          <span className="material-symbols-outlined text-[40px] text-outline-variant">bar_chart</span>
          <p className="font-body-md text-on-surface-variant mt-md">{t('usage.empty')}</p>
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

          {/* Breakdowns */}
          <BucketTable
            title={t('usage.section.byModel')}
            icon="smart_toy"
            labelTitle={t('usage.col.model')}
            buckets={stats!.by_model}
            locale={intl.locale}
            emptyLabel={t('usage.empty')}
          />
          <BucketTable
            title={t('usage.section.byProvider')}
            icon="cloud"
            labelTitle={t('usage.col.provider')}
            buckets={stats!.by_provider}
            locale={intl.locale}
            emptyLabel={t('usage.empty')}
          />
          <BucketTable
            title={t('usage.section.byDay')}
            icon="calendar_month"
            labelTitle={t('usage.col.date')}
            buckets={stats!.by_day}
            locale={intl.locale}
            emptyLabel={t('usage.empty')}
          />
        </div>
      )}
    </div>
  )
}

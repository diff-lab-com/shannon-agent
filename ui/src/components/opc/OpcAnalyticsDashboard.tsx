// OpcAnalyticsDashboard — Phase D P4 OPC analytics panel.
//
// Surfaces 7-day task activity (created/completed per day), completion rate,
// status/priority breakdowns, and per-assignee workload. All data comes from
// the `get_opc_metrics` Tauri command which aggregates over `.claude/tasks/`.
// File mtime is the time-series proxy — task JSON has no created_at field.

import { useEffect, useState, useCallback } from 'react'
import { useIntl } from 'react-intl'
import * as api from '@/lib/tauri-api'
import type { OpcMetrics } from '@/types'

const STATUS_TONES: Record<string, string> = {
  completed: 'bg-tertiary/15 text-tertiary border-tertiary/40',
  done: 'bg-tertiary/15 text-tertiary border-tertiary/40',
  in_progress: 'bg-primary/15 text-primary border-primary/40',
  running: 'bg-primary/15 text-primary border-primary/40',
  pending: 'bg-secondary/15 text-secondary border-secondary/40',
  todo: 'bg-outline/15 text-on-surface-variant border-outline/40',
  deprecated: 'bg-error/15 text-error border-error/40',
}

function toneFor(status: string): string {
  return STATUS_TONES[status.toLowerCase()] ?? 'bg-outline/15 text-on-surface-variant border-outline/40'
}

export default function OpcAnalyticsDashboard() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [metrics, setMetrics] = useState<OpcMetrics | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const m = await api.getOpcMetrics()
      setMetrics(m)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  if (loading && !metrics) {
    return (
      <div className="bg-surface-container-lowest rounded-2xl p-lg border border-outline-variant/30 shadow-sm">
        <p className="text-body-sm text-on-surface-variant text-center py-md">{t('opc.analytics.loading')}</p>
      </div>
    )
  }

  if (error) {
    return (
      <div className="bg-surface-container-lowest rounded-2xl p-lg border border-outline-variant/30 shadow-sm">
        <div className="font-label-sm text-error flex items-center gap-sm">
          <span className="material-symbols-outlined text-[14px]">error</span>
          {error}
        </div>
        <button
          type="button"
          onClick={refresh}
          className="mt-sm font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer"
        >
          {t('opc.analytics.retry')}
        </button>
      </div>
    )
  }

  if (!metrics) return null

  const maxDaily = Math.max(1, ...metrics.daily.map(d => Math.max(d.created, d.completed)))

  return (
    <section
      aria-label={t('opc.analytics.aria')}
      className="bg-surface-container-lowest rounded-2xl p-lg border border-outline-variant/30 shadow-sm flex flex-col gap-lg"
    >
      <header className="flex items-center justify-between">
        <div className="flex items-center gap-sm">
          <span className="material-symbols-outlined text-[20px] text-primary">monitoring</span>
          <h3 className="font-headline-md text-[16px] font-bold text-on-surface">{t('opc.analytics.title')}</h3>
        </div>
        <button
          type="button"
          onClick={refresh}
          disabled={loading}
          className="font-label-sm text-primary hover:bg-primary/10 rounded px-sm py-xs cursor-pointer flex items-center gap-1 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-40"
          aria-label={t('opc.analytics.refreshAria')}
        >
          <span className="material-symbols-outlined text-[14px]">{loading ? 'hourglass_top' : 'refresh'}</span>
          {t('opc.analytics.refresh')}
        </button>
      </header>

      <div className="grid grid-cols-2 md:grid-cols-4 gap-md">
        <StatCard label={t('opc.analytics.statTotal')} value={metrics.total} icon="inventory_2" />
        <StatCard label={t('opc.analytics.statCompletion')} value={`${metrics.completion_rate.toFixed(0)}%`} icon="trending_up" />
        <StatCard
          label={t('opc.analytics.statInProgress')}
          value={metrics.by_status.filter(s => /in_progress|running|pending/i.test(s.status)).reduce((n, s) => n + s.count, 0)}
          icon="work"
        />
        <StatCard
          label={t('opc.analytics.statDone')}
          value={metrics.by_status.filter(s => /completed|done/i.test(s.status)).reduce((n, s) => n + s.count, 0)}
          icon="task_alt"
        />
      </div>

      <div>
        <h4 className="font-label-md text-on-surface mb-sm flex items-center gap-xs">
          <span className="material-symbols-outlined text-[14px] text-on-surface-variant">bar_chart</span>
          {t('opc.analytics.dailyActivity')}
        </h4>
        {metrics.daily.length === 0 ? (
          <p className="font-label-sm text-on-surface-variant italic">{t('opc.analytics.noActivity')}</p>
        ) : (
          <div className="flex items-end justify-between gap-sm h-32" role="img" aria-label={t('opc.analytics.dailyChartAria')}>
            {metrics.daily.map(d => {
              const createdH = (d.created / maxDaily) * 100
              const completedH = (d.completed / maxDaily) * 100
              const shortDay = d.date.slice(5) // MM-DD
              return (
                <div key={d.date} className="flex-1 flex flex-col items-center gap-xs">
                  <div className="w-full flex items-end justify-center gap-0.5 h-24">
                    <div
                      className="w-3 bg-primary/70 rounded-t hover:bg-primary transition-colors"
                      style={{ height: `${Math.max(createdH, d.created > 0 ? 6 : 0)}%` }}
                      title={intl.formatMessage({ id: 'opc.analytics.created' }, { count: d.created })}
                    />
                    <div
                      className="w-3 bg-tertiary/70 rounded-t hover:bg-tertiary transition-colors"
                      style={{ height: `${Math.max(completedH, d.completed > 0 ? 6 : 0)}%` }}
                      title={intl.formatMessage({ id: 'opc.analytics.completedTitle' }, { count: d.completed })}
                    />
                  </div>
                  <span className="font-label-sm text-[10px] text-on-surface-variant">{shortDay}</span>
                </div>
              )
            })}
          </div>
        )}
        <div className="flex items-center gap-md mt-sm font-label-sm text-[11px] text-on-surface-variant">
          <span className="flex items-center gap-1"><span className="w-2 h-2 bg-primary/70 inline-block rounded-sm" /> {t('opc.analytics.createdLegend')}</span>
          <span className="flex items-center gap-1"><span className="w-2 h-2 bg-tertiary/70 inline-block rounded-sm" /> {t('opc.analytics.completedLegend')}</span>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-md">
        <div>
          <h4 className="font-label-md text-on-surface mb-sm flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px] text-on-surface-variant">bubble_chart</span>
            By status
          </h4>
          {metrics.by_status.length === 0 ? (
            <p className="font-label-sm text-on-surface-variant italic">No tasks.</p>
          ) : (
            <ul className="flex flex-col gap-xs">
              {metrics.by_status.map(s => (
                <li key={s.status} className="flex items-center gap-sm">
                  <span className={`inline-flex items-center px-xs py-1 rounded-full border font-label-sm text-[10px] font-bold uppercase tracking-wide w-32 justify-center ${toneFor(s.status)}`}>
                    {s.status}
                  </span>
                  <div className="flex-1 bg-surface-container-low rounded-full h-2 overflow-hidden">
                    <div
                      className="h-full bg-primary/60"
                      style={{ width: `${metrics.total === 0 ? 0 : (s.count / metrics.total) * 100}%` }}
                    />
                  </div>
                  <span className="font-label-sm text-on-surface-variant w-8 text-right">{s.count}</span>
                </li>
              ))}
            </ul>
          )}
        </div>

        <div>
          <h4 className="font-label-md text-on-surface mb-sm flex items-center gap-xs">
            <span className="material-symbols-outlined text-[14px] text-on-surface-variant">priority_high</span>
            By priority
          </h4>
          {metrics.by_priority.length === 0 ? (
            <p className="font-label-sm text-on-surface-variant italic">No priority set on any task.</p>
          ) : (
            <ul className="flex flex-col gap-xs">
              {metrics.by_priority.map(p => (
                <li key={p.priority} className="flex items-center gap-sm">
                  <span className="font-label-sm text-on-surface-variant w-24 capitalize">{p.priority}</span>
                  <div className="flex-1 bg-surface-container-low rounded-full h-2 overflow-hidden">
                    <div
                      className="h-full bg-tertiary/60"
                      style={{ width: `${metrics.total === 0 ? 0 : (p.count / metrics.total) * 100}%` }}
                    />
                  </div>
                  <span className="font-label-sm text-on-surface-variant w-8 text-right">{p.count}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>

      <div>
        <h4 className="font-label-md text-on-surface mb-sm flex items-center gap-xs">
          <span className="material-symbols-outlined text-[14px] text-on-surface-variant">group</span>
          Workload by assignee
        </h4>
        {metrics.by_assignee.length === 0 ? (
          <p className="font-label-sm text-on-surface-variant italic">No assignees yet.</p>
        ) : (
          <ul className="flex flex-col gap-xs">
            {metrics.by_assignee.map(a => (
              <li key={a.assignee} className="flex items-center gap-sm">
                <span className="material-symbols-outlined text-[14px] text-on-surface-variant">person</span>
                <span className="font-label-md text-on-surface flex-1 truncate">{a.assignee}</span>
                <span className="font-label-sm text-[11px] text-on-surface-variant">
                  <strong className="text-primary">{a.in_progress}</strong> in progress ·{' '}
                  <strong className="text-tertiary">{a.done}</strong> done ·{' '}
                  <strong className="text-on-surface">{a.total}</strong> total
                </span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </section>
  )
}

function StatCard({ label, value, icon }: { label: string; value: string | number; icon: string }) {
  return (
    <div className="bg-surface-container-low rounded-xl p-md flex items-center gap-sm border border-outline-variant/20">
      <span className="material-symbols-outlined text-[20px] text-primary">{icon}</span>
      <div className="min-w-0">
        <div className="font-headline-md text-[20px] font-bold text-on-surface leading-none">{value}</div>
        <div className="font-label-sm text-[11px] text-on-surface-variant mt-1">{label}</div>
      </div>
    </div>
  )
}

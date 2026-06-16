// HistoryView — surfaces scheduled-task execution history.
//
// Calls listTaskExecutions (Tauri) and renders a timeline of past runs with
// status, duration, cost, and token usage. Clicking a row calls
// getExecutionDetail and shows the run's prompt + error inline.
//
// P2.2 deliverable from OPC-SCHEDULED-GAP-ANALYSIS.md §2.6 Phase 2.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import * as api from '@/lib/tauri-api'
import type { TaskExecution, TaskExecutionDetail } from '@/types'
import { statusBadge, formatUnixDateTime } from './shared'

function durationLabel(start: number, end?: number): string {
  if (!end) return '—'
  const secs = Math.max(0, Math.round(end - start))
  if (secs < 60) return `${secs}s`
  const mins = Math.floor(secs / 60)
  const rem = secs % 60
  return rem === 0 ? `${mins}m` : `${mins}m ${rem}s`
}

function StatusPill({ status }: { status: string }) {
  const badge = statusBadge(status)
  return (
    <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full border text-[11px] font-bold ${badge.bg}`}>
      <span className="material-symbols-outlined text-[12px]">{badge.icon}</span>
      {badge.label}
    </span>
  )
}

export default function HistoryView({ taskId, limit = 50 }: { taskId?: string; limit?: number }) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [rows, setRows] = useState<TaskExecution[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<TaskExecutionDetail | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)

  useEffect(() => {
    let cancelled = false
    setLoading(true); setError(null)
    api.listTaskExecutions(taskId, limit)
      .then(r => { if (!cancelled) setRows(r) })
      .catch(e => { if (!cancelled) setError(e instanceof Error ? e.message : t('tasks.historyView.loadFailed')) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [taskId, limit])

  const openDetail = async (id: string) => {
    if (expandedId === id) { setExpandedId(null); setDetail(null); return }
    setExpandedId(id); setDetail(null); setDetailLoading(true)
    try {
      setDetail(await api.getExecutionDetail(id))
    } catch (e) {
      console.warn('Failed to load execution detail:', e)
    } finally {
      setDetailLoading(false)
    }
  }

  // Normalize: TaskExecution uses run_id; we expose `id` for keys/lookups
  const rowId = (r: TaskExecution) => r.run_id

  if (loading) {
    return (
      <div className="space-y-md">
        {Array.from({ length: 4 }).map((_, i) => <CardSkeleton key={i} />)}
      </div>
    )
  }
  if (error) {
    return (
      <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-error/10 border border-error/20 text-error font-label-md">
        <span className="material-symbols-outlined text-[18px]">error</span>
        {error}
      </div>
    )
  }
  if (rows.length === 0) {
    return (
      <div className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl p-xl">
        <EmptyState icon="history" title={t('tasks.historyView.emptyTitle')} description={t('tasks.historyView.emptyDesc')} />
      </div>
    )
  }

  return (
    <div className="space-y-sm">
      <div className="flex items-center justify-between mb-md">
        <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">{t('tasks.historyView.title')}</h3>
        <span className="font-label-sm text-[11px] text-on-surface-variant">{intl.formatMessage({ id: 'tasks.historyView.runsCount' }, { count: rows.length })}</span>
      </div>
      <div className="space-y-sm">
        {rows.map(row => {
          const id = rowId(row)
          const isExpanded = expandedId === id
          return (
            <div key={id} className="bg-surface-container-lowest/80 border border-outline-variant/20 rounded-xl overflow-hidden">
              <button
                type="button"
                className="w-full text-left px-md py-sm flex items-center gap-md hover:bg-surface-container-low/40 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                onClick={() => openDetail(id)}
                aria-expanded={isExpanded}
              >
                <StatusPill status={row.status} />
                <span className="font-label-md text-[13px] font-bold truncate flex-1">{row.task_name}</span>
                <span className="font-label-sm text-[11px] text-on-surface-variant whitespace-nowrap">{formatUnixDateTime(row.started_at)}</span>
                <span className="font-label-sm text-[11px] text-on-surface-variant whitespace-nowrap">{durationLabel(row.started_at, row.finished_at)}</span>
                {row.cost_usd != null ? (
                  <span className="font-label-sm text-[11px] text-on-surface-variant whitespace-nowrap">${row.cost_usd.toFixed(4)}</span>
                ) : <span className="font-label-sm text-[11px] text-on-surface-variant/60">—</span>}
                {row.token_usage != null ? (
                  <span className="font-label-sm text-[11px] text-on-surface-variant whitespace-nowrap">{row.token_usage.toLocaleString()} tok</span>
                ) : <span className="font-label-sm text-[11px] text-on-surface-variant/60">—</span>}
                <span className={`material-symbols-outlined text-[16px] text-on-surface-variant transition-transform ${isExpanded ? 'rotate-180' : ''}`}>expand_more</span>
              </button>
              {isExpanded ? (
                <div className="px-md pb-md border-t border-outline-variant/10">
                  {detailLoading ? (
                    <p className="font-label-sm text-on-surface-variant py-md">{t('tasks.historyView.loadingDetails')}</p>
                  ) : detail ? (
                    <div className="pt-md space-y-sm">
                      {detail.prompt ? (
                        <div>
                          <div className="font-label-sm text-[11px] text-on-surface-variant uppercase tracking-wider mb-xs">{t('tasks.historyView.prompt')}</div>
                          <pre className="font-mono text-[12px] bg-surface-container-low/60 rounded p-sm whitespace-pre-wrap break-words">{detail.prompt}</pre>
                        </div>
                      ) : null}
                      {detail.cron_expr ? (
                        <div className="font-label-sm text-[12px] text-on-surface-variant">
                          <strong>{t('tasks.historyView.cron')}:</strong> <code className="font-mono">{detail.cron_expr}</code>
                        </div>
                      ) : null}
                      {detail.next_fire_at ? (
                        <div className="font-label-sm text-[12px] text-on-surface-variant">
                          <strong>{t('tasks.historyView.nextFire')}:</strong> {formatUnixDateTime(detail.next_fire_at)}
                        </div>
                      ) : null}
                      {row.error_message ? (
                        <div>
                          <div className="font-label-sm text-[11px] text-error uppercase tracking-wider mb-xs">{t('tasks.historyView.error')}</div>
                          <pre className="font-mono text-[12px] bg-error/5 text-error border border-error/20 rounded p-sm whitespace-pre-wrap break-words">{row.error_message}</pre>
                        </div>
                      ) : null}
                    </div>
                  ) : (
                    <p className="font-label-sm text-on-surface-variant py-md">{t('tasks.historyView.noDetail')}</p>
                  )}
                </div>
              ) : null}
            </div>
          )
        })}
      </div>
    </div>
  )
}

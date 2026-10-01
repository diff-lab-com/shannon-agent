// OPCRunsTable — P2-8 cross-agent run table ("运行" view on /opc).
//
// Lists the newest runs across ALL routines/agents (list_agent_runs: the
// SQLite routine_runs table, usage-ledger and inbox joins backend-side) with
// the Notion-Insights-style columns: 时间 / 任务·agent / 状态 / 成本 / token /
// 模型. Row click expands the run's detail (prompt / cron / error — the same
// getExecutionDetail payload the Tasks History view shows); the per-row
// session button jumps straight into the conversation that produced the run.
//
// Interaction pattern deliberately mirrors HistoryView (row-toggle + secondary
// action button + stale-response guard) so the two run tables read as one
// component family.

import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useT } from '@/i18n'
import EmptyState from '@/components/ui/empty-state'
import ErrorState from '@/components/ui/error-state'
import { RowSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { useSessions } from '@/context/SessionContext'
import * as api from '@/lib/tauri-api'
import type { AgentRunRow, TaskExecutionDetail } from '@/types'
import { statusBadge, formatUnixDateTime } from '@/components/tasks/shared'

function durationLabel(start: number, end?: number): string {
  if (!end) return '—'
  const secs = Math.max(0, Math.round(end - start))
  if (secs < 60) return `${secs}s`
  const mins = Math.floor(secs / 60)
  const rem = secs % 60
  return rem === 0 ? `${mins}m` : `${mins}m ${rem}s`
}

function StatusPill({ status }: { status: string }) {
  const intl = useIntl()
  // routine_runs serializes RunStatus as succeeded/cancelled/archived, which
  // the Tasks badge table doesn't know — succeeded reads as completed,
  // everything unknown falls through to the interpolated default.
  const mapped = status === 'succeeded' ? 'completed' : status
  const badge = statusBadge(mapped)
  return (
    <span className={cn('inline-flex items-center gap-xs px-sm py-0.5 rounded-full border text-label-xs font-bold', badge.bg)}>
      <span className="material-symbols-outlined icon-xs">{badge.icon}</span>
      {intl.formatMessage({ id: badge.labelId }, badge.values)}
    </span>
  )
}

export default function OPCRunsTable({ limit = 100 }: { limit?: number }) {
  const t = useT()
  const navigate = useNavigate()
  const sessionCtx = useSessions()
  const [rows, setRows] = useState<AgentRunRow[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<TaskExecutionDetail | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)

  useEffect(() => {
    let cancelled = false
    setLoading(true)
    setError(null)
    api.listAgentRuns(limit)
      .then(r => { if (!cancelled) setRows(Array.isArray(r) ? r : []) })
      .catch(e => { if (!cancelled) setError(e instanceof Error ? e.message : t('opc.runs.loadFailed')) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [limit, t])

  const refresh = () => {
    setLoading(true)
    setError(null)
    api.listAgentRuns(limit)
      .then(setRows)
      .catch(e => setError(e instanceof Error ? e.message : t('opc.runs.loadFailed')))
      .finally(() => setLoading(false))
  }

  // Same monotonic-request guard as HistoryView: a slow detail response for
  // row A must never land under row B.
  const detailRequestRef = useRef(0)
  const openDetail = async (id: string) => {
    if (expandedId === id) { setExpandedId(null); setDetail(null); return }
    const request = ++detailRequestRef.current
    setExpandedId(id)
    setDetail(null)
    setDetailLoading(true)
    try {
      const d = await api.getExecutionDetail(id)
      if (request === detailRequestRef.current) setDetail(d)
    } catch (e) {
      console.warn('Failed to load execution detail:', e)
    } finally {
      if (request === detailRequestRef.current) setDetailLoading(false)
    }
  }

  const openSession = (row: AgentRunRow) => {
    if (!row.session_id) return
    // Optional call: test harnesses (and any stale provider) may hand back a
    // partial session context.
    void sessionCtx?.switchSession?.(row.session_id)
    navigate('/chat')
  }

  if (loading) {
    return (
      <div className="space-y-md">
        <RowSkeleton count={4} />
      </div>
    )
  }
  if (error) {
    return (
      <div className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl p-xl">
        <ErrorState
          title={t('opc.runs.loadFailed')}
          description={error}
          action={{ label: t('tasks.historyView.retry'), onClick: refresh }}
        />
      </div>
    )
  }
  if (rows.length === 0) {
    return (
      <div className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl p-xl">
        <EmptyState
          icon="forum"
          title={t('opc.runs.emptyTitle')}
          description={t('opc.runs.emptyDesc')}
        />
      </div>
    )
  }

  const th = 'px-md py-xs text-left font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider font-bold'
  const td = 'px-md py-sm text-label-sm'

  return (
    <section aria-label={t('opc.runs.title')} className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl overflow-hidden">
      <div className="overflow-x-auto">
        <table className="w-full min-w-[720px] border-collapse">
          <caption className="sr-only">{t('opc.runs.title')}</caption>
          <thead>
            <tr className="border-b border-outline-variant/20">
              <th scope="col" className={th}>{t('opc.runs.col.time')}</th>
              <th scope="col" className={th}>{t('opc.runs.col.task')}</th>
              <th scope="col" className={th}>{t('opc.runs.col.status')}</th>
              <th scope="col" className={cn(th, 'text-right')}>{t('opc.runs.col.cost')}</th>
              <th scope="col" className={cn(th, 'text-right')}>{t('opc.runs.col.tokens')}</th>
              <th scope="col" className={th}>{t('opc.runs.col.model')}</th>
              <th scope="col" className={cn(th, 'text-right')}>{t('opc.runs.col.session')}</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(row => {
              const isExpanded = expandedId === row.run_id
              return (
                <FragmentRow
                  key={row.run_id}
                  row={row}
                  isExpanded={isExpanded}
                  onToggle={() => void openDetail(row.run_id)}
                  onOpenSession={() => openSession(row)}
                  td={td}
                  detail={detail}
                  detailLoading={detailLoading}
                />
              )
            })}
          </tbody>
        </table>
      </div>
    </section>
  )
}

function FragmentRow({
  row,
  isExpanded,
  onToggle,
  onOpenSession,
  td,
  detail,
  detailLoading,
}: {
  row: AgentRunRow
  isExpanded: boolean
  onToggle: () => void
  onOpenSession: () => void
  td: string
  detail: TaskExecutionDetail | null
  detailLoading: boolean
}) {
  const t = useT()
  return (
    <>
      <tr
        className={cn(
          'border-b border-outline-variant/10 cursor-pointer transition-colors hover:bg-surface-container-low/50',
          isExpanded && 'bg-surface-container-low/60',
        )}
        onClick={onToggle}
        aria-expanded={isExpanded}
      >
        <td className={cn(td, 'whitespace-nowrap text-on-surface-variant tabular-nums')}>
          {formatUnixDateTime(row.started_at)}
        </td>
        <td className={cn(td, 'font-bold text-on-surface max-w-[280px] truncate')} title={row.task_name}>
          {row.task_name}
        </td>
        <td className={td}>
          <StatusPill status={row.status} />
        </td>
        <td className={cn(td, 'text-right tabular-nums whitespace-nowrap')}>
          {row.cost_usd != null ? `$${row.cost_usd.toFixed(4)}` : <span className="text-on-surface-variant/60">—</span>}
        </td>
        <td className={cn(td, 'text-right tabular-nums whitespace-nowrap')}>
          {row.token_usage != null
            ? `${row.token_usage.toLocaleString()} tok`
            : <span className="text-on-surface-variant/60">—</span>}
        </td>
        <td className={cn(td, 'font-mono text-label-xs text-on-surface-variant max-w-[180px] truncate')} title={row.model ?? undefined}>
          {row.model ?? <span className="text-on-surface-variant/60">—</span>}
        </td>
        <td className={cn(td, 'text-right whitespace-nowrap')}>
          <span className="font-label-xs text-on-surface-variant/70 mr-sm">
            {durationLabel(row.started_at, row.finished_at)}
          </span>
          {row.session_id ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="px-sm py-xs text-label-xs gap-xs text-primary hover:bg-primary/10"
              aria-label={`${t('opc.runs.openSession')}: ${row.task_name}`}
              title={t('opc.runs.openSession')}
              onClick={e => { e.stopPropagation(); onOpenSession() }}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">chat</span>
              {t('opc.runs.openSession')}
            </Button>
          ) : (
            <span className="inline-flex items-center gap-xs text-label-xs text-on-surface-variant/60">
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">{isExpanded ? 'expand_less' : 'expand_more'}</span>
              {t('opc.runs.detail')}
            </span>
          )}
        </td>
      </tr>
      {isExpanded && (
        <tr className="border-b border-outline-variant/10">
          <td colSpan={7} className="px-md pb-md pt-sm">
            {detailLoading ? (
              <p className="font-label-sm text-on-surface-variant py-sm">{t('tasks.historyView.loadingDetails')}</p>
            ) : detail ? (
              <div className="space-y-sm">
                {detail.prompt ? (
                  <div>
                    <div className="font-label-sm text-label-xs text-on-surface-variant uppercase tracking-wider mb-xs">{t('tasks.historyView.prompt')}</div>
                    <pre className="font-mono text-label-sm bg-surface-container-low/60 rounded-sm p-sm whitespace-pre-wrap break-words">{detail.prompt}</pre>
                  </div>
                ) : null}
                {detail.cron_expr ? (
                  <p className="font-label-sm text-label-sm text-on-surface-variant">
                    <strong>{t('tasks.historyView.cron')}:</strong> <code className="font-mono">{detail.cron_expr}</code>
                  </p>
                ) : null}
                {row.error_message ? (
                  <div>
                    <div className="font-label-sm text-label-xs text-error uppercase tracking-wider mb-xs">{t('tasks.historyView.error')}</div>
                    <pre className="font-mono text-label-sm bg-error-container text-on-error-container border border-error/20 rounded-sm p-sm whitespace-pre-wrap break-words">{row.error_message}</pre>
                  </div>
                ) : null}
              </div>
            ) : (
              <p className="font-label-sm text-on-surface-variant py-sm">{t('tasks.historyView.noDetail')}</p>
            )}
          </td>
        </tr>
      )}
    </>
  )
}

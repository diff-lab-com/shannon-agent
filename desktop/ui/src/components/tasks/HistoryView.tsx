// HistoryView — surfaces scheduled-task execution history.
//
// Calls listTaskExecutions (Tauri) and renders a timeline of past runs with
// status, duration, cost, and token usage. Clicking a row calls
// getExecutionDetail and shows the run's prompt + error inline.
//
// P2.2 deliverable from OPC-SCHEDULED-GAP-ANALYSIS.md §2.6 Phase 2.

import { useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import { useNavigate } from 'react-router-dom'
import { useT } from '@/i18n'
import EmptyState from '@/components/ui/empty-state'
import ErrorState from '@/components/ui/error-state'
import { RowSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { projectKeyOf } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type { InboxItem, TaskExecution, TaskExecutionDetail } from '@/types'
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
  const intl = useIntl()
  const badge = statusBadge(status)
  return (
    <span className={cn('inline-flex items-center gap-1 px-2 py-0.5 rounded-full border text-[11px] font-bold', badge.bg)}>
      <span className="material-symbols-outlined icon-xs">{badge.icon}</span>
      {intl.formatMessage({ id: badge.labelId }, badge.values)}
    </span>
  )
}

export default function HistoryView({
  taskId,
  limit = 50,
  onGoToActive,
  projectDir,
  routineDirById,
}: {
  taskId?: string
  limit?: number
  onGoToActive?: () => void
  /** P-U3: active project key (/tasks?project=). Rows whose routine's
   *  working_dir maps elsewhere are hidden; standalone usage (no props)
   *  keeps the full timeline. */
  projectDir?: string | null
  /** P-U3: task_id → routine working_dir join (the execution rows only
   *  know the routine id; Tasks supplies it from useScheduledTasks). */
  routineDirById?: Record<string, string | null | undefined>
}) {
  const intl = useIntl()
  const t = useT()
  const navigate = useNavigate()
  const [rows, setRows] = useState<TaskExecution[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [expandedId, setExpandedId] = useState<string | null>(null)
  const [detail, setDetail] = useState<TaskExecutionDetail | null>(null)
  const [detailLoading, setDetailLoading] = useState(false)
  // IA T2 (互链闭环): inbox items let a history run link back to its result
  // card on /triage. Best-effort read — matching failures degrade to a plain
  // /triage jump, never to an error state.
  const [inboxItems, setInboxItems] = useState<InboxItem[]>([])

  useEffect(() => {
    let cancelled = false
    api.listInboxItems()
      .then(items => { if (!cancelled) setInboxItems(items ?? []) })
      .catch(() => { if (!cancelled) setInboxItems([]) })
    return () => { cancelled = true }
  }, [])

  useEffect(() => {
    let cancelled = false
    setLoading(true); setError(null)
    api.listTaskExecutions(taskId, limit)
      .then(r => { if (!cancelled) setRows(r) })
      .catch(e => { if (!cancelled) setError(e instanceof Error ? e.message : t('tasks.historyView.loadFailed')) })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [taskId, limit, t])

  const refresh = () => {
    setLoading(true); setError(null)
    api.listTaskExecutions(taskId, limit)
      .then(setRows)
      .catch(e => setError(e instanceof Error ? e.message : t('tasks.historyView.loadFailed')))
      .finally(() => setLoading(false))
  }

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

  // P-U3: project scope — a run stays only when its routine's working_dir
  // maps to the active project key. Runs of routines without a working_dir
  // (or of unknown routines) drop out while the filter is active.
  const visibleRows = useMemo(() => {
    if (!projectDir) return rows
    return rows.filter(r => projectKeyOf({ working_dir: routineDirById?.[r.task_id] }) === projectDir)
  }, [rows, projectDir, routineDirById])

  // IA T2: jump to the run's inbox card on /triage (highlighted via router
  // state); with no matching item, fall back to the plain inbox list.
  const openInInbox = (row: TaskExecution) => {
    const match = inboxItems.find(item => item.sourceId === row.task_id)
    navigate('/triage', match ? { state: { highlightInboxId: match.id } } : undefined)
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
          title={t('tasks.historyView.loadFailed')}
          description={error}
          action={{ label: t('tasks.historyView.retry'), onClick: refresh }}
        />
      </div>
    )
  }
  if (visibleRows.length === 0) {
    return (
      <div className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl p-xl">
        <EmptyState
          icon="history"
          title={t('tasks.historyView.emptyTitle')}
          description={t('tasks.historyView.emptyDesc')}
          action={onGoToActive ? { label: t('tasks.historyView.cta'), onClick: onGoToActive } : undefined}
        />
      </div>
    )
  }

  return (
    <div className="space-y-sm">
      <div className="flex items-center justify-between mb-md">
        <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">{t('tasks.historyView.title')}</h3>
        <span className="font-label-sm text-[11px] text-on-surface-variant">{intl.formatMessage({ id: 'tasks.historyView.runsCount' }, { count: visibleRows.length })}</span>
      </div>
      <div className="space-y-sm">
        {visibleRows.map(row => {
          const id = rowId(row)
          const isExpanded = expandedId === id
          return (
            <div key={id} className="bg-surface-container-lowest/80 border border-outline-variant/20 rounded-xl overflow-hidden">
              <div className="flex items-stretch">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="flex-1 min-w-0 justify-start px-md hover:bg-surface-container-low/40"
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
                  <span className={cn('material-symbols-outlined icon-sm text-on-surface-variant transition-transform', isExpanded ? 'rotate-180' : '')}>expand_more</span>
                </Button>
                {/* IA T2: secondary action — the run's result card lives on
                    /triage (highlighted when source_id matches an item). */}
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  aria-label={t('tasks.historyView.viewInInbox.aria')}
                  title={t('tasks.historyView.viewInInbox.aria')}
                  className="px-sm rounded-none border-l border-outline-variant/10 hover:bg-surface-container-low/40 text-on-surface-variant hover:text-primary whitespace-nowrap"
                  onClick={() => openInInbox(row)}
                >
                  <span className="material-symbols-outlined text-[16px]" aria-hidden="true">inbox</span>
                  <span className="hidden md:inline font-label-sm text-[11px]">{t('tasks.historyView.viewInInbox')}</span>
                </Button>
              </div>
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

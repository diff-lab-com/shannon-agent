// Triage page — the automation inbox (P0-3). Backed by the SQLite inbox
// Tauri commands (`list_inbox_items` / `update_inbox_item_status` /
// `get_inbox_stats` / `rerun_inbox_item` / `continue_inbox_item_session`).
// Items are produced by scheduled routine runs (`routine`/`scheduled_task`),
// goal events, the external trigger endpoint, and parallel batch-run
// completions (`batch`) — see `inbox_commands.rs` / `batch_commands.rs`.
//
// Layout: header with stats summary → filter bar (status chips + source
// chips + sort) → bulk-action bar when items selected → list of InboxCard
// cards with checkbox + Mark Read / Archive / Continue-in-session / Rerun
// actions, expandable error details, empty states.
//
// The page keeps the previous triage page's visual language (glass-panel
// cards, chip filters, keyboard j/k navigation, bulk selection bar).

import { useState, useMemo, useCallback, useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { useInboxItems, useInboxStats } from '@/hooks/inbox'
import { useSessions } from '@/context/SessionContext'
import type { InboxItem, InboxItemStatus, InboxSource } from '@/types'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

type StatusFilter = InboxItemStatus | undefined
type SourceFilter = InboxSource | undefined
type SortOrder = 'newest' | 'oldest'

/// Sources whose backing routine can be re-executed via `rerun_inbox_item`.
/// `goal`/`trigger` items have no runnable task behind them (the backend
/// rejects reruns for them), so the UI disables the action instead.
const RERUNNABLE_SOURCES: readonly InboxSource[] = ['routine', 'scheduled_task']

function canRerun(item: InboxItem): boolean {
  return RERUNNABLE_SOURCES.includes(item.source) && item.status !== 'archived'
}

function sourceMeta(source: InboxSource): { icon: string; color: string; labelKey: string } {
  switch (source) {
    case 'routine':
      return { icon: 'event_repeat', color: 'text-primary', labelKey: 'inbox.source.routine' }
    case 'scheduled_task':
      return { icon: 'task_alt', color: 'text-primary', labelKey: 'inbox.source.scheduled_task' }
    case 'goal':
      return { icon: 'flag', color: 'text-secondary', labelKey: 'inbox.source.goal' }
    case 'trigger':
      return { icon: 'bolt', color: 'text-error', labelKey: 'inbox.source.trigger' }
    case 'batch':
      // Parallel batch run's aggregate completion record (T3) — same visual
      // language as the batch runner panel (call_split icon, tertiary color).
      return { icon: 'call_split', color: 'text-tertiary', labelKey: 'inbox.source.batch' }
    default:
      return { icon: 'notifications', color: 'text-on-surface-variant', labelKey: 'inbox.source.trigger' }
  }
}

const STATUS_OPTIONS: readonly (InboxItemStatus | 'all')[] = ['all', 'pending', 'read', 'archived']
const SOURCE_OPTIONS: readonly (InboxSource | 'all')[] = ['all', 'routine', 'scheduled_task', 'goal', 'trigger', 'batch']

function InboxCard({ item, selected, focused, onToggleSelected, onMarkRead, onArchive, onContinue, onRerun }: {
  item: InboxItem
  selected: boolean
  focused?: boolean
  onToggleSelected: (id: number) => void
  onMarkRead: (id: number) => void
  onArchive: (id: number) => void
  onContinue: (item: InboxItem) => void
  onRerun: (item: InboxItem) => void
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const [showError, setShowError] = useState(false)
  const meta = sourceMeta(item.source)
  const rerunnable = canRerun(item)
  const isPending = item.status === 'pending'

  return (
    <div role="listitem" data-focused={focused ? 'true' : undefined} className={cn('glass-panel border rounded-xl p-md shadow-sm hover:shadow-md transition-all group bg-surface-container-lowest/80', isPending ? 'border-primary/20' : 'border-outline-variant/10', focused ? 'ring-2 ring-primary' : selected ? 'ring-2 ring-primary/40' : '')}>
      <div className="flex items-start gap-sm">
        <label className="flex items-center pt-xs cursor-pointer shrink-0" aria-label={t('inbox.select.aria', { id: item.id })}>
          <input
            type="checkbox"
            checked={selected}
            onChange={() => onToggleSelected(item.id)}
            className="w-4 h-4 accent-primary cursor-pointer"
          />
        </label>
        <div className="flex items-start gap-md flex-1 min-w-0">
          <div className={cn("w-10 h-10 rounded-xl bg-surface-container-low flex items-center justify-center", meta.color, "shrink-0")}>
            <span className="material-symbols-outlined icon-lg" aria-hidden="true">{meta.icon}</span>
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-sm mb-xs flex-wrap">
              <span className={cn("font-label-sm text-[11px] font-bold uppercase tracking-wider", meta.color)}>{t(meta.labelKey)}</span>
              {isPending && <span className="w-2 h-2 rounded-full bg-primary shrink-0" title={t('inbox.pending.title')} />}
              {item.status === 'archived' && <span className="font-label-sm text-[11px] text-on-surface-variant">{t('inbox.status.archived')}</span>}
            </div>
            <p className={cn("text-body-sm font-medium mb-xs break-words", isPending ? "text-on-surface" : "text-on-surface-variant")}>{item.title}</p>
            {item.summary && (
              <p className="text-body-sm text-on-surface-variant mb-xs break-words whitespace-pre-wrap">{item.summary}</p>
            )}
            {item.error && (
              <div className="mb-xs">
                <Button
                  variant="ghost"
                  aria-expanded={showError}
                  aria-controls={`inbox-error-${item.id}`}
                  onClick={() => setShowError(v => !v)}
                  className="px-xs py-0.5 rounded-lg text-label-sm text-error hover:bg-error/10 cursor-pointer inline-flex items-center gap-xs"
                >
                  <span className="material-symbols-outlined text-[14px]" aria-hidden="true">warning</span>
                  {t('inbox.error.label')}
                  <span className="material-symbols-outlined text-[14px]" aria-hidden="true">{showError ? 'expand_less' : 'expand_more'}</span>
                </Button>
                {showError && (
                  <pre id={`inbox-error-${item.id}`} className="mt-xs px-sm py-xs rounded-lg bg-surface-container-low border border-outline-variant/30 text-label-sm text-error whitespace-pre-wrap break-words max-h-48 overflow-y-auto">{item.error}</pre>
                )}
              </div>
            )}
            <div className="flex items-center gap-md flex-wrap">
              <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px]" aria-hidden="true">schedule</span>
                {new Date(item.createdAtMs).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
              </span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-sm shrink-0">
          {item.sessionId && (
            /* Primary action (audit §3.4): the Codex review-queue loop is
               "result → resume the original thread", so resume gets a
               labelled primary button instead of a ghost icon. */
            <Button
              aria-label={t('inbox.continue.aria')}
              className="px-sm py-1.5 rounded-lg bg-primary text-on-primary text-label-sm font-medium cursor-pointer hover:bg-primary/90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary inline-flex items-center gap-xs"
              onClick={() => onContinue(item)}
            >
              <span className="material-symbols-outlined text-[16px]">forum</span>
              {t('inbox.action.resume')}
            </Button>
          )}
          <Button
            aria-label={t('inbox.rerun.aria')}
            variant="ghost"
            disabled={!rerunnable}
            className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => onRerun(item)}
            title={rerunnable ? t('inbox.rerun.title') : t('inbox.rerun.disabled.title')}
          >
            <span className="material-symbols-outlined text-[18px]">replay</span>
          </Button>
          {isPending && (
            <Button
              aria-label={t('inbox.markRead.aria', { id: item.id })}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onMarkRead(item.id)}
              title={t('inbox.markRead.title')}
            >
              <span className="material-symbols-outlined text-[18px]">check</span>
            </Button>
          )}
          {item.status !== 'archived' && (
            <Button
              aria-label={t('inbox.archive.aria', { id: item.id })}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onArchive(item.id)}
              title={t('inbox.archive.title')}
            >
              <span className="material-symbols-outlined text-[18px]">archive</span>
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

export default function Triage() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const navigate = useNavigate()
  const { switchSession } = useSessions()

  const [statusFilter, setStatusFilter] = useState<StatusFilter>(undefined)
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>(undefined)
  const [sortOrder, setSortOrder] = useState<SortOrder>('newest')
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const [bulkRunning, setBulkRunning] = useState(false)
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const { stats } = useInboxStats()
  const { items, loading, markRead, archive, rerun, getSessionId, setFilter, refresh } = useInboxItems()

  // Server-side filtering: status + source chips map 1:1 onto the
  // `list_inbox_items` filter args; sort stays client-side. Both hooks
  // subscribe to `inbox-updated` themselves, so the header chips and the
  // list stay in sync after every status write / new run item.
  useEffect(() => {
    setFilter({ status: statusFilter, source: sourceFilter })
  }, [statusFilter, sourceFilter, setFilter])

  const visibleItems = useMemo(() => {
    const sorted = [...items].sort((a, b) => {
      const diff = a.createdAtMs - b.createdAtMs
      return sortOrder === 'newest' ? -diff : diff
    })
    return sorted
  }, [items, sortOrder])

  // Drop selections that no longer match the visible list.
  const effectiveSelected = useMemo(() => {
    const visibleIds = new Set(visibleItems.map(i => i.id))
    const next = new Set<number>()
    for (const id of selectedIds) if (visibleIds.has(id)) next.add(id)
    return next
  }, [selectedIds, visibleItems])

  const allSelected = visibleItems.length > 0 && effectiveSelected.size === visibleItems.length

  const toggleSelected = useCallback((id: number) => {
    setSelectedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const toggleSelectAll = useCallback(() => {
    if (allSelected) setSelectedIds(new Set())
    else setSelectedIds(new Set(visibleItems.map(i => i.id)))
  }, [allSelected, visibleItems])

  const clearSelection = useCallback(() => setSelectedIds(new Set()), [])

  const bulkSetStatus = useCallback(async (status: InboxItemStatus, toastKey: string, toastKeyPlural: string) => {
    setBulkRunning(true)
    const results = await Promise.allSettled(
      Array.from(effectiveSelected).map(id => api.updateInboxItemStatus(id, status)),
    )
    await refresh()
    setBulkRunning(false)
    const ok = results.filter(r => r.status === 'fulfilled').length
    if (ok > 0) {
      toast.success(intl.formatMessage({ id: ok === 1 ? toastKey : toastKeyPlural }, { count: ok }))
    }
    clearSelection()
  }, [effectiveSelected, refresh, clearSelection, intl])

  const bulkMarkRead = useCallback(
    () => bulkSetStatus('read', 'inbox.bulk.toast.markRead', 'inbox.bulk.toast.markRead.plural'),
    [bulkSetStatus],
  )
  const bulkArchive = useCallback(
    () => bulkSetStatus('archived', 'inbox.bulk.toast.archived', 'inbox.bulk.toast.archived.plural'),
    [bulkSetStatus],
  )

  const handleContinue = useCallback(async (item: InboxItem) => {
    const sessionId = await getSessionId(item.id)
    if (!sessionId) return
    await switchSession(sessionId)
    navigate('/chat')
  }, [getSessionId, switchSession, navigate])

  const handleRerun = useCallback(async (item: InboxItem) => {
    await rerun(item.id)
  }, [rerun])

  // Keyboard navigation over the inbox list (list must be focused first).
  // j/ArrowDown = next, k/ArrowUp = previous, Enter = mark read, a = archive.
  // Ignored when the keystroke originates from a form field so the filter
  // chips keep working normally.
  useEffect(() => {
    if (focusedIndex == null) return
    if (visibleItems.length === 0) { setFocusedIndex(null); return }
    if (focusedIndex >= visibleItems.length) setFocusedIndex(visibleItems.length - 1)
  }, [visibleItems, focusedIndex])

  useEffect(() => {
    if (focusedIndex == null) return
    const el = listRef.current?.querySelector('[data-focused="true"]') as HTMLElement | null
    el?.scrollIntoView({ block: 'nearest' })
  }, [focusedIndex])

  const handleListKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const tag = (e.target as HTMLElement).tagName
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return
    const max = visibleItems.length
    if (max === 0) return
    const cur = focusedIndex ?? -1
    if (e.key === 'j' || e.key === 'ArrowDown') {
      e.preventDefault()
      setFocusedIndex(Math.min(cur + 1, max - 1))
    } else if (e.key === 'k' || e.key === 'ArrowUp') {
      e.preventDefault()
      setFocusedIndex(Math.max(cur - 1, 0))
    } else if (e.key === 'Enter') {
      if (cur >= 0) {
        const it = visibleItems[cur]
        if (it && it.status === 'pending') { e.preventDefault(); markRead(it.id) }
      }
    } else if (e.key === 'a') {
      if (cur >= 0) {
        const it = visibleItems[cur]
        if (it && it.status !== 'archived') { e.preventDefault(); archive(it.id) }
      }
    }
  }

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* Header */}
        <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">{t('inbox.title')}</h2>
            <p className="text-on-surface-variant mt-xs">{t('inbox.subtitle')}</p>
          </div>
          <div className="flex items-center gap-md">
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">mark_email_unread</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'inbox.stats.pending' }, { count: stats.pending })}</span>
            </div>
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant" aria-hidden="true">today</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'inbox.stats.today' }, { count: stats.today })}</span>
            </div>
          </div>
        </div>

        {/* Filter bar: status */}
        <div className="flex items-center gap-sm mb-sm flex-wrap">
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">{t('inbox.status.label')}</span>
          {STATUS_OPTIONS.map(opt => {
            const active = statusFilter === (opt === 'all' ? undefined : opt)
            return (
              <Button
                key={opt}
                variant="ghost"
                onClick={() => setStatusFilter(opt === 'all' ? undefined : opt)}
                aria-pressed={active}
                className={cn("px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer", active ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10')}
              >
                {t(opt === 'all' ? 'inbox.filter.all' : `inbox.status.${opt}`)}
              </Button>
            )
          })}
        </div>

        {/* Filter bar: source + sort */}
        <div className="flex items-center gap-sm mb-lg flex-wrap">
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">{t('inbox.source.label')}</span>
          {SOURCE_OPTIONS.map(opt => {
            const active = sourceFilter === (opt === 'all' ? undefined : opt)
            const label = opt === 'all' ? t('inbox.filter.all') : t(sourceMeta(opt).labelKey)
            return (
              <Button
                key={opt}
                variant="ghost"
                onClick={() => setSourceFilter(opt === 'all' ? undefined : opt)}
                aria-pressed={active}
                className={cn("px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer", active ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10')}
              >
                {label}
              </Button>
            )
          })}
          <Button
            variant="ghost"
            onClick={() => setSortOrder(sortOrder === 'newest' ? 'oldest' : 'newest')}
            aria-label={t('inbox.sort.aria')}
            className="ml-auto px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10"
          >
            <span className="material-symbols-outlined text-[14px] mr-xs align-middle" aria-hidden="true">
              {sortOrder === 'newest' ? 'arrow_downward' : 'arrow_upward'}
            </span>
            {t(sortOrder === 'newest' ? 'inbox.sort.newest' : 'inbox.sort.oldest')}
          </Button>
        </div>

        {/* Bulk-action bar (visible when items are selected) */}
        {effectiveSelected.size > 0 ? (
          <div
            role="region"
            aria-label={t('inbox.bulk.title')}
            className="sticky top-0 z-raised mb-md flex items-center gap-md px-md py-sm rounded-xl bg-primary/10 border border-primary/30 backdrop-blur-md"
          >
            <span className="font-label-md text-primary font-bold">
              {intl.formatMessage({ id: 'inbox.bulk.selected' }, { count: effectiveSelected.size })}
            </span>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkMarkRead}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined icon-sm mr-xs align-middle" aria-hidden="true">done_all</span>
              {t('inbox.bulk.markRead')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkArchive}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined icon-sm mr-xs align-middle" aria-hidden="true">archive</span>
              {t('inbox.bulk.archive')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={clearSelection}
              className="ml-auto px-sm py-xs rounded-lg text-label-md text-on-surface-variant hover:text-primary cursor-pointer"
            >
              {t('inbox.bulk.clear')}
            </Button>
          </div>
        ) : null}

        {/* List */}
        {loading ? (
          <div className="space-y-md">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : items.length === 0 ? (
          <EmptyState
            icon="inbox"
            title={t('inbox.empty.title')}
            description={t('inbox.empty.description')}
            action={{ label: t('inbox.empty.cta'), onClick: () => void refresh() }}
          />
        ) : (
          <>
            {/* Select-all row */}
            <div className="flex items-center gap-sm mb-sm">
              <label className="flex items-center gap-xs cursor-pointer">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleSelectAll}
                  aria-label={t(allSelected ? 'inbox.select.deselectAll' : 'inbox.select.selectAll')}
                  className="w-4 h-4 accent-primary cursor-pointer"
                />
                <span className="font-label-sm text-on-surface-variant">
                  {t(allSelected ? 'inbox.select.deselectAll' : 'inbox.select.selectAll')}
                </span>
              </label>
              <span className="font-label-sm text-on-surface-variant ml-auto">
                {intl.formatMessage({ id: 'inbox.select.shown' }, { visible: visibleItems.length, total: items.length })}
              </span>
            </div>
            <div
              ref={listRef}
              role="list"
              tabIndex={0}
              aria-label={t('inbox.list.aria')}
              onKeyDown={handleListKey}
              className="space-y-md outline-none"
            >
              {visibleItems.map((item, i) => (
                <InboxCard
                  key={item.id}
                  item={item}
                  selected={effectiveSelected.has(item.id)}
                  focused={focusedIndex === i}
                  onToggleSelected={toggleSelected}
                  onMarkRead={markRead}
                  onArchive={archive}
                  onContinue={item => void handleContinue(item)}
                  onRerun={item => void handleRerun(item)}
                />
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  )
}

// Triage page — surfaces items needing user attention (failed runs, budget
// alerts, needs-review). Backed by the Sprint 2 triage Tauri commands.
//
// W11 (PM roadmap): added multi-select bulk operations, read-state filter,
// and sort-by-created_at toggle. Filter chips for kind + archived remain.
//
// Layout: header with stats summary → filter bar (kind chips + read filter
// + sort + archived toggle) → bulk-action bar when items selected → list of
// TriageItem cards with checkbox + Mark Read / Archive actions, empty state.

import { useState, useMemo, useCallback } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { useTriageItems, useTriageStats } from '@/hooks/scheduled-tasks'
import type { TriageItem } from '@/types'
import { formatUnixDateTime } from '@/components/tasks/shared'

type ReadFilter = 'all' | 'unread' | 'read'
type SortOrder = 'newest' | 'oldest'

// Severity heuristic: map a triage `kind` to a color/icon for visual sorting.
function kindMeta(kind: string): { icon: string; color: string; label: string } {
  switch (kind) {
    case 'failed_run':
      return { icon: 'error', color: 'text-error', label: 'Failed Run' }
    case 'budget_exceeded':
      return { icon: 'payments', color: 'text-error', label: 'Budget Exceeded' }
    case 'needs_review':
      return { icon: 'rate_review', color: 'text-primary', label: 'Needs Review' }
    case 'timeout':
      return { icon: 'schedule', color: 'text-secondary', label: 'Timeout' }
    default:
      return { icon: 'notifications', color: 'text-on-surface-variant', label: kind }
  }
}

function TriageCard({ item, selected, onToggleSelected, onMarkRead, onArchive, onDelete }: {
  item: TriageItem
  selected: boolean
  onToggleSelected: (id: string) => void
  onMarkRead: (id: string) => void
  onArchive: (id: string) => void
  onDelete: (id: string) => void
}) {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const meta = kindMeta(item.kind)
  return (
    <div className={`glass-panel border rounded-xl p-md shadow-sm hover:shadow-md transition-all group bg-surface-container-lowest/80 ${item.read ? 'border-outline-variant/10 opacity-70' : 'border-primary/20'} ${selected ? 'ring-2 ring-primary/40' : ''}`}>
      <div className="flex items-start gap-sm">
        <label className="flex items-center pt-xs cursor-pointer shrink-0" aria-label={t('triage.select.aria', { id: item.id })}>
          <input
            type="checkbox"
            checked={selected}
            onChange={() => onToggleSelected(item.id)}
            className="w-4 h-4 accent-primary cursor-pointer"
          />
        </label>
        <div className="flex items-start gap-md flex-1 min-w-0">
          <div className={`w-10 h-10 rounded-xl bg-surface-container-low flex items-center justify-center ${meta.color} shrink-0`}>
            <span className="material-symbols-outlined text-[24px]">{meta.icon}</span>
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-sm mb-xs">
              <span className={`font-label-sm text-[11px] font-bold uppercase tracking-wider ${meta.color}`}>{meta.label}</span>
              {!item.read && <span className="w-2 h-2 rounded-full bg-primary shrink-0" title={t('triage.unread.title')} />}
              {item.archived && <span className="font-label-sm text-[11px] text-on-surface-variant">{t('triage.archived.label')}</span>}
            </div>
            <p className="text-body-sm text-on-surface mb-xs break-words">{item.message}</p>
            <div className="flex items-center gap-md flex-wrap">
              {item.task_name && (
                <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                  <span className="material-symbols-outlined text-[14px]">task_alt</span>
                  {item.task_name}
                </span>
              )}
              <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px]">schedule</span>
                {formatUnixDateTime(item.created_at)}
              </span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-sm shrink-0">
          {!item.read && (
            <Button
              aria-label={t('triage.markRead.aria')}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onMarkRead(item.id)}
              title={t('triage.markRead.title')}
            >
              <span className="material-symbols-outlined text-[18px]">check</span>
            </Button>
          )}
          {!item.archived && (
            <Button
              aria-label={t('triage.archive.aria')}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onArchive(item.id)}
              title={t('triage.archive.title')}
            >
              <span className="material-symbols-outlined text-[18px]">archive</span>
            </Button>
          )}
          <Button
            aria-label={t('triage.delete.aria')}
            variant="ghost"
            className="p-2 rounded-lg hover:bg-error/10 text-on-surface-variant hover:text-error cursor-pointer"
            onClick={() => onDelete(item.id)}
            title={t('triage.delete.title')}
          >
            <span className="material-symbols-outlined text-[18px]">delete</span>
          </Button>
        </div>
      </div>
    </div>
  )
}

export default function Triage() {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const [kindFilter, setKindFilter] = useState<string | undefined>(undefined)
  const [showArchived, setShowArchived] = useState(false)
  const [readFilter, setReadFilter] = useState<ReadFilter>('all')
  const [sortOrder, setSortOrder] = useState<SortOrder>('newest')
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkRunning, setBulkRunning] = useState(false)
  const [deleteTargetId, setDeleteTargetId] = useState<string | null>(null)
  const [showBulkDeleteConfirm, setShowBulkDeleteConfirm] = useState(false)
  const { stats } = useTriageStats()
  const filter = useMemo(() => ({
    kind: kindFilter,
    unarchived_only: showArchived ? undefined : true,
  }), [kindFilter, showArchived])
  const { items, loading, markRead, archive, refresh } = useTriageItems(filter)

  const availableKinds = useMemo(() => {
    return Object.entries(stats.by_kind)
      .sort((a, b) => b[1] - a[1])
      .map(([k]) => k)
  }, [stats.by_kind])

  // Client-side read filter + sort on top of the server-side kind/archived filter.
  const visibleItems = useMemo(() => {
    const filtered = items.filter(item => {
      if (readFilter === 'unread' && item.read) return false
      if (readFilter === 'read' && !item.read) return false
      return true
    })
    const sorted = [...filtered].sort((a, b) => {
      const diff = a.created_at - b.created_at
      return sortOrder === 'newest' ? -diff : diff
    })
    return sorted
  }, [items, readFilter, sortOrder])

  // Drop selections that no longer match the visible list.
  const effectiveSelected = useMemo(() => {
    const visibleIds = new Set(visibleItems.map(i => i.id))
    const next = new Set<string>()
    for (const id of selectedIds) if (visibleIds.has(id)) next.add(id)
    return next
  }, [selectedIds, visibleItems])

  const allSelected = visibleItems.length > 0 && effectiveSelected.size === visibleItems.length

  const toggleSelected = useCallback((id: string) => {
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

  const bulkMarkRead = useCallback(async () => {
    setBulkRunning(true)
    let ok = 0
    for (const id of effectiveSelected) {
      if (await markRead(id)) ok++
    }
    setBulkRunning(false)
    if (ok > 0) {
      const key = ok === 1 ? 'triage.toast.markRead' : 'triage.toast.markRead.plural'
      toast.success(intl.formatMessage({ id: key }, { count: ok }))
    }
    clearSelection()
  }, [effectiveSelected, markRead, clearSelection, intl])

  const bulkArchive = useCallback(async () => {
    setBulkRunning(true)
    let ok = 0
    for (const id of effectiveSelected) {
      if (await archive(id)) ok++
    }
    setBulkRunning(false)
    if (ok > 0) {
      const key = ok === 1 ? 'triage.toast.archived' : 'triage.toast.archived.plural'
      toast.success(intl.formatMessage({ id: key }, { count: ok }))
    }
    clearSelection()
  }, [effectiveSelected, archive, clearSelection, intl])

  // Delete in the UI maps to archive in the backend — the backend doesn't
  // expose a hard-delete endpoint, so we treat archive as soft removal and
  // surface it as "Delete" to match the spec's bulk complete/archive/delete
  // triad. A future backend hard-delete can swap in here without UI changes.
  const deleteItem = useCallback(async (id: string) => {
    if (await archive(id)) toast.success(intl.formatMessage({ id: 'triage.toast.deleted' }, { count: 1 }))
    setDeleteTargetId(null)
  }, [archive, intl])

  const bulkDelete = useCallback(async () => {
    setBulkRunning(true)
    let ok = 0
    for (const id of effectiveSelected) {
      if (await archive(id)) ok++
    }
    setBulkRunning(false)
    if (ok > 0) {
      const key = ok === 1 ? 'triage.toast.deleted' : 'triage.toast.deleted.plural'
      toast.success(intl.formatMessage({ id: key }, { count: ok }))
    }
    setShowBulkDeleteConfirm(false)
    clearSelection()
  }, [effectiveSelected, archive, clearSelection, intl])

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* Header */}
        <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">{t('triage.title')}</h2>
            <p className="text-on-surface-variant mt-xs">{t('triage.subtitle')}</p>
          </div>
          <div className="flex items-center gap-md">
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-primary">mark_email_unread</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'triage.unread' }, { count: stats.unread })}</span>
            </div>
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant">inbox</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'triage.total' }, { count: stats.total })}</span>
            </div>
          </div>
        </div>

        {/* Filter bar */}
        <div className="flex items-center gap-sm mb-md flex-wrap">
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">{t('triage.kind.label')}</span>
          <Button
            variant="ghost"
            onClick={() => setKindFilter(undefined)}
            aria-pressed={!kindFilter}
            className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${!kindFilter ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
          >
            {t('triage.filter.all')}
          </Button>
          {availableKinds.map(kind => {
            const meta = kindMeta(kind)
            return (
              <Button
                key={kind}
                variant="ghost"
                onClick={() => setKindFilter(kind)}
                aria-pressed={kindFilter === kind}
                className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${kindFilter === kind ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
              >
                {meta.label}
              </Button>
            )
          })}
        </div>

        <div className="flex items-center gap-sm mb-lg flex-wrap">
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">{t('triage.read.label')}</span>
          {(['all', 'unread', 'read'] as const).map(opt => (
            <Button
              key={opt}
              variant="ghost"
              onClick={() => setReadFilter(opt)}
              aria-pressed={readFilter === opt}
              className={`px-sm py-xs rounded-full text-label-sm capitalize transition-colors cursor-pointer ${readFilter === opt ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
            >
              {t(`triage.filter.${opt}`)}
            </Button>
          ))}
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider ml-md mr-xs">{t('triage.sort.label')}</span>
          <Button
            variant="ghost"
            onClick={() => setSortOrder(sortOrder === 'newest' ? 'oldest' : 'newest')}
            aria-label={t('triage.sort.aria')}
            className="px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10"
          >
            <span className="material-symbols-outlined text-[14px] mr-xs align-middle">
              {sortOrder === 'newest' ? 'arrow_downward' : 'arrow_upward'}
            </span>
            {t(sortOrder === 'newest' ? 'triage.sort.newest' : 'triage.sort.oldest')}
          </Button>
          <div className="ml-auto">
            <Button
              variant="ghost"
              onClick={() => setShowArchived(!showArchived)}
              aria-pressed={showArchived}
              className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${showArchived ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
            >
              <span className="material-symbols-outlined text-[14px] mr-xs align-middle">archive</span>
              {t(showArchived ? 'triage.archived.show' : 'triage.archived.hide')}
            </Button>
          </div>
        </div>

        {/* Bulk-action bar (visible when items are selected) */}
        {effectiveSelected.size > 0 ? (
          <div
            role="region"
            aria-label={t('triage.bulk.title')}
            className="sticky top-0 z-10 mb-md flex items-center gap-md px-md py-sm rounded-xl bg-primary/10 border border-primary/30 backdrop-blur-md"
          >
            <span className="font-label-md text-primary font-bold">
              {intl.formatMessage({ id: 'triage.bulk.selected' }, { count: effectiveSelected.size })}
            </span>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkMarkRead}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[16px] mr-xs align-middle">done_all</span>
              {t('triage.bulk.markRead')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkArchive}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[16px] mr-xs align-middle">archive</span>
              {t('triage.bulk.archive')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={() => setShowBulkDeleteConfirm(true)}
              className="px-sm py-xs rounded-lg text-label-md text-error hover:bg-error/10 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined text-[16px] mr-xs align-middle">delete</span>
              {t('triage.bulk.delete')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={clearSelection}
              className="ml-auto px-sm py-xs rounded-lg text-label-md text-on-surface-variant hover:text-primary cursor-pointer"
            >
              {t('triage.bulk.clear')}
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
            icon="check_circle"
            title={t('triage.empty.title')}
            description={t('triage.empty.description')}
            action={{ label: t('triage.empty.cta'), onClick: () => void refresh() }}
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
                  aria-label={t(allSelected ? 'triage.select.deselectAll' : 'triage.select.selectAll')}
                  className="w-4 h-4 accent-primary cursor-pointer"
                />
                <span className="font-label-sm text-on-surface-variant">
                  {t(allSelected ? 'triage.select.deselectAll' : 'triage.select.selectAll')}
                </span>
              </label>
              <span className="font-label-sm text-outline ml-auto">
                {intl.formatMessage({ id: 'triage.select.shown' }, { visible: visibleItems.length, total: items.length })}
              </span>
            </div>
            {visibleItems.length === 0 ? (
              <EmptyState
                icon="filter_alt_off"
                title={t('triage.noMatch.title')}
                description={t('triage.noMatch.description')}
              />
            ) : (
              <div className="space-y-md">
                {visibleItems.map(item => (
                  <TriageCard
                    key={item.id}
                    item={item}
                    selected={effectiveSelected.has(item.id)}
                    onToggleSelected={toggleSelected}
                    onMarkRead={markRead}
                    onArchive={archive}
                    onDelete={setDeleteTargetId}
                  />
                ))}
              </div>
            )}
          </>
        )}
      </div>

      {/* Single-item delete confirmation */}
      {deleteTargetId && (
        <div
          role="dialog"
          aria-label={t('triage.deleteDialog.title')}
          className="fixed inset-0 z-50 bg-black/30 backdrop-blur-sm flex items-center justify-center"
          onClick={() => setDeleteTargetId(null)}
          onKeyDown={e => { if (e.key === 'Escape') setDeleteTargetId(null) }}
        >
          <div
            className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">delete</span>
              <h3 className="font-headline-md text-on-surface">{t('triage.deleteDialog.title')}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">
              {t('triage.deleteDialog.message')}
            </p>
            <div className="flex justify-end gap-sm">
              <Button
                variant="ghost"
                className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
                onClick={() => setDeleteTargetId(null)}
              >
                {t('triage.deleteDialog.cancel')}
              </Button>
              <Button
                className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90 cursor-pointer"
                onClick={() => deleteItem(deleteTargetId)}
              >
                {t('triage.deleteDialog.confirm')}
              </Button>
            </div>
          </div>
        </div>
      )}

      {/* Bulk delete confirmation */}
      {showBulkDeleteConfirm && (
        <div
          role="dialog"
          aria-label={t('triage.bulkDeleteDialog.title', { count: effectiveSelected.size })}
          className="fixed inset-0 z-50 bg-black/30 backdrop-blur-sm flex items-center justify-center"
          onClick={() => setShowBulkDeleteConfirm(false)}
          onKeyDown={e => { if (e.key === 'Escape') setShowBulkDeleteConfirm(false) }}
        >
          <div
            className="bg-surface-container-lowest rounded-2xl p-xl shadow-xl border border-outline-variant/30 max-w-sm w-full mx-md"
            onClick={e => e.stopPropagation()}
          >
            <div className="flex items-center gap-sm mb-md">
              <span className="material-symbols-outlined text-error text-[24px]">delete</span>
              <h3 className="font-headline-md text-on-surface">{t('triage.bulkDeleteDialog.title', { count: effectiveSelected.size })}</h3>
            </div>
            <p className="text-body-md text-on-surface-variant mb-lg">
              {t('triage.bulkDeleteDialog.message')}
            </p>
            <div className="flex justify-end gap-sm">
              <Button
                variant="ghost"
                disabled={bulkRunning}
                className="px-lg py-sm rounded-xl text-on-surface-variant hover:bg-surface-container cursor-pointer"
                onClick={() => setShowBulkDeleteConfirm(false)}
              >
                {t('triage.deleteDialog.cancel')}
              </Button>
              <Button
                disabled={bulkRunning}
                className="px-lg py-sm rounded-xl bg-error text-on-error hover:bg-error/90 cursor-pointer disabled:opacity-50"
                onClick={bulkDelete}
              >
                {bulkRunning ? t('triage.bulkDeleteDialog.deleting') : t('triage.deleteDialog.confirm')}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

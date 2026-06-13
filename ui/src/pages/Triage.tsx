// Triage page — surfaces items needing user attention (failed runs, budget
// alerts, needs-review). Backed by the Sprint 2 triage Tauri commands.
//
// Layout: header with stats summary, filter bar (kind chips + archived toggle),
// list of TriageItem cards with Mark Read / Archive actions, empty state.
//
// MD3 tokens. Matches Tasks.tsx visual rhythm.

import { useState, useMemo } from 'react'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { useTriageItems, useTriageStats } from '@/hooks/scheduled-tasks'
import type { TriageItem } from '@/types'
import { formatUnixDateTime } from '@/components/tasks/shared'

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

function TriageCard({ item, onMarkRead, onArchive }: {
  item: TriageItem
  onMarkRead: (id: string) => void
  onArchive: (id: string) => void
}) {
  const meta = kindMeta(item.kind)
  return (
    <div className={`glass-panel border rounded-xl p-md shadow-sm hover:shadow-md transition-all group bg-surface-container-lowest/80 ${item.read ? 'border-outline-variant/10 opacity-70' : 'border-primary/20'}`}>
      <div className="flex items-start justify-between gap-md">
        <div className="flex items-start gap-md flex-1 min-w-0">
          <div className={`w-10 h-10 rounded-xl bg-surface-container-low flex items-center justify-center ${meta.color} shrink-0`}>
            <span className="material-symbols-outlined text-[24px]">{meta.icon}</span>
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-sm mb-xs">
              <span className={`font-label-sm text-[11px] font-bold uppercase tracking-wider ${meta.color}`}>{meta.label}</span>
              {!item.read && <span className="w-2 h-2 rounded-full bg-primary shrink-0" title="Unread" />}
              {item.archived && <span className="font-label-sm text-[11px] text-on-surface-variant">Archived</span>}
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
              aria-label="Mark read"
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onMarkRead(item.id)}
              title="Mark as read"
            >
              <span className="material-symbols-outlined text-[18px]">check</span>
            </Button>
          )}
          {!item.archived && (
            <Button
              aria-label="Archive item"
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onArchive(item.id)}
              title="Archive"
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
  const [kindFilter, setKindFilter] = useState<string | undefined>(undefined)
  const [showArchived, setShowArchived] = useState(false)
  const { stats } = useTriageStats()
  const filter = useMemo(() => ({
    kind: kindFilter,
    unarchived_only: showArchived ? undefined : true,
  }), [kindFilter, showArchived])
  const { items, loading, markRead, archive } = useTriageItems(filter)

  const availableKinds = useMemo(() => {
    return Object.entries(stats.by_kind)
      .sort((a, b) => b[1] - a[1])
      .map(([k]) => k)
  }, [stats.by_kind])

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* Header */}
        <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">Triage</h2>
            <p className="text-on-surface-variant mt-xs">Items needing your attention.</p>
          </div>
          <div className="flex items-center gap-md">
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-primary">mark_email_unread</span>
              <span className="font-label-md text-on-surface">{stats.unread} unread</span>
            </div>
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant">inbox</span>
              <span className="font-label-md text-on-surface">{stats.total} total</span>
            </div>
          </div>
        </div>

        {/* Filter bar */}
        <div className="flex items-center gap-sm mb-lg flex-wrap">
          <Button
            variant="ghost"
            onClick={() => setKindFilter(undefined)}
            className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${!kindFilter ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
          >
            All Kinds
          </Button>
          {availableKinds.map(kind => {
            const meta = kindMeta(kind)
            return (
              <Button
                key={kind}
                variant="ghost"
                onClick={() => setKindFilter(kind)}
                className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${kindFilter === kind ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
              >
                {meta.label}
              </Button>
            )
          })}
          <div className="ml-auto">
            <Button
              variant="ghost"
              onClick={() => setShowArchived(!showArchived)}
              className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${showArchived ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
            >
              <span className="material-symbols-outlined text-[14px] mr-xs align-middle">archive</span>
              {showArchived ? 'Showing Archived' : 'Hide Archived'}
            </Button>
          </div>
        </div>

        {/* List */}
        {loading ? (
          <div className="space-y-md">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : items.length === 0 ? (
          <EmptyState
            icon="check_circle"
            title="All clear."
            description="No triage items match the current filter."
          />
        ) : (
          <div className="space-y-md">
            {items.map(item => (
              <TriageCard
                key={item.id}
                item={item}
                onMarkRead={markRead}
                onArchive={archive}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

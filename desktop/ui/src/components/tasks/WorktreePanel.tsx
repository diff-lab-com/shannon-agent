// WorktreePanel — P2.5 deliverable.
//
// Lists all task worktrees (Tauri `list_task_worktrees`), lets the user
// create a new worktree for a chosen scheduled routine, remove an
// individual worktree, or prune stale ones. Mirrors the layout of
// HistoryView for visual consistency.
//
// Backend (createTaskWorktree / listTaskWorktrees / removeTaskWorktree /
// pruneTaskWorktrees) was merged in shannon-code PR #18.

import { useMemo, useState } from 'react'
import { Banner } from '@/components/ui/banner'
import { useIntl } from 'react-intl'
import EmptyState from '@/components/ui/empty-state'
import { Button } from '@/components/ui/button'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { useScheduledTasks, useTaskWorktrees } from '@/hooks/scheduled-tasks'
import type { ScheduledRoutine } from '@/types'

function shortPath(p: string): string {
  // Show last 2 path segments for compact display.
  const parts = p.replace(/\\/g, '/').split('/').filter(Boolean)
  if (parts.length <= 2) return '/' + parts.join('/')
  return '/…/' + parts.slice(-2).join('/')
}

export default function WorktreePanel() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { tasks: routines } = useScheduledTasks()
  const { worktrees, loading, error, create, remove, prune, refresh } = useTaskWorktrees()
  const [selectedTaskId, setSelectedTaskId] = useState<string>('')
  const [busy, setBusy] = useState(false)
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null)

  const routineById = useMemo(() => {
    const m = new Map<string, ScheduledRoutine>()
    for (const r of routines) m.set(r.id, r)
    return m
  }, [routines])

  const handleCreate = async () => {
    if (!selectedTaskId) return
    setBusy(true)
    try {
      await create(selectedTaskId)
      setSelectedTaskId('')
    } finally {
      setBusy(false)
    }
  }

  const handlePrune = async () => {
    setBusy(true)
    try {
      await prune()
    } finally {
      setBusy(false)
    }
  }

  const handleRemoveConfirm = async () => {
    if (!confirmRemove) return
    setBusy(true)
    try {
      await remove(confirmRemove)
    } finally {
      setBusy(false)
      setConfirmRemove(null)
    }
  }

  if (loading) {
    return (
      <div className="space-y-md">
        {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
      </div>
    )
  }

  return (
    <div className="space-y-md">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="font-headline-md text-on-surface">{t('tasks.worktreePanel.title')}</h3>
          <p className="text-on-surface-variant text-[13px] mt-xs">
            {t('tasks.worktreePanel.description')}
          </p>
        </div>
        <Button
          variant="outline"
          onClick={handlePrune}
          disabled={busy || worktrees.length === 0}
          aria-label={t('tasks.worktreePanel.pruneStaleAria')}
        >
          <span className="material-symbols-outlined text-[18px]">cleaning_services</span>
          {t('tasks.worktreePanel.pruneStale')}
        </Button>
      </div>

      {error ? (
        <Banner
          variant="card"
          tone="error"
          className="text-error font-label-md"
        >
          <span className="material-symbols-outlined text-[18px]">error</span>
          <span className="flex-1">{error}</span>
        </Banner>
      ) : null}

      {/* Create-worktree picker */}
      <div className="bg-surface-container-lowest/80 border border-outline-variant/20 rounded-xl p-md flex flex-col md:flex-row md:items-end gap-sm">
        <div className="flex-1">
          <label htmlFor="worktree-task-select" className="block font-label-sm text-[11px] text-on-surface-variant uppercase tracking-wider mb-xs">
            {t('tasks.worktreePanel.routine')}
          </label>
          <select
            id="worktree-task-select"
            className="w-full bg-surface-container-low border border-outline-variant rounded-lg px-md py-sm text-on-surface font-label-md focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            value={selectedTaskId}
            onChange={e => setSelectedTaskId(e.target.value)}
            disabled={busy || routines.length === 0}
          >
            <option value="">{routines.length === 0 ? t('tasks.worktreePanel.noRoutines') : t('tasks.worktreePanel.selectRoutine')}</option>
            {routines.map(r => (
              <option key={r.id} value={r.id}>{r.name}</option>
            ))}
          </select>
        </div>
        <Button
          onClick={handleCreate}
          disabled={busy || !selectedTaskId}
          aria-label={t('tasks.worktreePanel.createWorktreeAria')}
        >
          <span className="material-symbols-outlined text-[18px]">add</span>
          {t('tasks.worktreePanel.createWorktree')}
        </Button>
      </div>

      {/* Worktree list */}
      {worktrees.length === 0 ? (
        <div className="bg-surface-container-lowest/70 border border-outline-variant/20 rounded-xl p-xl">
          <EmptyState
            icon="fork_right"
            title={t('tasks.worktreePanel.emptyTitle')}
            description={t('tasks.worktreePanel.emptyDesc')}
            action={{ label: t('tasks.worktreePanel.emptyCta'), onClick: () => void refresh() }}
          />
        </div>
      ) : (
        <div className="space-y-sm">
          {worktrees.map(wt => {
            const routine = routineById.get(wt.task_id)
            return (
              <div
                key={wt.path}
                className="bg-surface-container-lowest/80 border border-outline-variant/20 rounded-xl px-md py-sm flex items-center gap-md"
              >
                <span className="material-symbols-outlined icon-md text-primary">folder_git</span>
                <div className="flex-1 min-w-0">
                  <div className="font-label-md text-[13px] font-bold text-on-surface truncate">
                    {wt.task_name}
                    {routine ? <span className="text-on-surface-variant font-normal"> · {routine.trigger_type}</span> : null}
                  </div>
                  <div className="flex items-center gap-md mt-xs">
                    <span className="font-mono text-[11px] text-on-surface-variant truncate" title={wt.path}>
                      {shortPath(wt.path)}
                    </span>
                    <span className="font-mono text-[11px] text-on-surface-variant truncate" title={wt.branch}>
                      <span className="material-symbols-outlined text-[11px] align-middle">commit</span>
                      {wt.branch}
                    </span>
                  </div>
                </div>
                {confirmRemove === wt.path ? (
                  <div className="flex items-center gap-xs">
                    <Button
                      size="sm"
                      variant="destructive"
                      onClick={handleRemoveConfirm}
                      disabled={busy}
                      aria-label={t('tasks.worktreePanel.confirmRemoveAria')}
                    >
                      {t('tasks.worktreePanel.remove')}
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setConfirmRemove(null)}
                      disabled={busy}
                      aria-label={t('tasks.worktreePanel.cancelRemoveAria')}
                    >
                      {t('tasks.worktreePanel.cancel')}
                    </Button>
                  </div>
                ) : (
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    onClick={() => setConfirmRemove(wt.path)}
                    aria-label={intl.formatMessage({ id: 'tasks.worktreePanel.removeWorktreeAria' }, { name: wt.task_name })}
                    title={t('tasks.worktreePanel.removeWorktree')}
                    className="text-on-surface-variant hover:text-error"
                  >
                    <span className="material-symbols-outlined text-[18px]">delete</span>
                  </Button>
                )}
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}
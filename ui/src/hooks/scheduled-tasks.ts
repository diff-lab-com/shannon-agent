// React hooks wrapping the Sprint 2 scheduled-task Tauri commands.
//
// Each hook manages its own loading/error state and exposes action
// functions that show toasts on success/failure via `sonner`. The pattern
// mirrors how `AppContext.tsx` consumes the existing API module.

import { useCallback, useEffect, useState } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type {
  ScheduledRoutine,
  CreateTaskPayload,
  UpdateTaskPayload,
  CronPreview,
  TriageItem,
  TriageFilter,
  TriageStats,
  TaskExecution,
  TaskExecutionDetail,
  TaskWorktreeDto,
} from '@/types'

// ─── Scheduled tasks (CRUD) ────────────────────────────────────────────────

export function useScheduledTasks() {
  const [tasks, setTasks] = useState<ScheduledRoutine[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setTasks(await api.listScheduledTasks())
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useScheduledTasks.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const create = useCallback(async (payload: CreateTaskPayload): Promise<ScheduledRoutine | null> => {
    try {
      const task = await api.createScheduledTask(payload)
      toast.success('Task created')
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to create task'
      setError(msg)
      toast.error('Failed to create task')
      return null
    }
  }, [refresh])

  const update = useCallback(async (payload: UpdateTaskPayload): Promise<ScheduledRoutine | null> => {
    try {
      const task = await api.updateScheduledTask(payload)
      toast.success('Task updated')
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to update task'
      setError(msg)
      toast.error('Failed to update task')
      return null
    }
  }, [refresh])

  const remove = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.deleteScheduledTask(id)
      toast.success('Task deleted')
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to delete task'
      setError(msg)
      toast.error('Failed to delete task')
      return false
    }
  }, [refresh])

  const toggle = useCallback(async (id: string, enabled: boolean): Promise<ScheduledRoutine | null> => {
    try {
      const task = await api.toggleScheduledTask(id, enabled)
      toast.success(enabled ? 'Task enabled' : 'Task disabled')
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to toggle task'
      setError(msg)
      toast.error('Failed to toggle task')
      return null
    }
  }, [refresh])

  const trigger = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.triggerTaskNow(id)
      toast.success('Task triggered')
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to trigger task'
      setError(msg)
      toast.error('Failed to trigger task')
      return false
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  return { tasks, loading, error, refresh, create, update, remove, toggle, trigger }
}

// ─── Triage items ──────────────────────────────────────────────────────────

export function useTriageItems(initialFilter?: TriageFilter) {
  const [filter, setFilter] = useState<TriageFilter | undefined>(initialFilter)
  const [items, setItems] = useState<TriageItem[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setItems(await api.listTriageItems(filter))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useTriageItems.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [filter])

  const markRead = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.markTriageRead(id)
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to mark item read'
      setError(msg)
      toast.error('Failed to mark item read')
      return false
    }
  }, [refresh])

  const archive = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.archiveTriageItem(id)
      toast.success('Item archived')
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to archive item'
      setError(msg)
      toast.error('Failed to archive item')
      return false
    }
  }, [refresh])

  useEffect(() => { refresh() }, [refresh])

  return { items, loading, error, filter, setFilter, refresh, markRead, archive }
}

// ─── Task executions (history) ─────────────────────────────────────────────

export function useTaskExecutions(taskId?: string) {
  const [executions, setExecutions] = useState<TaskExecution[]>([])
  const [detail, setDetail] = useState<TaskExecutionDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setExecutions(await api.listTaskExecutions(taskId))
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useTaskExecutions.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [taskId])

  const loadDetail = useCallback(async (id: string): Promise<TaskExecutionDetail | null> => {
    try {
      const d = await api.getExecutionDetail(id)
      setDetail(d)
      return d
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to load execution detail'
      setError(msg)
      toast.error('Failed to load execution detail')
      return null
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  return { executions, detail, loading, error, refresh, loadDetail }
}

// ─── Cron preview ──────────────────────────────────────────────────────────

export function useCronPreview() {
  const [preview, setPreview] = useState<CronPreview | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const runPreview = useCallback(async (expr: string): Promise<CronPreview | null> => {
    if (!expr.trim()) {
      setPreview(null)
      return null
    }
    setLoading(true)
    setError(null)
    try {
      const result = await api.previewCron(expr)
      setPreview(result)
      if (!result.valid && result.error) {
        setError(result.error)
      }
      return result
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to preview cron'
      setError(msg)
      return null
    } finally {
      setLoading(false)
    }
  }, [])

  return { preview, loading, error, runPreview }
}

// ─── Triage stats ──────────────────────────────────────────────────────────

export function useTriageStats() {
  const [stats, setStats] = useState<TriageStats>({ total: 0, unread: 0, archived: 0, by_kind: {} })
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setStats(await api.getTriageStats())
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useTriageStats.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => { refresh() }, [refresh])

  return { stats, loading, error, refresh }
}

// ─── Task worktrees (P2.5) ─────────────────────────────────────────────────

export function useTaskWorktrees() {
  const [worktrees, setWorktrees] = useState<TaskWorktreeDto[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      setWorktrees(await api.listTaskWorktrees())
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      setError(msg)
      console.warn('useTaskWorktrees.refresh failed:', e)
    } finally {
      setLoading(false)
    }
  }, [])

  const create = useCallback(async (taskId: string): Promise<TaskWorktreeDto | null> => {
    try {
      const wt = await api.createTaskWorktree(taskId)
      toast.success(`Worktree created for "${wt.task_name}"`)
      await refresh()
      return wt
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to create worktree'
      setError(msg)
      toast.error('Failed to create worktree')
      return null
    }
  }, [refresh])

  const remove = useCallback(async (path: string): Promise<boolean> => {
    try {
      await api.removeTaskWorktree(path)
      toast.success('Worktree removed')
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to remove worktree'
      setError(msg)
      toast.error('Failed to remove worktree')
      return false
    }
  }, [refresh])

  const prune = useCallback(async (): Promise<string[] | null> => {
    try {
      const removed = await api.pruneTaskWorktrees()
      toast.success(removed.length === 0 ? 'No stale worktrees to prune' : `Pruned ${removed.length} worktree${removed.length === 1 ? '' : 's'}`)
      await refresh()
      return removed
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to prune worktrees'
      setError(msg)
      toast.error('Failed to prune worktrees')
      return null
    }
  }, [refresh])

  useEffect(() => { refresh() }, [refresh])

  return { worktrees, loading, error, refresh, create, remove, prune }
}

// React hooks wrapping the Sprint 2 scheduled-task Tauri commands.
//
// Each hook manages its own loading/error state and exposes action
// functions that show toasts on success/failure via `sonner`. The pattern
// mirrors how `AppContext.tsx` consumes the existing API module.

import { useCallback, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
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
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
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
      toast.success(t('tasks.toast.created'))
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.create')
      setError(msg)
      toast.error(t('tasks.toast.failed.create'))
      return null
    }
  }, [refresh, intl])

  const update = useCallback(async (payload: UpdateTaskPayload): Promise<ScheduledRoutine | null> => {
    try {
      const task = await api.updateScheduledTask(payload)
      toast.success(t('tasks.toast.updated'))
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.update')
      setError(msg)
      toast.error(t('tasks.toast.failed.update'))
      return null
    }
  }, [refresh, intl])

  const remove = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.deleteScheduledTask(id)
      toast.success(t('tasks.toast.deleted'))
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.delete')
      setError(msg)
      toast.error(t('tasks.toast.failed.delete'))
      return false
    }
  }, [refresh, intl])

  const toggle = useCallback(async (id: string, enabled: boolean): Promise<ScheduledRoutine | null> => {
    try {
      const task = await api.toggleScheduledTask(id, enabled)
      toast.success(t(enabled ? 'tasks.toast.enabled' : 'tasks.toast.disabled'))
      await refresh()
      return task
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.toggle')
      setError(msg)
      toast.error(t('tasks.toast.failed.toggle'))
      return null
    }
  }, [refresh, intl])

  const trigger = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.triggerTaskNow(id)
      toast.success(t('tasks.toast.triggeredNoName'))
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.trigger')
      setError(msg)
      toast.error(t('tasks.toast.failed.trigger'))
      return false
    }
  }, [intl])

  useEffect(() => { refresh() }, [refresh])

  return { tasks, loading, error, refresh, create, update, remove, toggle, trigger }
}

// ─── Triage items ──────────────────────────────────────────────────────────

export function useTriageItems(initialFilter?: TriageFilter) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
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
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.markRead')
      setError(msg)
      toast.error(t('tasks.toast.failed.markRead'))
      return false
    }
  }, [refresh, intl])

  const archive = useCallback(async (id: string): Promise<boolean> => {
    try {
      await api.archiveTriageItem(id)
      toast.success(t('tasks.toast.archived'))
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.archive')
      setError(msg)
      toast.error(t('tasks.toast.failed.archive'))
      return false
    }
  }, [refresh, intl])

  useEffect(() => { refresh() }, [refresh])

  return { items, loading, error, filter, setFilter, refresh, markRead, archive }
}

// ─── Task executions (history) ─────────────────────────────────────────────

export function useTaskExecutions(taskId?: string) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
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
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.loadExecution')
      setError(msg)
      toast.error(t('tasks.toast.failed.loadExecution'))
      return null
    }
  }, [intl])

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
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)
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
      toast.success(t('tasks.toast.worktreeCreated', { name: wt.task_name }))
      await refresh()
      return wt
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.createWorktree')
      setError(msg)
      toast.error(t('tasks.toast.failed.createWorktree'))
      return null
    }
  }, [refresh, intl])

  const remove = useCallback(async (path: string): Promise<boolean> => {
    try {
      await api.removeTaskWorktree(path)
      toast.success(t('tasks.toast.worktreeRemoved'))
      await refresh()
      return true
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.removeWorktree')
      setError(msg)
      toast.error(t('tasks.toast.failed.removeWorktree'))
      return false
    }
  }, [refresh, intl])

  const prune = useCallback(async (): Promise<string[] | null> => {
    try {
      const removed = await api.pruneTaskWorktrees()
      if (removed.length === 0) {
        toast.success(t('tasks.toast.worktreePrunedNone'))
      } else {
        toast.success(t('tasks.toast.worktreePruned', { count: removed.length }))
      }
      await refresh()
      return removed
    } catch (e) {
      const msg = e instanceof Error ? e.message : t('tasks.toast.failed.pruneWorktrees')
      setError(msg)
      toast.error(t('tasks.toast.failed.pruneWorktrees'))
      return null
    }
  }, [refresh, intl])

  useEffect(() => { refresh() }, [refresh])

  return { worktrees, loading, error, refresh, create, remove, prune }
}

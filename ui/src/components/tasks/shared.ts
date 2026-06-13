// Shared helpers for the Tasks page components.
//
// Extracted from the original monolith to avoid duplication across the 12
// split components. Pure functions only — no React state.

export type FilterStatus = 'all' | 'pending' | 'running' | 'completed'

export interface StatusBadge {
  bg: string
  dot: string
  label: string
  icon: string
  tip: string
}

/// Map a task status string to MD3 badge classes. Used by TaskCard,
/// TaskExecutionLog, and the calendar view.
export function statusBadge(status: string): StatusBadge {
  switch (status) {
    case 'completed':
      return { bg: 'bg-tertiary/10 text-tertiary border-tertiary/20', dot: 'bg-tertiary', label: 'Completed', icon: 'check_circle', tip: 'Task finished successfully' }
    case 'running':
    case 'in_progress':
      return { bg: 'bg-primary/10 text-primary border-primary/20', dot: 'bg-primary animate-pulse', label: 'Running', icon: 'autorenew', tip: 'Task is currently executing' }
    case 'failed':
    case 'error':
      return { bg: 'bg-error/10 text-error border-error/20', dot: 'bg-error', label: 'Failed', icon: 'error', tip: 'Task encountered an error' }
    case 'pending':
      return { bg: 'bg-surface-container-highest text-on-surface-variant border-outline-variant/30', dot: 'bg-outline', label: 'Pending', icon: 'schedule', tip: 'Waiting to be executed' }
    default:
      return { bg: 'bg-surface-container-high text-on-surface-variant border-outline-variant/30', dot: 'bg-outline-variant', label: status, icon: 'task_alt', tip: status }
  }
}

/// Does a task status match the active filter?
export function statusMatchesFilter(status: string, filter: FilterStatus): boolean {
  if (filter === 'all') return true
  if (filter === 'pending') return status === 'pending' || status === 'todo'
  if (filter === 'running') return status === 'running' || status === 'in_progress'
  if (filter === 'completed') return status === 'completed'
  return true
}

/// Format a timestamp as HH:MM.
export function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

/// Format a unix timestamp (seconds) as a locale date string.
export function formatUnixDate(ts: number): string {
  return new Date(ts * 1000).toLocaleDateString([], { month: 'short', day: 'numeric', year: 'numeric' })
}

/// Format a unix timestamp (seconds) as a locale date+time string.
export function formatUnixDateTime(ts: number): string {
  return new Date(ts * 1000).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })
}

export const DAY_NAMES = ['Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa', 'Su'] as const
export const MONTH_NAMES = [
  'January', 'February', 'March', 'April', 'May', 'June',
  'July', 'August', 'September', 'October', 'November', 'December',
] as const

export const TASKS_PER_PAGE = 10

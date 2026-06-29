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

/// Locale-aware long month name for calendar headers (0=January..11=December).
/// Uses Intl.DateTimeFormat so it follows the active UI locale.
export function monthName(locale: string, monthIndex: number): string {
  return new Intl.DateTimeFormat(locale, { month: 'long' }).format(new Date(2024, monthIndex, 1))
}

/// Locale-aware weekday name. `jsDay` is 0=Sun..6=Sat (JS Date convention);
/// for a Monday-first grid pass `(index + 1) % 7`.
export function weekdayName(
  locale: string,
  jsDay: number,
  form: 'long' | 'short' | 'narrow' = 'long',
): string {
  // 2024-01-07 is a Sunday (jsDay 0); adding jsDay lands on the right weekday.
  return new Intl.DateTimeFormat(locale, { weekday: form }).format(new Date(2024, 0, 7 + jsDay))
}

export const TASKS_PER_PAGE = 10

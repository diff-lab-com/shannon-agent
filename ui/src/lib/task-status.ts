// Shared task-status taxonomy for all kanban/task surfaces.
//
// Before this file existed, Mission Control and OPC each defined their own
// column taxonomies with different names, different status mappings, and even
// different terminal states (MC had `Failed`; OPC had `Deprecated`). That made
// "move this task to in-progress" mean different things on different pages.
//
// This module is the single source of truth. Both kanban surfaces, the task
// list, the task drawer, and any future view should consume it.

export type TaskStatusFamily = 'queued' | 'active' | 'blocked' | 'done' | 'failed'

export interface StatusFamilyMeta {
  key: TaskStatusFamily
  /** i18n key for the column header title. */
  titleKey: string
  /** Material Symbols icon name. */
  icon: string
  /** All raw backend statuses that map to this family (lowercase compare). */
  statuses: string[]
  /** Tailwind class for the small dot in headers/totals. */
  dotClass: string
  /** Tailwind class for the column background tint. */
  bgClass: string
}

export const STATUS_FAMILY: Record<TaskStatusFamily, StatusFamilyMeta> = {
  queued: {
    key: 'queued',
    titleKey: 'taskStatus.queued.title',
    icon: 'inbox',
    statuses: ['pending', 'queued', 'ready', 'todo', 'backlog'],
    dotClass: 'bg-outline',
    bgClass: 'bg-surface-container-low/40',
  },
  active: {
    key: 'active',
    titleKey: 'taskStatus.active.title',
    icon: 'play_circle',
    statuses: ['in_progress', 'running', 'active', 'doing'],
    dotClass: 'bg-primary',
    bgClass: 'bg-primary/5',
  },
  blocked: {
    key: 'blocked',
    titleKey: 'taskStatus.blocked.title',
    icon: 'block',
    statuses: ['blocked', 'waiting', 'review', 'pending_review', 'pending'],
    dotClass: 'bg-warning',
    bgClass: 'bg-warning/5',
  },
  done: {
    key: 'done',
    titleKey: 'taskStatus.done.title',
    icon: 'check_circle',
    statuses: ['completed', 'done', 'succeeded', 'shipped'],
    dotClass: 'bg-tertiary',
    bgClass: 'bg-tertiary/5',
  },
  failed: {
    key: 'failed',
    titleKey: 'taskStatus.failed.title',
    icon: 'error',
    statuses: ['failed', 'error', 'canceled', 'cancelled', 'deprecated', 'abandoned'],
    dotClass: 'bg-error',
    bgClass: 'bg-error/5',
  },
}

/** Default column order, left → right. */
export const DEFAULT_COLUMN_ORDER: TaskStatusFamily[] = ['queued', 'active', 'blocked', 'done', 'failed']

/** Map any backend status string to a column family. Unknown → queued. */
export function classifyStatus(status: string | undefined | null | undefined): TaskStatusFamily {
  if (!status) return 'queued'
  const s = status.toLowerCase()
  for (const fam of Object.values(STATUS_FAMILY)) {
    if (fam.statuses.includes(s)) return fam.key
  }
  return 'queued'
}

/** Pick the first raw status string for a family — useful when synthesizing an
 *  update payload and you don't care which exact status name the backend uses. */
export function canonicalStatusFor(family: TaskStatusFamily): string {
  return STATUS_FAMILY[family].statuses[0]
}

/** Priority sort rank — lower sorts first within a column. */
export const PRIORITY_RANK: Record<string, number> = {
  critical: 0,
  high: 1,
  normal: 2,
  low: 3,
}

/** Sort tasks within a column: priority desc, then title asc. Stable. */
export function sortTasksByPriorityThenTitle<T extends { priority?: string; title: string }>(tasks: T[]): T[] {
  return [...tasks].sort((a, b) => {
    const pa = PRIORITY_RANK[a.priority ?? 'normal'] ?? 2
    const pb = PRIORITY_RANK[b.priority ?? 'normal'] ?? 2
    if (pa !== pb) return pa - pb
    return a.title.localeCompare(b.title)
  })
}

/** Group a flat task list into the 5 families, sorted within each. */
export function groupTasksByFamily<T extends { status?: string; priority?: string; title: string }>(
  tasks: T[],
): Record<TaskStatusFamily, T[]> {
  const map: Record<TaskStatusFamily, T[]> = {
    queued: [],
    active: [],
    blocked: [],
    done: [],
    failed: [],
  }
  for (const t of tasks) {
    map[classifyStatus(t.status)].push(t)
  }
  for (const key of Object.keys(map) as TaskStatusFamily[]) {
    map[key] = sortTasksByPriorityThenTitle(map[key])
  }
  return map
}

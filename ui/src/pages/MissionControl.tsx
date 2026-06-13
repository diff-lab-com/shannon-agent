// Mission Control — full-screen kanban grid aggregating tasks across all teams.
//
// G12. Columns are by status family (queued, active, blocked, done, failed).
// Cards are clickable to surface the task ID via onSelectTask (parent wires to
// the TaskDetailDrawer). Drag-and-drop is intentionally out of scope: this is
// an observation-first grid, not a write surface.

import { useMemo, useState } from 'react'
import { useApp } from '@/context/AppContext'
import type { TaskItem } from '@/types'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'

interface MissionControlProps {
  onSelectTask?: (id: string) => void
}

type ColumnKey = 'queued' | 'active' | 'blocked' | 'done' | 'failed'

interface Column {
  key: ColumnKey
  title: string
  emoji: string
  statuses: string[]
  dotClass: string
  bgClass: string
}

const COLUMNS: Column[] = [
  { key: 'queued', title: 'Queued', emoji: 'inbox', statuses: ['pending', 'queued', 'ready'], dotClass: 'bg-outline', bgClass: 'bg-surface-container-low/40' },
  { key: 'active', title: 'In Progress', emoji: 'play_circle', statuses: ['in_progress', 'running', 'active'], dotClass: 'bg-primary', bgClass: 'bg-primary/5' },
  { key: 'blocked', title: 'Blocked', emoji: 'block', statuses: ['blocked', 'waiting'], dotClass: 'bg-warning', bgClass: 'bg-warning/5' },
  { key: 'done', title: 'Completed', emoji: 'check_circle', statuses: ['completed', 'done', 'succeeded'], dotClass: 'bg-tertiary', bgClass: 'bg-tertiary/5' },
  { key: 'failed', title: 'Failed', emoji: 'error', statuses: ['failed', 'error', 'canceled', 'cancelled'], dotClass: 'bg-error', bgClass: 'bg-error/5' },
]

function classify(status: string): ColumnKey {
  const s = status.toLowerCase()
  for (const col of COLUMNS) {
    if (col.statuses.includes(s)) return col.key
  }
  return 'queued'
}

const PRIORITY_RING: Record<string, string> = {
  critical: 'ring-error/40',
  high: 'ring-warning/40',
  normal: 'ring-outline-variant/30',
  low: 'ring-outline-variant/20',
}

export default function MissionControl({ onSelectTask }: MissionControlProps) {
  const { tasks, refreshTasks } = useApp()
  const [localSelectedId, setLocalSelectedId] = useState<string | null>(null)
  const selectedTask = localSelectedId ? tasks.find(t => t.id === localSelectedId) ?? null : null

  const handleSelect = (id: string) => {
    setLocalSelectedId(id)
    onSelectTask?.(id)
  }

  const grouped = useMemo(() => {
    const map: Record<ColumnKey, TaskItem[]> = { queued: [], active: [], blocked: [], done: [], failed: [] }
    for (const t of tasks) {
      map[classify(t.status ?? '')].push(t)
    }
    // Sort within column: critical/high first, then by title.
    const priorityRank: Record<string, number> = { critical: 0, high: 1, normal: 2, low: 3 }
    for (const key of Object.keys(map) as ColumnKey[]) {
      map[key].sort((a, b) => {
        const pa = priorityRank[a.priority ?? 'normal'] ?? 2
        const pb = priorityRank[b.priority ?? 'normal'] ?? 2
        if (pa !== pb) return pa - pb
        return a.title.localeCompare(b.title)
      })
    }
    return map
  }, [tasks])

  const totals = useMemo(() => {
    const t: Record<ColumnKey, number> = { queued: 0, active: 0, blocked: 0, done: 0, failed: 0 }
    for (const k of Object.keys(t) as ColumnKey[]) t[k] = grouped[k].length
    return t
  }, [grouped])

  return (
    <div className="flex-1 flex flex-col w-full overflow-hidden">
      <header className="px-lg py-md border-b border-outline-variant/20 bg-surface-container-lowest/60 backdrop-blur-md">
        <div className="flex items-end justify-between gap-md flex-wrap">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">dashboard</span>
              Mission Control
            </h2>
            <p className="text-on-surface-variant mt-xs text-body-sm">
              Aggregated view across {tasks.length} task{tasks.length === 1 ? '' : 's'} from all teams.
            </p>
          </div>
          <div className="flex gap-xs">
            {COLUMNS.map(c => (
              <div
                key={c.key}
                className={`flex items-center gap-xs px-sm py-xs rounded-full text-label-sm font-label-md ${c.bgClass}`}
              >
                <span className={`w-2 h-2 rounded-full ${c.dotClass}`} />
                <span className="text-on-surface-variant">{c.title}</span>
                <span className="text-on-surface font-bold">{totals[c.key]}</span>
              </div>
            ))}
          </div>
        </div>
      </header>

      <div
        className="flex-1 overflow-x-auto overflow-y-hidden"
        role="grid"
        aria-label="Mission control task grid"
      >
        <div className="h-full flex gap-md p-lg min-w-max">
          {COLUMNS.map(col => (
            <section
              key={col.key}
              role="row"
              aria-label={col.title}
              className={`flex flex-col w-[300px] rounded-2xl border border-outline-variant/20 ${col.bgClass}`}
            >
              <header className="flex items-center justify-between px-md py-sm border-b border-outline-variant/20">
                <div className="flex items-center gap-xs">
                  <span className={`w-2 h-2 rounded-full ${col.dotClass}`} />
                  <span className="font-label-md text-label-md text-on-surface font-bold uppercase tracking-wider">
                    {col.title}
                  </span>
                </div>
                <span className="text-label-sm text-on-surface-variant font-mono">{totals[col.key]}</span>
              </header>
              <div
                className="flex-1 overflow-y-auto p-sm space-y-sm"
                role="gridcell"
                aria-busy={grouped[col.key].length === 0}
              >
                {grouped[col.key].length === 0 ? (
                  <div className="text-center text-label-sm text-on-surface-variant py-xl opacity-60">
                    Nothing here.
                  </div>
                ) : (
                  grouped[col.key].map(task => (
                    <button
                      key={task.id}
                      type="button"
                      onClick={() => handleSelect(task.id)}
                      className={
                        'w-full text-left p-md rounded-xl bg-surface-container-lowest/90 shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all duration-200 cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 ring-1 ' +
                        (PRIORITY_RING[task.priority ?? ''] ?? PRIORITY_RING.normal)
                      }
                    >
                      <div className="flex items-start justify-between gap-xs">
                        <h4 className="font-body-md font-semibold text-on-surface line-clamp-2 flex-1">
                          {task.title}
                        </h4>
                        {task.priority ? (
                          <span className="shrink-0 text-[10px] uppercase tracking-wider font-bold text-on-surface-variant px-xs py-0.5 rounded bg-surface-container-high">
                            {task.priority}
                          </span>
                        ) : null}
                      </div>
                      {task.description ? (
                        <p className="text-label-sm text-on-surface-variant mt-xs line-clamp-2">
                          {task.description}
                        </p>
                      ) : null}
                      <div className="mt-sm flex items-center gap-md text-label-sm text-on-surface-variant">
                        {task.assignee ? (
                          <span className="inline-flex items-center gap-xs">
                            <span className="material-symbols-outlined text-[14px]">smart_toy</span>
                            {task.assignee}
                          </span>
                        ) : null}
                        {task.team ? (
                          <span className="inline-flex items-center gap-xs">
                            <span className="material-symbols-outlined text-[14px]">groups</span>
                            {task.team}
                          </span>
                        ) : null}
                        {task.due_date ? (
                          <span className="inline-flex items-center gap-xs">
                            <span className="material-symbols-outlined text-[14px]">event</span>
                            {new Date(task.due_date * 1000).toLocaleDateString()}
                          </span>
                        ) : null}
                      </div>
                    </button>
                  ))
                )}
              </div>
            </section>
          ))}
        </div>
      </div>

      <TaskDetailDrawer
        task={selectedTask}
        onClose={() => setLocalSelectedId(null)}
        onUpdated={() => void refreshTasks()}
      />
    </div>
  )
}

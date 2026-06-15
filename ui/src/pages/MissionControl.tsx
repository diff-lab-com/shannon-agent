// Mission Control — full-screen kanban grid aggregating tasks across all teams.
//
// SCOPE: READ-ONLY observation surface. No create/edit/cancel actions.
// Click a card to open the TaskDetailDrawer (which itself has the write actions).
//
// DISTINCTION from Tasks and OPC:
//   - Tasks: full CRUD for scheduled routines + history + worktrees.
//   - OPC: agent-orchestration workspace with optimistic DnD (write surface).
//   - MissionControl (this page): cross-team status grid — observation only.
//
// This page now consumes the shared KanbanBoard primitive (mode='observe') and
// the unified task-status taxonomy in lib/task-status.ts. The column taxonomy
// is identical to OPC; only the interaction mode differs (click vs drag).

import { useState } from 'react'
import { useApp } from '@/context/AppContext'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import { KanbanBoard, STATUS_FAMILY, type TaskStatusFamily } from '@/components/shared/KanbanBoard'

interface MissionControlProps {
  onSelectTask?: (id: string) => void
}

export default function MissionControl({ onSelectTask }: MissionControlProps) {
  const { tasks, refreshTasks } = useApp()
  const [localSelectedId, setLocalSelectedId] = useState<string | null>(null)
  const selectedTask = localSelectedId ? tasks.find(t => t.id === localSelectedId) ?? null : null

  const handleSelect = (id: string) => {
    setLocalSelectedId(id)
    onSelectTask?.(id)
  }

  const totals = tasks.reduce<Record<TaskStatusFamily, number>>(
    (acc, t) => {
      // cheap inline classify to avoid recomputing the whole group; KanbanBoard
      // does its own grouping for rendering — this is just for the header chips.
      const s = (t.status ?? '').toLowerCase()
      for (const fam of Object.values(STATUS_FAMILY)) {
        if (fam.statuses.includes(s)) { acc[fam.key]++; break }
      }
      return acc
    },
    { queued: 0, active: 0, blocked: 0, done: 0, failed: 0 },
  )

  const headerExtra = (
    <div className="flex gap-xs flex-wrap">
      {(Object.keys(STATUS_FAMILY) as TaskStatusFamily[]).map(key => {
        const meta = STATUS_FAMILY[key]
        return (
          <div
            key={key}
            className={`flex items-center gap-xs px-sm py-xs rounded-full text-label-sm font-label-md ${meta.bgClass}`}
          >
            <span className={`w-2 h-2 rounded-full ${meta.dotClass}`} />
            <span className="text-on-surface-variant">{meta.title}</span>
            <span className="text-on-surface font-bold">{totals[key]}</span>
          </div>
        )
      })}
    </div>
  )

  return (
    <div className="flex-1 flex flex-col w-full overflow-hidden">
      <header className="px-lg py-md border-b border-outline-variant/20 bg-surface-container-lowest/60 backdrop-blur-md">
        <div className="flex items-end justify-between gap-md flex-wrap">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">dashboard</span>
              Conversations
            </h2>
            <p className="text-on-surface-variant mt-xs text-body-sm">
              Aggregated view across {tasks.length} task{tasks.length === 1 ? '' : 's'} from all teams.
            </p>
          </div>
          {headerExtra}
        </div>
      </header>

      <div className="flex-1 flex min-h-0 px-lg py-lg">
        <KanbanBoard
          tasks={tasks}
          mode="observe"
          onSelectTask={handleSelect}
        />
      </div>

      <TaskDetailDrawer
        task={selectedTask}
        onClose={() => setLocalSelectedId(null)}
        onUpdated={() => void refreshTasks()}
      />
    </div>
  )
}

// OPC (One Person Company) — agent-orchestration workspace.
//
// SCOPE: write surface for orchestrating agents + their work. Has Spawn Agent,
// Stop/Pause/Reassign actions, optimistic DnD on the Kanban board (local
// override — backend update_task_status pending).
//
// DISTINCTION from Tasks and MissionControl:
//   - Tasks: full CRUD for scheduled routines + history + worktrees.
//   - MissionControl: read-only kanban across all teams (observation).
//   - OPC (this page): agent-orchestration workspace with optimistic DnD.
//
// Composition: this file is a thin shell. Logic lives in components/opc/:
//   - OPCMissionFocus: editable strategic-focus statement.
//   - OPCAgentSwarm: agent sidebar + Spawn/Reassign modals + action menu.
//   - OPCKanbanBoard: 5-column kanban with bucketFor() status mapping.

import { useMemo, useState } from 'react'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { useCatalog } from '@/context/CatalogContext'
import OpcAnalyticsDashboard from '@/components/opc/OpcAnalyticsDashboard'
import OPCMissionFocus from '@/components/opc/OPCMissionFocus'
import OPCAgentSwarm from '@/components/opc/OPCAgentSwarm'
import OPCKanbanBoard from '@/components/opc/OPCKanbanBoard'

export default function OPC() {
  const { agents, tasks, config, loading, refreshTasks } = useCatalog()
  // 2026-09 review: teams / projects appear in real multi-team deployments
  // but the Kanban used to be a single flat tasks array. Surface a team
  // filter at the page level so a controller with several teams can scope
  // the Kanban to one team at a time.
  const teamNames = useMemo(() => {
    const set = new Set<string>()
    for (const t of tasks) {
      if ((t as { team?: string | null }).team) set.add((t as { team: string }).team)
      else if (t.assignee) set.add(t.assignee)
    }
    return [...set].sort()
  }, [tasks])
  const [teamFilter, setTeamFilter] = useState<string>('all')
  const filteredTasks = useMemo(() => {
    if (teamFilter === 'all') return tasks
    return tasks.filter((t) => (t as { team?: string | null }).team === teamFilter || t.assignee === teamFilter)
  }, [tasks, teamFilter])

  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <div className="max-w-[1600px] mx-auto animate-in fade-in duration-700">
        <OPCMissionFocus config={config} />

        {loading ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : (
          <>
            <OpcAnalyticsDashboard />
            {teamNames.length > 1 && (
              <div className="flex items-center gap-sm flex-wrap mb-md" role="group" aria-label="Team filter">
                <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">Team</span>
                <button
                  type="button"
                  aria-pressed={teamFilter === 'all'}
                  onClick={() => setTeamFilter('all')}
                  className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${teamFilter === 'all' ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
                >
                  All ({tasks.length})
                </button>
                {teamNames.map(name => {
                  const count = tasks.filter(t => (t as { team?: string | null }).team === name || t.assignee === name).length
                  return (
                    <button
                      key={name}
                      type="button"
                      aria-pressed={teamFilter === name}
                      onClick={() => setTeamFilter(name)}
                      className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${teamFilter === name ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
                    >
                      {name} ({count})
                    </button>
                  )
                })}
              </div>
            )}
            <div className="flex flex-col lg:flex-row gap-lg items-start">
              <OPCAgentSwarm agents={agents} tasks={filteredTasks} />
              <OPCKanbanBoard tasks={filteredTasks} refreshTasks={refreshTasks} />
            </div>
          </>
        )}
      </div>
    </div>
  )
}

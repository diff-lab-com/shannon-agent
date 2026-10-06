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
import { useIntl } from 'react-intl'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { useCatalog } from '@/context/CatalogContext'
import { cn } from '@/lib/utils'
import OpcAnalyticsDashboard from '@/components/opc/OpcAnalyticsDashboard'
import OPCMissionFocus from '@/components/opc/OPCMissionFocus'
import OPCAgentSwarm from '@/components/opc/OPCAgentSwarm'
import OPCKanbanBoard from '@/components/opc/OPCKanbanBoard'
import OPCRunsTable from '@/components/opc/OPCRunsTable'

/** P2-8: the page's two views — the orchestration board (kanban + swarm)
 *  and the cross-agent run table. */
type OpcView = 'board' | 'runs'

export default function OPC() {
  const intl = useIntl()
  const { agents, tasks, config, loading, refreshTasks } = useCatalog()
  // P2-8 — view switch (tab): the board stays the landing view; the runs
  // table is one click away.
  const [view, setView] = useState<OpcView>('board')
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
      <div className="max-w-wide mx-auto animate-in fade-in duration-(--duration-slower)">
        <OPCMissionFocus config={config} />

        {/* P2-8 — 看板 / 运行 view tabs. Radiogroup semantics: the two views
            are mutually exclusive page states, not links. */}
        <div
          className="flex items-center gap-xs mt-lg mb-md"
          role="tablist"
          aria-label={intl.formatMessage({ id: 'opc.view.tabs.aria' })}
        >
          {([
            ['board', 'opc.view.board', 'view_kanban'],
            ['runs', 'opc.view.runs', 'forum'],
          ] as const).map(([key, labelKey, icon]) => (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={view === key}
              data-testid={`opc-view-${key}`}
              onClick={() => setView(key)}
              className={cn(
                'inline-flex items-center gap-xs px-md py-xs rounded-full text-label-sm transition-colors cursor-pointer',
                view === key
                  ? 'bg-primary-container text-on-primary-container font-bold'
                  : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10',
              )}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">{icon}</span>
              {intl.formatMessage({ id: labelKey })}
            </button>
          ))}
        </div>

        {loading ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : view === 'runs' ? (
          <OPCRunsTable />
        ) : (
          <>
            <OpcAnalyticsDashboard />
            {teamNames.length > 1 && (
              <div
                className="flex items-center gap-sm flex-wrap mb-md"
                role="group"
                aria-label={intl.formatMessage({ id: 'opc.teamFilter.aria' })}
              >
                <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">
                  {intl.formatMessage({ id: 'opc.teamFilter.label' })}
                </span>
                <button
                  type="button"
                  aria-pressed={teamFilter === 'all'}
                  onClick={() => setTeamFilter('all')}
                  className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${teamFilter === 'all' ? 'bg-primary-container text-on-primary-container font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
                >
                  {intl.formatMessage({ id: 'opc.teamFilter.all' })} ({tasks.length})
                </button>
                {teamNames.map(name => {
                  const count = tasks.filter(t => (t as { team?: string | null }).team === name || t.assignee === name).length
                  return (
                    <button
                      key={name}
                      type="button"
                      aria-pressed={teamFilter === name}
                      onClick={() => setTeamFilter(name)}
                      className={`px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer ${teamFilter === name ? 'bg-primary-container text-on-primary-container font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'}`}
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

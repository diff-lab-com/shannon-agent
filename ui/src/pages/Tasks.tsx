// Tasks page — thin orchestrator for the Sprint 2 scheduled-tasks UI.
//
// SCOPE: calendar-driven management of SCHEDULED + TRIGGERED routines and
// one-off background tasks. Has create/schedule/cancel write actions,
// calendar/DAG views, and team + status filters.
//
// DISTINCTION from MissionControl and OPC:
//   - MissionControl: read-only kanban across all teams (observation).
//   - OPC: agent-orchestration workspace with optimistic DnD (write surface).
//   - Tasks (this page): full CRUD for scheduled routines + history + worktrees.
//
// Cross-component state (selectedTaskId, filter, calendarView, showFilters)
// lives here; component-local state stays in the components. All visual
// behavior of the original 584-line monolith is preserved.
//
// Backend wiring: the new Tauri scheduled-task commands are loaded via
// useScheduledTasks() and rendered into the calendar (next_fire_at). The
// legacy background-task / agent data still comes from useApp().

import { useMemo, useState } from 'react'
import { toast } from 'sonner'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { useScheduledTasks } from '@/hooks/scheduled-tasks'
import type { CreateTaskPayload } from '@/types'
import { type FilterStatus, statusMatchesFilter, TASKS_PER_PAGE } from '@/components/tasks/shared'
import TasksHeader from '@/components/tasks/TasksHeader'
import TasksFilters from '@/components/tasks/TasksFilters'
import NewTaskForm from '@/components/tasks/NewTaskForm'
import ScheduleForm from '@/components/tasks/ScheduleForm'
import TaskList from '@/components/tasks/TaskList'
import TaskCalendarView from '@/components/tasks/TaskCalendarView'
import TaskDAGView from '@/components/tasks/TaskDAGView'
import CalendarSidebarWidget from '@/components/tasks/CalendarSidebarWidget'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import RoutineDetailDrawer from '@/components/tasks/RoutineDetailDrawer'
import CancelTaskModal from '@/components/tasks/CancelTaskModal'
import TaskExecutionLog from '@/components/tasks/TaskExecutionLog'
import EfficiencyCard from '@/components/tasks/EfficiencyCard'
import AgentAllocation from '@/components/tasks/AgentAllocation'
import HistoryView from '@/components/tasks/HistoryView'
import WorktreePanel from '@/components/tasks/WorktreePanel'
import ScheduleDAGView from '@/components/tasks/ScheduleDAGView'
import HookTaskPipeline from '@/components/tasks/HookTaskPipeline'

type Tab = 'active' | 'history' | 'worktrees'

export default function Tasks() {
  const { tasks, backgroundTasks, agents, refreshTasks, loading } = useApp()
  const { tasks: scheduledTasks, create: createScheduled } = useScheduledTasks()

  // Cross-component state
  const [tab, setTab] = useState<Tab>('active')
  const [running, setRunning] = useState<string | null>(null)
  const [viewMonth, setViewMonth] = useState(new Date().getMonth())
  const [viewYear, setViewYear] = useState(new Date().getFullYear())
  const [showFilters, setShowFilters] = useState(false)
  const [calendarView, setCalendarView] = useState(false)
  const [dagView, setDagView] = useState(false)
  const [selectedDay, setSelectedDay] = useState<number | null>(null)
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null)
  const [selectedRoutineId, setSelectedRoutineId] = useState<string | null>(null)
  const [activeFilter, setActiveFilter] = useState<FilterStatus>('all')
  const [teamFilter, setTeamFilter] = useState<string>('all')
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [showNewTask, setShowNewTask] = useState(false)
  const [showSchedule, setShowSchedule] = useState(false)
  const [newTaskPrompt, setNewTaskPrompt] = useState('')
  const [taskPage, setTaskPage] = useState(1)
  const [cancelTarget, setCancelTarget] = useState<string | null>(null)

  const selectedTask = selectedTaskId
    ? tasks.find(t => t.id === selectedTaskId) ?? backgroundTasks.find(t => t.task_id === selectedTaskId) ?? null
    : null
  const selectedRoutine = selectedRoutineId
    ? scheduledTasks.find(r => r.id === selectedRoutineId) ?? null
    : null

  // G11: derive unique team list from tasks (multi-session aggregated view).
  const teams = useMemo(() => {
    const set = new Set<string>()
    for (const t of tasks) if (t.team) set.add(t.team)
    return Array.from(set).sort()
  }, [tasks])

  const filteredTasks = tasks.filter(t => {
    if (!statusMatchesFilter(t.status, activeFilter)) return false
    if (teamFilter !== 'all' && (t.team ?? '') !== teamFilter) return false
    return true
  })
  const taskTotalPages = Math.ceil(filteredTasks.length / TASKS_PER_PAGE)
  const pagedFilteredTasks = filteredTasks.slice((taskPage - 1) * TASKS_PER_PAGE, taskPage * TASKS_PER_PAGE)

  const completedCount = tasks.filter(t => t.status === 'completed').length
  const efficiencyPct = tasks.length > 0 ? Math.round((completedCount / tasks.length) * 100) : 0

  const prevMonth = () => { if (viewMonth === 0) { setViewMonth(11); setViewYear(viewYear - 1) } else { setViewMonth(viewMonth - 1) } }
  const nextMonth = () => { if (viewMonth === 11) { setViewMonth(0); setViewYear(viewYear + 1) } else { setViewMonth(viewMonth + 1) } }

  const handleStartTask = async (rich?: { prompt: string; assignee: string; priority: string }) => {
    const body = rich?.prompt ?? newTaskPrompt.trim()
    if (!body) return
    try {
      setErrorMsg(null)
      await api.startBackgroundTask(body)
      setNewTaskPrompt('')
      setShowNewTask(false)
      toast.success(rich?.assignee ? `Task assigned to ${rich.assignee}` : 'Task created')
      await refreshTasks()
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : 'Failed to start task'); toast.error('Failed to create task') }
  }

  const handleCreateSchedule = async (payload: CreateTaskPayload) => {
    try {
      setErrorMsg(null)
      const created = await createScheduled(payload)
      if (created) {
        if (created.trigger_type === 'webhook') {
          toast.success(`Webhook ready: copy URL from routine detail`)
        } else {
          toast.success(`Routine "${created.name}" scheduled`)
        }
        setShowSchedule(false)
      }
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : 'Failed to create routine'); toast.error('Failed to create routine') }
  }

  const handleCancelTask = async (id: string) => {
    try {
      setErrorMsg(null)
      await api.cancelBackgroundTask(id)
      toast.success('Task cancelled')
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : 'Failed to cancel task'); toast.error('Failed to cancel task') }
    setCancelTarget(null)
    await refreshTasks()
  }

  const handleRunNow = async (id: string) => {
    setRunning(id)
    try {
      setErrorMsg(null)
      const routine = scheduledTasks.find(t => t.id === id)
      if (routine) {
        await api.triggerTaskNow(id)
        toast.success(`Triggered "${routine.name}"`)
      } else {
        const fallbackTitle = tasks.find(t => t.id === id)?.title ?? id
        await api.startBackgroundTask(`Execute task: ${fallbackTitle}`)
        toast.success('Task started')
      }
      await refreshTasks()
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : 'Failed to run task'); toast.error('Failed to run task') }
    setTimeout(() => setRunning(null), 1500)
  }

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        <TasksHeader
          showFilters={showFilters}
          onToggleFilters={() => setShowFilters(!showFilters)}
          calendarView={calendarView}
          onToggleCalendar={() => { setCalendarView(!calendarView); if (!calendarView) setDagView(false) }}
          dagView={dagView}
          onToggleDag={() => { setDagView(!dagView); if (!dagView) setCalendarView(false) }}
          onToggleNewTask={() => setShowNewTask(!showNewTask)}
          onToggleSchedule={() => setShowSchedule(!showSchedule)}
          teams={teams}
          teamFilter={teamFilter}
          onTeamFilterChange={setTeamFilter}
        />

        {/* P2.2: Active / History / Worktrees tab switcher */}
        <div role="tablist" aria-label="Tasks view" className="flex gap-xs mb-lg border-b border-outline-variant/30">
          {(['active', 'history', 'worktrees'] as const).map(t => {
            const selected = tab === t
            return (
              <button
                key={t}
                role="tab"
                aria-selected={selected}
                onClick={() => setTab(t)}
                className={`px-md py-sm font-label-md text-[13px] font-bold cursor-pointer border-b-2 -mb-px transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 ${
                  selected ? 'border-primary text-primary' : 'border-transparent text-on-surface-variant hover:text-on-surface'
                }`}
              >
                {t === 'active' ? 'Active' : t === 'history' ? 'History' : 'Worktrees'}
              </button>
            )
          })}
        </div>

        {tab === 'history' ? (
          <HistoryView />
        ) : tab === 'worktrees' ? (
          <WorktreePanel />
        ) : (
          <>
        {errorMsg && (
          <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-error/10 border border-error/20 text-error font-label-md mb-lg">
            <span className="material-symbols-outlined text-[18px]">error</span>
            {errorMsg}
            <button
              className="ml-auto text-error/60 hover:text-error cursor-pointer"
              onClick={() => setErrorMsg(null)}
            >
              <span className="material-symbols-outlined text-[18px]">close</span>
            </button>
          </div>
        )}

        {showNewTask && (
          <NewTaskForm
            value={newTaskPrompt}
            onChange={setNewTaskPrompt}
            onSubmit={(rich) => handleStartTask(rich)}
            onCancel={() => { setShowNewTask(false); setNewTaskPrompt('') }}
          />
        )}

        {showSchedule && (
          <ScheduleForm
            onSubmit={handleCreateSchedule}
            onCancel={() => setShowSchedule(false)}
          />
        )}

        {showFilters && <TasksFilters active={activeFilter} onChange={setActiveFilter} />}

        {dagView ? (
          <TaskDAGView tasks={tasks} onSelectTask={setSelectedTaskId} />
        ) : calendarView ? (
          <TaskCalendarView
            viewMonth={viewMonth}
            viewYear={viewYear}
            selectedDay={selectedDay}
            filteredTasks={filteredTasks}
            allTasks={tasks}
            agents={agents}
            scheduledTasks={scheduledTasks}
            efficiencyPct={efficiencyPct}
            onSelectDay={setSelectedDay}
            onSelectTask={setSelectedTaskId}
          />
        ) : (
          <div className="grid grid-cols-12 gap-gutter">
            <TaskList
              tasks={pagedFilteredTasks}
              loading={loading}
              page={taskPage}
              totalPages={taskTotalPages}
              onPageChange={setTaskPage}
              runningId={running}
              onSelectTask={setSelectedTaskId}
              onRunNow={handleRunNow}
              onCancelTask={setCancelTarget}
            />
            <div className="col-span-12 lg:col-span-4 space-y-gutter">
              <CalendarSidebarWidget
                viewMonth={viewMonth}
                viewYear={viewYear}
                onPrevMonth={prevMonth}
                onNextMonth={nextMonth}
                tasks={tasks}
                scheduledTasks={scheduledTasks}
                onSelectTask={setSelectedTaskId}
                onSelectRoutine={setSelectedRoutineId}
              />
              <EfficiencyCard percentage={efficiencyPct} variant="full" />
              <AgentAllocation agents={agents} />
              <HookTaskPipeline />
            </div>
            <div className="col-span-12">
              <ScheduleDAGView routines={scheduledTasks} onSelectRoutine={setSelectedRoutineId} />
            </div>
            <div className="col-span-12">
              <TaskExecutionLog tasks={backgroundTasks} onCancel={setCancelTarget} />
            </div>
          </div>
        )}
          </>
        )}
      </div>

      <TaskDetailDrawer
        task={selectedTask}
        onClose={() => setSelectedTaskId(null)}
        onUpdated={() => void refreshTasks()}
      />
      <RoutineDetailDrawer
        routine={selectedRoutine}
        routines={scheduledTasks}
        onClose={() => setSelectedRoutineId(null)}
        onUpdated={() => {/* useScheduledTasks auto-refreshes via its own hook */}}
      />
      <CancelTaskModal
        open={cancelTarget !== null}
        onCancel={() => setCancelTarget(null)}
        onConfirm={() => cancelTarget && handleCancelTask(cancelTarget)}
      />
    </div>
  )
}

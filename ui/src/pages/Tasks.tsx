// Tasks page — thin orchestrator for the Sprint 2 scheduled-tasks UI.
//
// Cross-component state (selectedTaskId, filter, calendarView, showFilters)
// lives here; component-local state stays in the components. All visual
// behavior of the original 584-line monolith is preserved.
//
// Backend wiring: the new Tauri scheduled-task commands are loaded via
// useScheduledTasks() and rendered into the calendar (next_fire_at). The
// legacy background-task / agent data still comes from useApp().

import { useState } from 'react'
import { toast } from 'sonner'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { useScheduledTasks } from '@/hooks/scheduled-tasks'
import { type FilterStatus, statusMatchesFilter, TASKS_PER_PAGE } from '@/components/tasks/shared'
import TasksHeader from '@/components/tasks/TasksHeader'
import TasksFilters from '@/components/tasks/TasksFilters'
import NewTaskForm from '@/components/tasks/NewTaskForm'
import TaskList from '@/components/tasks/TaskList'
import TaskCalendarView from '@/components/tasks/TaskCalendarView'
import CalendarSidebarWidget from '@/components/tasks/CalendarSidebarWidget'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import CancelTaskModal from '@/components/tasks/CancelTaskModal'
import TaskExecutionLog from '@/components/tasks/TaskExecutionLog'
import EfficiencyCard from '@/components/tasks/EfficiencyCard'
import AgentAllocation from '@/components/tasks/AgentAllocation'
import HistoryView from '@/components/tasks/HistoryView'

type Tab = 'active' | 'history'

export default function Tasks() {
  const { tasks, backgroundTasks, agents, refreshTasks, loading } = useApp()
  const { tasks: scheduledTasks } = useScheduledTasks()

  // Cross-component state
  const [tab, setTab] = useState<Tab>('active')
  const [running, setRunning] = useState<string | null>(null)
  const [viewMonth, setViewMonth] = useState(new Date().getMonth())
  const [viewYear, setViewYear] = useState(new Date().getFullYear())
  const [showFilters, setShowFilters] = useState(false)
  const [calendarView, setCalendarView] = useState(false)
  const [selectedDay, setSelectedDay] = useState<number | null>(null)
  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null)
  const [activeFilter, setActiveFilter] = useState<FilterStatus>('all')
  const [errorMsg, setErrorMsg] = useState<string | null>(null)
  const [showNewTask, setShowNewTask] = useState(false)
  const [newTaskPrompt, setNewTaskPrompt] = useState('')
  const [taskPage, setTaskPage] = useState(1)
  const [cancelTarget, setCancelTarget] = useState<string | null>(null)

  const selectedTask = selectedTaskId
    ? tasks.find(t => t.id === selectedTaskId) ?? backgroundTasks.find(t => t.task_id === selectedTaskId) ?? null
    : null

  const filteredTasks = tasks.filter(t => statusMatchesFilter(t.status, activeFilter))
  const taskTotalPages = Math.ceil(filteredTasks.length / TASKS_PER_PAGE)
  const pagedFilteredTasks = filteredTasks.slice((taskPage - 1) * TASKS_PER_PAGE, taskPage * TASKS_PER_PAGE)

  const completedCount = tasks.filter(t => t.status === 'completed').length
  const efficiencyPct = tasks.length > 0 ? Math.round((completedCount / tasks.length) * 100) : 0

  const prevMonth = () => { if (viewMonth === 0) { setViewMonth(11); setViewYear(viewYear - 1) } else { setViewMonth(viewMonth - 1) } }
  const nextMonth = () => { if (viewMonth === 11) { setViewMonth(0); setViewYear(viewYear + 1) } else { setViewMonth(viewMonth + 1) } }

  const handleStartTask = async () => {
    if (!newTaskPrompt.trim()) return
    try {
      setErrorMsg(null)
      await api.startBackgroundTask(newTaskPrompt.trim())
      setNewTaskPrompt('')
      setShowNewTask(false)
      toast.success('Task created')
      await refreshTasks()
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : 'Failed to start task'); toast.error('Failed to create task') }
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
          onToggleCalendar={() => setCalendarView(!calendarView)}
          onToggleNewTask={() => setShowNewTask(!showNewTask)}
        />

        {/* P2.2: Active / History tab switcher */}
        <div role="tablist" aria-label="Tasks view" className="flex gap-xs mb-lg border-b border-outline-variant/30">
          {(['active', 'history'] as const).map(t => {
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
                {t === 'active' ? 'Active' : 'History'}
              </button>
            )
          })}
        </div>

        {tab === 'history' ? (
          <HistoryView />
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
            onSubmit={handleStartTask}
            onCancel={() => { setShowNewTask(false); setNewTaskPrompt('') }}
          />
        )}

        {showFilters && <TasksFilters active={activeFilter} onChange={setActiveFilter} />}

        {calendarView ? (
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
              />
              <EfficiencyCard percentage={efficiencyPct} variant="full" />
              <AgentAllocation agents={agents} />
            </div>
            <div className="col-span-12">
              <TaskExecutionLog tasks={backgroundTasks} onCancel={setCancelTarget} />
            </div>
          </div>
        )}
          </>
        )}
      </div>

      <TaskDetailDrawer task={selectedTask} onClose={() => setSelectedTaskId(null)} />
      <CancelTaskModal
        open={cancelTarget !== null}
        onCancel={() => setCancelTarget(null)}
        onConfirm={() => cancelTarget && handleCancelTask(cancelTarget)}
      />
    </div>
  )
}

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
// legacy background-task / agent data still comes from useCatalog().

import { useMemo, useState, useEffect } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import { toastError } from '@/lib/errorToast'
import { useIntl } from 'react-intl'
import { useCatalog } from '@/context/CatalogContext'
import { useSessions } from '@/context/SessionContext'
import * as api from '@/lib/tauri-api'
import { useScheduledTasks, useTaskExecutions } from '@/hooks/scheduled-tasks'
import { useBatchRuns } from '@/hooks/batchRuns'
import { useProjectDeepLink } from '@/hooks/projectDeepLink'
import ProjectFilterChip from '@/components/ProjectFilterChip'
import { projectKeyOf } from '@/components/SidebarSessions'
import type { CreateTaskPayload } from '@/types'
import { type FilterStatus, statusMatchesFilter, TASKS_PER_PAGE } from '@/components/tasks/shared'
import { useSidebarMode } from '@/components/Sidebar'
import { Banner } from '@/components/ui/banner'
import TasksHeader from '@/components/tasks/TasksHeader'
import RoutineTemplatesBrowser from '@/components/routines/RoutineTemplatesBrowser'
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
import GoalRunPanel from '@/components/tasks/GoalRunPanel'
import BatchRunPanel from '@/components/tasks/BatchRunPanel'
import BatchForm from '@/components/tasks/BatchForm'
import SubagentPanel from '@/components/tasks/SubagentPanel'
import WebhookTriggerCard from '@/components/tasks/WebhookTriggerCard'
import ScheduleDAGView from '@/components/tasks/ScheduleDAGView'
import HookTaskPipeline from '@/components/tasks/HookTaskPipeline'

// IA (2026-09): tabs map to user jobs, not implementation panels —
// active work / history / routines / pipelines / worktrees. Active + history
// lead because users check live status and recent results far more often than
// they configure scheduled pipelines. In Simple mode only the universal two
// (active / history) are surfaced; the developer-only surfaces (routines /
// pipelines / worktrees) move behind the Dev-mode toggle so casual users
// don't have to learn the task-ops taxonomy before they can find their tasks.
type Tab = 'active' | 'history' | 'routines' | 'pipelines' | 'worktrees'
const SIMPLE_TABS: readonly Tab[] = ['active', 'history']
const DEV_TABS: readonly Tab[] = ['active', 'history', 'routines', 'pipelines', 'worktrees']

export default function Tasks() {
  const { tasks, backgroundTasks, agents, refreshTasks, loading } = useCatalog()
  const { switchSession, currentSessionId } = useSessions()
  const navigate = useNavigate()
  const location = useLocation()
  const [mode] = useSidebarMode()
  const { tasks: scheduledTasks, create: createScheduled, refresh: refreshScheduled } = useScheduledTasks()
  // P2-5: recent executions across all routines — drives the "queued for
  // off-peak window" status chip on the routines DAG nodes.
  const { executions } = useTaskExecutions()
  // P1-2: start action for the batch form (the live cards in BatchRunPanel
  // keep their own subscription, mirroring the goal-run split).
  const { start: startBatch } = useBatchRuns()
  // P-U3: /tasks?project=<encoded path> — scope the page to one project.
  // Drives the removable chip, the 例行 tab's working_dir filter, the
  // goal-run cards (dto.workingDir) and the execution history (joined
  // through its routine's working_dir).
  const { projectKey, projectLabel, clearProject } = useProjectDeepLink()
  // I2 (review fix): the project menu's 新建例行 deep-links here with
  // ?project=…&new=routine — distinct from 查看自动化's plain URL. The
  // marker opens the create-schedule form, then is drained (replace
  // navigation) so a refresh doesn't re-open it.
  const [searchParams, setSearchParams] = useSearchParams()
  const newRoutineMarker = searchParams.get('new') === 'routine'
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })

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
  // P1-2: best-of-N batch creation form + its data panel (live cards below).
  const [showBatchForm, setShowBatchForm] = useState(false)
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

  // IA T2 (互链闭环): Triage cards link here with { openRoutineId } in the
  // router state — open that routine's drawer, then drain the state so a
  // refresh doesn't re-open it.
  useEffect(() => {
    const openRoutineId = (location.state as { openRoutineId?: string } | null)?.openRoutineId
    if (openRoutineId) {
      setSelectedRoutineId(openRoutineId)
      navigate(location.pathname, { replace: true })
    }
  }, [location.state, location.pathname, navigate])

  // P2-5: ids of routines whose latest run is queued for their off-peak
  // execution window (list_task_executions returns newest first).
  const queuedRoutineIds = useMemo(() => {
    const seen = new Set<string>()
    const queued = new Set<string>()
    for (const e of executions) {
      if (seen.has(e.task_id)) continue
      seen.add(e.task_id)
      if (e.status === 'queued') queued.add(e.task_id)
    }
    return queued
  }, [executions])

  // G11: derive unique team list from tasks (multi-session aggregated view).
  const teams = useMemo(() => {
    const set = new Set<string>()
    for (const t of tasks) if (t.team) set.add(t.team)
    return Array.from(set).sort()
  }, [tasks])

  // P-U3: routines scoped to the deep-linked project (working_dir matches the
  // normalized project key). Routines with no working_dir drop out while the
  // filter is active — they are not the project's automations.
  const scopedRoutines = useMemo(() => {
    if (!projectKey) return scheduledTasks
    return scheduledTasks.filter(r => projectKeyOf({ working_dir: r.working_dir }) === projectKey)
  }, [scheduledTasks, projectKey])

  // P-U3: task_id → routine working_dir join for the execution-history tab
  // (rows only know the routine id; the project lives on the routine).
  const routineDirById = useMemo(() => {
    const m: Record<string, string | null | undefined> = {}
    for (const r of scheduledTasks) m[r.id] = r.working_dir
    return m
  }, [scheduledTasks])

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
      toast.success(rich?.assignee
        ? intl.formatMessage({ id: 'tasks.toast.assigned' }, { name: rich.assignee })
        : t('tasks.toast.created'))
      await refreshTasks()
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : t('tasks.error.create')); toastError(t('tasks.toast.failed.create'), e) }
  }

  // I2: 新建例行 marker → open the create form, drain the marker param.
  useEffect(() => {
    if (!newRoutineMarker) return
    setShowSchedule(true)
    const next = new URLSearchParams(searchParams)
    next.delete('new')
    setSearchParams(next, { replace: true })
  }, [newRoutineMarker, searchParams, setSearchParams])

  const handleCreateSchedule = async (payload: CreateTaskPayload) => {
    try {
      setErrorMsg(null)
      // I2 (review fix): a routine created from a project deep link must
      // land HOUSED in that project — default working_dir to the active
      // ?project= key when the form didn't set one. Without it the routine
      // is unhoused and instantly vanishes from the scopedRoutines view.
      const created = await createScheduled({
        ...payload,
        working_dir: payload.working_dir ?? projectKey ?? undefined,
      })
      if (created) {
        if (created.trigger_type === 'webhook') {
          toast.success(t('tasks.toast.webhookReady'))
        } else {
          toast.success(intl.formatMessage({ id: 'tasks.toast.routineScheduled' }, { name: created.name }))
        }
        setShowSchedule(false)
      }
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : t('tasks.error.createRoutine')); toastError(t('tasks.toast.failed.createRoutine'), e) }
  }

  const handleCancelTask = async (id: string) => {
    try {
      setErrorMsg(null)
      await api.cancelBackgroundTask(id)
      toast.success(t('tasks.toast.cancelled'))
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : t('tasks.error.cancel')); toastError(t('tasks.toast.failed.cancel'), e) }
    setCancelTarget(null)
    await refreshTasks()
  }

  // P1-23: `running` is a real pending flag now — it tracks the in-flight
  // trigger call and clears when it settles (previously a fixed 1.5s
  // setTimeout faked success and leaked a timer across unmounts).
  const handleRunNow = async (id: string) => {
    setRunning(id)
    try {
      setErrorMsg(null)
      const routine = scheduledTasks.find(task => task.id === id)
      if (routine) {
        await api.triggerTaskNow(id)
        toast.success(intl.formatMessage({ id: 'tasks.toast.triggered' }, { name: routine.name }))
      } else {
        const fallbackTitle = tasks.find(task => task.id === id)?.title ?? id
        await api.startBackgroundTask(intl.formatMessage({ id: 'tasks.toast.executeTask' }, { name: fallbackTitle }))
        toast.success(t('tasks.toast.started'))
      }
      await refreshTasks()
    } catch (e) { setErrorMsg(e instanceof Error ? e.message : t('tasks.error.run')); toastError(t('tasks.toast.failed.run'), e) }
    finally {
      setRunning(null)
    }
  }

  // P0-2: open the session a goal run is driving in the chat page.
  const handleViewGoalSession = async (sessionId: string) => {
    try {
      await switchSession(sessionId)
    } catch (e) {
      toastError(t('goal.toast.failed.viewSession'), e)
      return
    }
    navigate('/chat')
  }

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* E2E (extensions.spec.ts) and screen readers look up the Tasks page
            by its h1 — the visible h1/h2 was retired in P0-3 (the global app
            Header carries the page name now), but a sr-only h1 keeps the page
            self-identifying for AT, axe, and the smoke test without forcing
            the design to re-adopt a visible page title. */}
        <h1 className="sr-only">{t('tasks.tasksHeader.title')}</h1>
        <TasksHeader
          showFilters={showFilters}
          onToggleFilters={() => setShowFilters(!showFilters)}
          calendarView={calendarView}
          onToggleCalendar={() => { setCalendarView(!calendarView); if (!calendarView) setDagView(false) }}
          dagView={dagView}
          onToggleDag={() => { setDagView(!dagView); if (!dagView) setCalendarView(false) }}
          onToggleNewTask={() => setShowNewTask(!showNewTask)}
          onToggleBatch={() => setShowBatchForm(!showBatchForm)}
          onToggleSchedule={() => setShowSchedule(!showSchedule)}
          teams={teams}
          teamFilter={teamFilter}
          onTeamFilterChange={setTeamFilter}
          mode={mode}
        />

        {/* P-U3: project deep-link chip — × strips ?project= and the page
            falls back to the unscoped view. */}
        {projectKey && projectLabel && (
          <ProjectFilterChip label={projectLabel} onRemove={clearProject} />
        )}

        {/* P2.2: Active / History / Worktrees tab switcher — Simple mode
            only shows the two universal tabs; the dev-only surfaces move
            behind the sidebar Dev-mode toggle. */}
        <div role="tablist" aria-label={t('tasks.tabs.aria')} className="flex gap-xs mb-lg border-b border-outline-variant/30">
          {(mode === 'dev' ? DEV_TABS : SIMPLE_TABS).map(tabId => {
            const selected = tab === tabId
            return (
              <Button
                key={tabId}
                role="tab"
                variant="ghost"
                aria-selected={selected}
                onClick={() => setTab(tabId)}
                title={t(`tasks.tab.${tabId}.title`)}
                className={cn(
                  'h-auto px-md py-sm font-label-md text-[13px] font-bold cursor-pointer border-b-2 -mb-px transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded-none',
                  selected ? 'border-primary text-primary' : 'border-transparent text-on-surface-variant hover:text-on-surface',
                )}
              >
                {t(`tasks.tab.${tabId}`)}
              </Button>
            )
          })}
        </div>

        {tab === 'history' ? (
          <HistoryView
            onGoToActive={() => setTab('active')}
            projectDir={projectKey}
            routineDirById={routineDirById}
          />
        ) : tab === 'worktrees' ? (
          <WorktreePanel />
        ) : tab === 'routines' ? (
          <div className="space-y-gutter">
            <ScheduleDAGView routines={scopedRoutines} onSelectRoutine={setSelectedRoutineId} queuedTaskIds={queuedRoutineIds} />
            <WebhookTriggerCard routines={scopedRoutines} />
            <RoutineTemplatesBrowser onInstantiated={() => void refreshScheduled()} />
          </div>
        ) : tab === 'pipelines' ? (
          <div className="space-y-gutter">
            <HookTaskPipeline />
            <TaskExecutionLog tasks={backgroundTasks} onCancel={setCancelTarget} />
          </div>
        ) : (
          <>
        {/* 操作员视图（§5-1 裁决）— the panels below (batch cards, goal runs,
            subagent inventory) are the operator surfaces; stay out of scope
            for the nav/IA redesign unless the proposal says otherwise. */}
        {/* P1-2: best-of-N batch cards (live per-branch chips) + form. */}
        <BatchRunPanel />

        {showBatchForm && (
          <BatchForm
            sessionId={currentSessionId}
            onSubmit={async ({ title, prompt, count, sessionId }) => {
              const ok = await startBatch({ title, prompt, count, baseSessionId: sessionId })
              if (ok !== null) {
                setShowBatchForm(false)
              }
            }}
            onCancel={() => setShowBatchForm(false)}
          />
        )}

        {/* P0-2: live goal-run cards sit above the regular task list.
            P-U3: scoped to the deep-linked project when ?project= is set. */}
        <GoalRunPanel onViewSession={handleViewGoalSession} projectDir={projectKey} />

        {/* B2 follow-up: live sub-agent inventory (system-wide). Hidden
            when the user has not enabled agent teams. */}
        <SubagentPanel />

        {errorMsg && (
          <Banner
            variant="card"
            tone="error"
            onDismiss={() => setErrorMsg(null)}
            dismissLabel={t('common.dismiss')}
            className="mb-lg text-error font-label-md"
          >
            <span className="material-symbols-outlined icon-md text-error">error</span>
            <span className="flex-1">{errorMsg}</span>
          </Banner>
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
              onCreateTask={() => setShowNewTask(true)}
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

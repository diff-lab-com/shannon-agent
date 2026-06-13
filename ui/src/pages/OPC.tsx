import { useState, useEffect, useMemo, useRef } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'
import type { TaskItem } from '@/types'
import * as api from '@/lib/tauri-api'
import OpcAnalyticsDashboard from '@/components/opc/OpcAnalyticsDashboard'

const AGENT_ICONS: Record<string, string> = {
  research: 'query_stats',
  cod: 'code',
  dev: 'code',
  test: 'bug_report',
  qa: 'bug_report',
  design: 'palette',
  ops: 'trending_up',
  cloud: 'cloud',
  schedule: 'schedule',
  edit: 'edit_square',
}

function iconForAgent(name: string): string {
  const n = name.toLowerCase()
  for (const [key, icon] of Object.entries(AGENT_ICONS)) {
    if (n.includes(key)) return icon
  }
  return 'smart_toy'
}

// Maps Kanban column → acceptable task statuses (mirrors the bucketing below)
const COLUMN_STATUSES: Record<ColumnId, string[]> = {
  todo: ['pending', 'todo'],
  pending: ['review', 'blocked'],
  doing: ['in_progress', 'running'],
  done: ['completed'],
  deprecated: ['deprecated'],
}

type ColumnId = 'todo' | 'pending' | 'doing' | 'done' | 'deprecated'

function bucketFor(status: string): ColumnId {
  for (const [col, statuses] of Object.entries(COLUMN_STATUSES)) {
    if (statuses.includes(status)) return col as ColumnId
  }
  return 'todo'
}

// Shorten a worktree path to its tail for compact display: "/Users/x/worktrees/foo" → "/foo"
function shortWorktree(p: string): string {
  const trimmed = p.replace(/\/+$/, '')
  const slash = trimmed.lastIndexOf('/')
  return slash >= 0 ? trimmed.slice(slash) : trimmed
}

export default function OPC() {
  const { agents, tasks, config, loading, refreshTasks } = useApp()
  const navigate = useNavigate()
  const [quickTask, setQuickTask] = useState('')
  const [editingFocus, setEditingFocus] = useState(false)
  const [focusText, setFocusText] = useState('')
  // Local override layer — when the user drag-drops a card, we patch the task
  // status locally until a backend update_task_status API lands.
  const [overrides, setOverrides] = useState<Record<string, string>>({})
  const [openMenuId, setOpenMenuId] = useState<string | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)

  // C5: Spawn Agent drawer state
  const [spawnOpen, setSpawnOpen] = useState(false)
  const [spawnName, setSpawnName] = useState('')
  const [spawnModel, setSpawnModel] = useState('')
  const [spawnPrompt, setSpawnPrompt] = useState('')
  const [spawnTools, setSpawnTools] = useState<Record<string, boolean>>({ bash: true, read: true, write: true })
  const [spawnError, setSpawnError] = useState<string | null>(null)
  const [spawnSaving, setSpawnSaving] = useState(false)

  // F6: Reassign modal state
  const [reassignTarget, setReassignTarget] = useState<{ agentId: string; agentName: string; taskId: string; taskTitle: string } | null>(null)
  const [reassignPick, setReassignPick] = useState('')
  const [reassignSaving, setReassignSaving] = useState(false)
  const [reassignError, setReassignError] = useState<string | null>(null)

  const resetSpawn = () => {
    setSpawnName(''); setSpawnModel(''); setSpawnPrompt('')
    setSpawnTools({ bash: true, read: true, write: true })
    setSpawnError(null); setSpawnSaving(false)
  }

  const handleSpawnAgent = async () => {
    if (!spawnName.trim()) { setSpawnError('Agent name is required'); return }
    setSpawnSaving(true); setSpawnError(null)
    const tools = Object.entries(spawnTools).filter(([, v]) => v).map(([k]) => k)
    try {
      await api.createAgentDefinition(spawnName.trim(), spawnModel || undefined, spawnPrompt || undefined, tools)
      toast.success(`Agent "${spawnName.trim()}" created`)
      resetSpawn(); setSpawnOpen(false)
    } catch (e) {
      console.warn('Failed to create agent:', e)
      setSpawnError(e instanceof Error ? e.message : 'Failed to create agent')
    } finally {
      setSpawnSaving(false)
    }
  }

  const currentFocus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : 'Autonomous task execution through multi-agent orchestration and intelligent coordination.')

  useEffect(() => { setFocusText(currentFocus) }, [currentFocus])

  // Close agent menu on outside click
  useEffect(() => {
    if (!openMenuId) return
    function onDown(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setOpenMenuId(null)
      }
    }
    document.addEventListener('mousedown', onDown)
    return () => document.removeEventListener('mousedown', onDown)
  }, [openMenuId])

  const handleSaveFocus = () => {
    import('@/lib/tauri-api').then(api => api.configure({ key: 'strategic_focus', value: focusText })).then(() => toast.success('Strategic focus saved')).catch(() => toast.error('Failed to save focus'))
    setEditingFocus(false)
  }

  const handleQuickTask = async () => {
    const trimmed = quickTask.trim()
    if (!trimmed) return
    try {
      await import('@/lib/tauri-api').then(api => api.startBackgroundTask(trimmed))
      setQuickTask('')
      toast.success('Task created')
    } catch (e) { console.warn('Failed to start quick task:', e); toast.error('Failed to create task') }
  }

  // Merge overrides into tasks list so DnD moves reflect immediately
  const effectiveTasks: TaskItem[] = useMemo(
    () => tasks.map(t => (overrides[t.id] ? { ...t, status: overrides[t.id] } : t)),
    [tasks, overrides],
  )

  const todoTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'todo')
  const pendingTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'pending')
  const inProgressTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'doing')
  const doneTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'done')
  const deprecatedTasks = effectiveTasks.filter(t => bucketFor(t.status) === 'deprecated')

  // F4: native HTML5 DnD — no extra deps. Backend update_task_status is not
  // wired yet, so we apply an optimistic local override and surface a toast.
  const handleDrop = (taskId: string, target: ColumnId) => {
    const task = effectiveTasks.find(t => t.id === taskId)
    if (!task) return
    const current = bucketFor(task.status)
    if (current === target) return
    const newStatus = COLUMN_STATUSES[target][0]
    setOverrides(prev => ({ ...prev, [taskId]: newStatus }))
    toast.success(`Moved to ${target}`, { description: 'Local override — backend persistence pending' })
  }

  // F5: agent card click → navigate to task detail (session-aware)
  const handleAgentClick = (agentId: string, sessionId?: string) => {
    setOpenMenuId(null)
    const target = sessionId ? `/opc/task?agent=${agentId}&session=${sessionId}` : `/opc/task?agent=${agentId}`
    navigate(target)
  }

  // F6: agent menu actions
  const handleStopAgent = async (agentId: string, name: string) => {
    setOpenMenuId(null)
    try {
      await import('@/lib/tauri-api').then(api => api.cancelBackgroundTask(agentId))
      toast.success(`Stopped ${name}`)
    } catch (e) {
      console.warn('Failed to stop agent:', e)
      toast.error(`Failed to stop ${name}`)
    }
  }

  // F6 View Logs: navigate to OPCTask page filtered to this agent's task
  const handleViewLogs = (agentId: string, sessionId?: string) => {
    setOpenMenuId(null)
    const target = sessionId ? `/opc/task?agent=${agentId}&session=${sessionId}` : `/opc/task?agent=${agentId}`
    navigate(target)
  }

  // F6 Pause: real pause is not yet a backend command. Surface a toast that
  // explains what's missing so users aren't misled into thinking it worked.
  const handlePauseAgent = (name: string) => {
    setOpenMenuId(null)
    toast.info(`Pause not yet supported for ${name}`, { description: 'Backend pause command pending — use Stop to halt.' })
  }

  // F6 Reassign: find the in-progress task this agent owns, open picker
  const handleReassignOpen = (agent: { id: string; name: string }) => {
    setOpenMenuId(null)
    const owned = effectiveTasks.find(t => t.assignee === agent.name && (t.status === 'in_progress' || t.status === 'running' || t.status === 'pending'))
    if (!owned) {
      toast.info(`No active task to reassign for ${agent.name}`)
      return
    }
    setReassignTarget({ agentId: agent.id, agentName: agent.name, taskId: owned.id, taskTitle: owned.title })
    setReassignPick('')
    setReassignError(null)
  }

  const handleReassignSubmit = async () => {
    if (!reassignTarget) return
    const pick = reassignPick.trim()
    if (!pick) { setReassignError('Pick an agent'); return }
    setReassignSaving(true); setReassignError(null)
    try {
      await import('@/lib/tauri-api').then(api => api.updateTask({ id: reassignTarget.taskId, assignee: pick }))
      toast.success(`Reassigned "${reassignTarget.taskTitle}" to ${pick}`)
      setReassignTarget(null)
    } catch (e) {
      console.warn('Failed to reassign:', e)
      setReassignError(e instanceof Error ? e.message : 'Failed to reassign')
    } finally {
      setReassignSaving(false)
    }
  }

  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <div className="max-w-[1600px] mx-auto animate-in fade-in duration-700">

        {/* Mission Statement */}
        <div className="bg-surface-container-lowest/70 backdrop-blur-md rounded-2xl p-xl mb-lg border border-outline-variant/30 relative shadow-sm">
          <div className="flex items-center justify-between mb-2">
            <div className="flex items-center gap-2 uppercase font-label-md text-[13px] tracking-widest text-on-surface-variant font-bold">
              <span className="w-1.5 h-1.5 bg-outline-variant rotate-45 block" />
              Strategic Focus
            </div>
            <button className="text-label-sm text-primary hover:underline cursor-pointer" onClick={() => setEditingFocus(!editingFocus)}>
              {editingFocus ? 'Cancel' : 'Edit'}
            </button>
          </div>
          {editingFocus ? (
            <div className="mt-2 space-y-md">
              <textarea
                className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
                value={focusText}
                onChange={e => setFocusText(e.target.value)}
              />
              <button className="px-md py-sm bg-primary text-on-primary rounded-lg font-label-md cursor-pointer hover:opacity-90" onClick={handleSaveFocus}>
                Save Focus
              </button>
            </div>
          ) : (
            <h2 className="font-headline-lg text-[28px] font-bold text-on-surface mt-2 max-w-5xl">
              {currentFocus}
            </h2>
          )}
        </div>

        {loading ? (
          <div className="grid grid-cols-1 md:grid-cols-3 gap-lg">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : (
        <>
        <OpcAnalyticsDashboard />
        <div className="flex flex-col lg:flex-row gap-lg items-start">

          {/* Agent Swarm Sidebar */}
          <div className="w-full lg:w-[320px] shrink-0 space-y-4">
            <div className="flex items-center gap-3">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant">Agent Swarm</h3>
              <span className="bg-secondary text-on-secondary text-[11px] font-bold px-2 py-0.5 rounded-full">{agents.length} Active</span>
              {/* C5: Spawn Agent button */}
              <button
                type="button"
                className="ml-auto flex items-center gap-1 text-[11px] font-bold text-primary hover:bg-primary/10 rounded-md px-2 py-1 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                onClick={() => setSpawnOpen(true)}
                aria-label="Spawn new agent"
              >
                <span className="material-symbols-outlined text-[14px]">add_circle</span>
                Spawn
              </button>
            </div>

            {agents.length === 0 ? (
              <div className="bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/20 rounded-xl p-lg">
                <EmptyState
                  icon="group"
                  title="No agents running."
                  description="Start a team coordination to see agents here."
                />
              </div>
            ) : (
              <div className="space-y-sm">
                {agents.map(agent => {
                  const isActive = agent.status === 'active' || agent.status === 'running'
                  const isMenuOpen = openMenuId === agent.id
                  return (
                    <div
                      key={agent.id}
                      role="button"
                      tabIndex={0}
                      aria-label={`${agent.name} — ${agent.status}${agent.worktree_path ? ` (${shortWorktree(agent.worktree_path)})` : ''}`}
                      className="bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/20 rounded-xl p-md flex flex-col shadow-sm cursor-pointer hover:border-primary/30 transition-colors group relative"
                      onClick={() => handleAgentClick(agent.id, agent.session_id)}
                      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); handleAgentClick(agent.id, agent.session_id) } }}
                    >
                      <div className="flex items-center justify-between mb-sm">
                        <div className="flex items-center gap-3">
                          <div className="w-10 h-10 rounded-lg bg-primary/10 flex items-center justify-center">
                            <span className="material-symbols-outlined text-[20px] text-on-surface-variant opacity-70">{iconForAgent(agent.name)}</span>
                          </div>
                          <div>
                            <div className="font-label-md text-[14px] font-bold">{agent.name}</div>
                            <div className="font-label-sm text-[11px] text-on-surface-variant">{agent.model || 'Default Model'}</div>
                          </div>
                        </div>
                        <div className="flex items-center gap-1">
                          <span className={`w-2 h-2 rounded-full shrink-0 ${isActive ? 'bg-tertiary animate-pulse' : 'bg-outline-variant'}`} />
                          {/* F6: ⋮ menu trigger */}
                          <button
                            type="button"
                            aria-label={`Actions for ${agent.name}`}
                            aria-haspopup="menu"
                            aria-expanded={isMenuOpen}
                            className="w-6 h-6 flex items-center justify-center rounded text-on-surface-variant hover:bg-surface-container-high/60 focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40 cursor-pointer"
                            onClick={e => { e.stopPropagation(); setOpenMenuId(isMenuOpen ? null : agent.id) }}
                          >
                            <span className="material-symbols-outlined text-[16px]">more_vert</span>
                          </button>
                        </div>
                      </div>
                      <div className="flex items-center gap-2">
                        <div className={`w-1 h-3 rounded-full shrink-0 ${isActive ? 'bg-tertiary' : 'bg-outline-variant'}`} />
                        <span className={`font-label-sm text-[12px] ${isActive ? 'text-tertiary' : 'text-on-surface-variant italic opacity-80'}`}>
                          {agent.task || agent.status}
                        </span>
                      </div>
                      {/* C1: worktree path label */}
                      {agent.worktree_path ? (
                        <div className="mt-sm flex items-center gap-1 font-label-sm text-[10px] text-on-surface-variant/80 bg-surface-container-low/60 rounded px-1.5 py-0.5 self-start">
                          <span className="material-symbols-outlined text-[12px]">fork_right</span>
                          <span className="font-mono truncate max-w-[200px]" title={agent.worktree_path}>{shortWorktree(agent.worktree_path)}</span>
                        </div>
                      ) : null}

                      {/* F6: dropdown menu */}
                      {isMenuOpen ? (
                        <div
                          ref={menuRef}
                          role="menu"
                          aria-label={`${agent.name} actions`}
                          className="absolute right-2 top-12 z-50 w-40 bg-surface-container-lowest border border-outline-variant/30 rounded-lg shadow-lg py-1 text-on-surface"
                          onClick={e => e.stopPropagation()}
                        >
                          <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleStopAgent(agent.id, agent.name)}>
                            <span className="material-symbols-outlined text-[14px] text-error">stop_circle</span> Stop
                          </button>
                          <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handlePauseAgent(agent.name)}>
                            <span className="material-symbols-outlined text-[14px]">pause_circle</span> Pause
                          </button>
                          <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleViewLogs(agent.id, agent.session_id)}>
                            <span className="material-symbols-outlined text-[14px]">description</span> View Logs
                          </button>
                          <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleReassignOpen({ id: agent.id, name: agent.name })}>
                            <span className="material-symbols-outlined text-[14px]">swap_horiz</span> Reassign
                          </button>
                        </div>
                      ) : null}
                    </div>
                  )
                })}
              </div>
            )}
          </div>

          {/* Kanban Board */}
          <div className="flex-1 w-full flex flex-col min-w-0">
            <div className="flex justify-between items-center mb-4">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant uppercase tracking-widest">KANBAN</h3>

              <div className="flex items-center gap-xs">
                <div className="relative">
                  <Input
                    type="text"
                    placeholder="Quick inject task..."
                    value={quickTask}
                    onChange={e => setQuickTask(e.target.value)}
                    className="bg-surface-container-low border-none rounded-lg py-1.5 pl-3 pr-8 w-[200px] text-[13px] font-body-md focus:ring-2 focus:ring-primary/20 transition-all outline-none"
                  />
                  <Button className="absolute right-1 top-1/2 -translate-y-1/2 w-6 h-6 bg-primary text-on-primary rounded-[4px] flex items-center justify-center hover:bg-primary/90 transition-colors" onClick={handleQuickTask}>
                    <span className="material-symbols-outlined text-[16px]">add</span>
                  </Button>
                </div>
              </div>
            </div>

            <div className="flex gap-4 overflow-x-auto pb-4 custom-scrollbar items-start min-h-[600px]">

              {/* To Do */}
              <KanbanColumn title="To Do" color="bg-secondary" count={todoTasks.length} onDrop={taskId => handleDrop(taskId, 'todo')}>
                {todoTasks.map(task => <KanbanCard key={task.id} task={task} draggable />)}
              </KanbanColumn>

              {/* Pending */}
              <KanbanColumn title="Pending" color="bg-secondary" count={pendingTasks.length} onDrop={taskId => handleDrop(taskId, 'pending')}>
                {pendingTasks.map(task => (
                  <div
                    key={task.id}
                    draggable
                    onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
                    className="bg-surface-container-lowest rounded-xl p-md border border-error/20 shadow-sm mb-3 ring-1 ring-error/5 cursor-grab active:cursor-grabbing hover:border-error/40 transition-colors relative"
                  >
                    <div className="absolute left-0 top-0 bottom-0 w-1 bg-error rounded-l-xl" />
                    <div className="flex justify-between items-start mb-2 ml-1">
                      <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
                      {task.priority === 'high' ? (
                        <span className="bg-error/10 text-error text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider">Critical</span>
                      ) : null}
                    </div>
                    <h4 className="font-label-md text-[15px] font-bold mb-2 leading-tight ml-1">{task.title}</h4>
                    {task.assignee ? (
                      <div className="flex justify-between items-center ml-1">
                        <span className="font-label-sm text-[11px] text-on-surface-variant">Assigned to <strong className="text-on-surface">{task.assignee}</strong></span>
                        <span className="font-label-sm text-[12px] text-primary font-bold">Review</span>
                      </div>
                    ) : null}
                  </div>
                ))}
              </KanbanColumn>

              {/* Doing */}
              <KanbanColumn title="Doing" color="bg-primary" count={inProgressTasks.length} onDrop={taskId => handleDrop(taskId, 'doing')}>
                {inProgressTasks.map(task => (
                  <div
                    key={task.id}
                    draggable
                    onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
                    className="bg-surface-container-lowest rounded-xl p-md border border-primary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:border-primary/50 transition-colors relative"
                  >
                    <div className="absolute left-0 top-0 bottom-0 w-1 bg-primary rounded-l-xl" />
                    <div className="flex justify-between items-center mb-2 ml-1">
                      <span className="font-label-sm text-[10px] font-bold text-primary tracking-wider">{task.id.slice(0, 8)}</span>
                      <span className="material-symbols-outlined text-[16px] text-primary">autorenew</span>
                    </div>
                    <h4 className="font-label-md text-[15px] font-bold mb-4 leading-tight ml-1">{task.title}</h4>
                    {task.assignee ? (
                      <div className="ml-1 mb-2">
                        <div className="h-1.5 w-full bg-surface-container rounded-full overflow-hidden mb-1">
                          {task.progress != null ? (
                            <div className="h-full bg-primary rounded-full transition-all duration-500" style={{ width: `${Math.min(100, Math.max(0, task.progress))}%` }} />
                          ) : (
                            <div className="h-full w-1/3 bg-primary/60 rounded-full animate-pulse" />
                          )}
                        </div>
                        <div className="flex justify-between items-center">
                          <span className="font-label-sm text-[10px] text-on-surface-variant">{task.assignee}</span>
                          <span className="font-label-sm text-[10px] font-bold text-on-surface-variant">{task.progress != null ? `${Math.min(100, Math.max(0, Math.round(task.progress)))}%` : 'In Progress'}</span>
                        </div>
                      </div>
                    ) : null}
                  </div>
                ))}
              </KanbanColumn>

              {/* Done */}
              <KanbanColumn title="Done" color="bg-tertiary" count={doneTasks.length} onDrop={taskId => handleDrop(taskId, 'done')}>
                {doneTasks.map(task => (
                  <Link
                    key={task.id}
                    to="/opc/task"
                    draggable
                    onDragStart={e => e.dataTransfer.setData('text/plain', task.id)}
                    className="block bg-surface-container-lowest rounded-xl p-3 border border-tertiary/20 shadow-sm mb-3 cursor-grab active:cursor-grabbing hover:bg-surface-bright transition-colors bg-tertiary/5"
                  >
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="material-symbols-outlined text-[16px] text-tertiary">check_circle</span>
                        <span className="font-label-md text-[13px] text-on-surface">{task.title}</span>
                      </div>
                      <span className="font-label-sm text-[10px] text-on-surface-variant">Done</span>
                    </div>
                  </Link>
                ))}
              </KanbanColumn>

              {/* Deprecated */}
              <KanbanColumn title="Deprecated" color="bg-outline-variant" count={deprecatedTasks.length} onDrop={taskId => handleDrop(taskId, 'deprecated')}>
                {deprecatedTasks.length === 0 ? (
                  <div className="flex items-center justify-center p-xl mt-xl">
                    <EmptyState icon="archive" title="No deprecated tasks." description="Completed or cancelled tasks will appear here." />
                  </div>
                ) : (
                  deprecatedTasks.map(task => <KanbanCard key={task.id} task={task} draggable />)
                )}
              </KanbanColumn>

            </div>
            {/* Hidden helper: trigger refresh button when overrides exist so users can reset */}
            {Object.keys(overrides).length > 0 ? (
              <div className="mt-sm flex justify-end">
                <button
                  className="text-label-sm text-on-surface-variant hover:text-primary cursor-pointer underline"
                  onClick={() => { setOverrides({}); void refreshTasks() }}
                >
                  Reset local overrides
                </button>
              </div>
            ) : null}
          </div>
        </div>
        </>
        )}

        {/* C5: Spawn Agent modal */}
        {spawnOpen ? (
          <div
            className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-md"
            role="dialog"
            aria-modal="true"
            aria-labelledby="spawn-agent-title"
            onClick={() => setSpawnOpen(false)}
          >
            <div
              className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-md p-xl space-y-md"
              onClick={e => e.stopPropagation()}
            >
              <div className="flex items-center justify-between">
                <h3 id="spawn-agent-title" className="font-headline-md text-[20px] font-bold flex items-center gap-2">
                  <span className="material-symbols-outlined text-primary">smart_toy</span>
                  Spawn New Agent
                </h3>
                <button
                  type="button"
                  className="text-on-surface-variant hover:text-on-surface cursor-pointer"
                  onClick={() => setSpawnOpen(false)}
                  aria-label="Close spawn agent dialog"
                >
                  <span className="material-symbols-outlined">close</span>
                </button>
              </div>

              <div className="space-y-sm">
                <label htmlFor="spawn-name" className="font-label-md text-on-surface-variant block">Name</label>
                <Input
                  id="spawn-name"
                  type="text"
                  placeholder="e.g. Research Agent"
                  value={spawnName}
                  onChange={e => setSpawnName(e.target.value)}
                  className="bg-surface-container-low"
                  autoFocus
                />
              </div>

              <div className="space-y-sm">
                <label htmlFor="spawn-model" className="font-label-md text-on-surface-variant block">Model (optional)</label>
                <Input
                  id="spawn-model"
                  type="text"
                  placeholder="default"
                  value={spawnModel}
                  onChange={e => setSpawnModel(e.target.value)}
                  className="bg-surface-container-low"
                />
              </div>

              <div className="space-y-sm">
                <label htmlFor="spawn-prompt" className="font-label-md text-on-surface-variant block">System Prompt</label>
                <textarea
                  id="spawn-prompt"
                  className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
                  placeholder="Describe the agent's role and capabilities..."
                  value={spawnPrompt}
                  onChange={e => setSpawnPrompt(e.target.value)}
                />
              </div>

              <div className="space-y-sm">
                <span className="font-label-md text-on-surface-variant block">Tools</span>
                <div className="flex flex-wrap gap-md">
                  {['bash', 'read', 'write', 'search', 'mcp'].map(tool => (
                    <label key={tool} className="flex items-center gap-xs font-label-md cursor-pointer">
                      <input
                        type="checkbox"
                        checked={!!spawnTools[tool]}
                        onChange={e => setSpawnTools(prev => ({ ...prev, [tool]: e.target.checked }))}
                        className="accent-primary"
                      />
                      {tool.charAt(0).toUpperCase() + tool.slice(1)}
                    </label>
                  ))}
                </div>
              </div>

              {spawnError ? (
                <p role="alert" className="text-error font-label-sm">{spawnError}</p>
              ) : null}

              <div className="flex gap-sm pt-sm">
                <Button
                  className="flex-1 bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
                  onClick={handleSpawnAgent}
                  disabled={spawnSaving}
                >
                  {spawnSaving ? 'Creating…' : 'Create Agent'}
                </Button>
                <Button
                  variant="ghost"
                  className="px-md rounded-lg border border-outline-variant font-label-md cursor-pointer"
                  onClick={() => { resetSpawn(); setSpawnOpen(false) }}
                  disabled={spawnSaving}
                >
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        ) : null}

        {/* F6: Reassign modal */}
        {reassignTarget ? (
          <div
            className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-md"
            role="dialog"
            aria-modal="true"
            aria-labelledby="reassign-title"
            onClick={() => setReassignTarget(null)}
          >
            <div
              className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-md p-xl space-y-md"
              onClick={e => e.stopPropagation()}
            >
              <div className="flex items-center justify-between">
                <h3 id="reassign-title" className="font-headline-md text-[20px] font-bold flex items-center gap-2">
                  <span className="material-symbols-outlined text-primary">swap_horiz</span>
                  Reassign Task
                </h3>
                <button
                  type="button"
                  className="text-on-surface-variant hover:text-on-surface cursor-pointer"
                  onClick={() => setReassignTarget(null)}
                  aria-label="Close reassign dialog"
                >
                  <span className="material-symbols-outlined">close</span>
                </button>
              </div>
              <p className="font-body-md text-on-surface-variant">
                Move <strong className="text-on-surface">{reassignTarget.taskTitle}</strong> off{' '}
                <strong className="text-on-surface">{reassignTarget.agentName}</strong>.
              </p>
              <div className="space-y-sm">
                <label htmlFor="reassign-pick" className="font-label-md text-on-surface-variant block">New assignee</label>
                <input
                  id="reassign-pick"
                  type="text"
                  list="reassign-agent-options"
                  placeholder="Pick or type an agent name"
                  value={reassignPick}
                  onChange={e => setReassignPick(e.target.value)}
                  className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30 w-full"
                  autoFocus
                />
                <datalist id="reassign-agent-options">
                  {agents
                    .map(a => a.name)
                    .filter(n => n !== reassignTarget.agentName)
                    .map(n => <option key={n} value={n} />)}
                </datalist>
              </div>
              {reassignError ? <p role="alert" className="text-error font-label-sm">{reassignError}</p> : null}
              <div className="flex gap-sm pt-sm">
                <Button
                  className="flex-1 bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
                  onClick={handleReassignSubmit}
                  disabled={reassignSaving || !reassignPick.trim()}
                >
                  {reassignSaving ? 'Reassigning…' : 'Reassign'}
                </Button>
                <Button
                  variant="ghost"
                  className="px-md rounded-lg border border-outline-variant font-label-md cursor-pointer"
                  onClick={() => setReassignTarget(null)}
                  disabled={reassignSaving}
                >
                  Cancel
                </Button>
              </div>
            </div>
          </div>
        ) : null}

      </div>
    </div>
  )
}

function KanbanColumn({
  title,
  color,
  count,
  children,
  onDrop,
}: {
  title: string
  color: string
  count: number
  children: React.ReactNode
  onDrop?: (taskId: string) => void
}) {
  const [isOver, setIsOver] = useState(false)
  return (
    <div
      className={`w-[300px] shrink-0 rounded-xl p-xs border transition-colors ${isOver ? 'bg-surface-container-high/40 border-primary/40' : 'bg-surface-container-lowest/50 border-transparent hover:bg-surface-container-low/30'}`}
      onDragOver={e => { e.preventDefault(); setIsOver(true) }}
      onDragLeave={() => setIsOver(false)}
      onDrop={e => {
        e.preventDefault()
        setIsOver(false)
        const taskId = e.dataTransfer.getData('text/plain')
        if (taskId && onDrop) onDrop(taskId)
      }}
    >
      <div className="flex justify-between items-center px-2 py-3 mb-1">
        <div className="flex items-center gap-2">
          <span className={`w-2 h-2 rounded-full ${color}`} />
          <span className="font-label-md text-[14px] font-bold">{title}</span>
        </div>
        <span className="font-label-sm text-[11px] text-on-surface-variant">{count}</span>
      </div>
      {children}
      {count === 0 && title !== 'Deprecated' ? (
        <div className="flex items-center justify-center p-xl mt-xl">
          <p className="font-label-sm text-[12px] text-on-surface-variant italic opacity-60">Empty</p>
        </div>
      ) : null}
    </div>
  )
}

function KanbanCard({ task, draggable }: { task: { id: string; title: string; description?: string; assignee?: string; priority?: string }; draggable?: boolean }) {
  return (
    <div
      draggable={draggable}
      onDragStart={draggable ? (e => e.dataTransfer.setData('text/plain', task.id)) : undefined}
      className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-pointer hover:border-primary/50 hover:shadow-md transition-all group/card focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none"
      tabIndex={0}
      role="button"
      onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); window.location.hash = `/opc/task/${task.id}` } }}
    >
      <div className="flex justify-between items-start mb-2">
        <span className="font-label-sm text-[10px] font-bold text-on-surface-variant tracking-wider">{task.id.slice(0, 8)}</span>
        {task.priority ? (
          <span className={`text-[10px] font-bold px-2 py-0.5 rounded uppercase tracking-wider ${
            task.priority === 'high' ? 'bg-error/10 text-error' : task.priority === 'medium' ? 'bg-secondary/10 text-secondary' : 'bg-surface-container text-on-surface-variant'
          }`}>{task.priority}</span>
        ) : null}
      </div>
      <h4 className="font-label-md text-[15px] font-bold mb-3 leading-tight group-hover/card:text-primary transition-colors">{task.title}</h4>
      {task.description ? <p className="font-body-sm text-[12px] text-on-surface-variant mb-2 leading-snug line-clamp-2">{task.description}</p> : null}
      <div className="flex justify-between items-center">
        {task.assignee ? (
          <span className="font-label-sm text-[11px] text-on-surface-variant">Proposed by <strong className="text-on-surface">{task.assignee}</strong></span>
        ) : null}
      </div>
    </div>
  )
}

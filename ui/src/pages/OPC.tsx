import { useState, useEffect } from 'react'
import { Link } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'

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

export default function OPC() {
  const { agents, tasks, config, loading } = useApp()
  const [quickTask, setQuickTask] = useState('')
  const [editingFocus, setEditingFocus] = useState(false)
  const [focusText, setFocusText] = useState('')

  const currentFocus = config?.strategic_focus
    || (config?.provider
      ? `${config.provider.charAt(0).toUpperCase() + config.provider.slice(1)} Agent Orchestration — autonomous task execution with multi-agent coordination.`
      : 'Autonomous task execution through multi-agent orchestration and intelligent coordination.')

  useEffect(() => { setFocusText(currentFocus) }, [currentFocus])

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

  const todoTasks = tasks.filter(t => t.status === 'pending' || t.status === 'todo')
  const pendingTasks = tasks.filter(t => t.status === 'review' || t.status === 'blocked')
  const inProgressTasks = tasks.filter(t => t.status === 'in_progress' || t.status === 'running')
  const doneTasks = tasks.filter(t => t.status === 'completed')
  const deprecatedTasks = tasks.filter(t => t.status === 'deprecated')

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
        <div className="flex flex-col lg:flex-row gap-lg items-start">

          {/* Agent Swarm Sidebar */}
          <div className="w-full lg:w-[320px] shrink-0 space-y-4">
            <div className="flex items-center gap-3">
              <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant">Agent Swarm</h3>
              <span className="bg-secondary text-on-secondary text-[11px] font-bold px-2 py-0.5 rounded-full">{agents.length} Active</span>
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
                  return (
                    <div key={agent.id} role="button" tabIndex={0} aria-label={`${agent.name} — ${agent.status}`} className="bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/20 rounded-xl p-md flex flex-col shadow-sm cursor-pointer hover:border-primary/30 transition-colors group" onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') e.currentTarget.click() }}>
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
                        <span className={`w-2 h-2 rounded-full shrink-0 ${isActive ? 'bg-tertiary animate-pulse' : 'bg-outline-variant'}`} />
                      </div>
                      <div className="flex items-center gap-2">
                        <div className={`w-1 h-3 rounded-full shrink-0 ${isActive ? 'bg-tertiary' : 'bg-outline-variant'}`} />
                        <span className={`font-label-sm text-[12px] ${isActive ? 'text-tertiary' : 'text-on-surface-variant italic opacity-80'}`}>
                          {agent.task || agent.status}
                        </span>
                      </div>
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
              <KanbanColumn title="To Do" color="bg-secondary" count={todoTasks.length}>
                {todoTasks.map(task => <KanbanCard key={task.id} task={task} />)}
              </KanbanColumn>

              {/* Pending */}
              <KanbanColumn title="Pending" color="bg-secondary" count={pendingTasks.length}>
                {pendingTasks.map(task => (
                  <div key={task.id} className="bg-surface-container-lowest rounded-xl p-md border border-error/20 shadow-sm mb-3 ring-1 ring-error/5 cursor-grab active:cursor-grabbing hover:border-error/40 transition-colors relative">
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
              <KanbanColumn title="Doing" color="bg-primary" count={inProgressTasks.length}>
                {inProgressTasks.map(task => (
                  <div key={task.id} className="bg-surface-container-lowest rounded-xl p-md border border-primary/20 shadow-sm mb-3 cursor-pointer hover:border-primary/50 transition-colors relative">
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
              <KanbanColumn title="Done" color="bg-tertiary" count={doneTasks.length}>
                {doneTasks.map(task => (
                  <Link key={task.id} to="/opc/task" className="block bg-surface-container-lowest rounded-xl p-3 border border-tertiary/20 shadow-sm mb-3 cursor-pointer hover:bg-surface-bright transition-colors bg-tertiary/5">
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
              <KanbanColumn title="Deprecated" color="bg-outline-variant" count={deprecatedTasks.length}>
                <div className="flex items-center justify-center p-xl mt-xl">
                  <EmptyState icon="archive" title="No deprecated tasks." description="Completed or cancelled tasks will appear here." />
                </div>
              </KanbanColumn>

            </div>
          </div>
        </div>
        )}

      </div>
    </div>
  )
}

function KanbanColumn({ title, color, count, children }: { title: string; color: string; count: number; children: React.ReactNode }) {
  return (
    <div className="w-[300px] shrink-0 bg-surface-container-lowest/50 rounded-xl p-xs border border-transparent hover:bg-surface-container-low/30 transition-colors">
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

function KanbanCard({ task }: { task: { id: string; title: string; description?: string; assignee?: string; priority?: string } }) {
  return (
    <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/30 shadow-sm mb-3 cursor-pointer hover:border-primary/50 hover:shadow-md transition-all group/card focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none" tabIndex={0} role="button" onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); window.location.hash = `/opc/task/${task.id}` } }}>
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

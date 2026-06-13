import { useApp } from '@/context/AppContext'
import { useParams, Link } from 'react-router-dom'
import { useState } from 'react'
import { toast } from 'sonner'
import AgentMessagesPanel from '@/components/tasks/AgentMessagesPanel'
import AgentLoadPanel from '@/components/tasks/AgentLoadPanel'

export default function OPCTask() {
  const { tasks, agents, usage, respondPermission } = useApp()
  const [revisionNote, setRevisionNote] = useState('')
  const [showRevisionInput, setShowRevisionInput] = useState<string | null>(null)
  const { id } = useParams()

  // Find the task by URL param, or the first in-progress task
  const task = (id ? tasks.find(t => t.id === id) : null) ?? tasks.find(t => t.status === 'in_progress' || t.status === 'running')
  const hasRunningTasks = tasks.some(t => t.status === 'in_progress' || t.status === 'running' || t.status === 'pending')
  const taskId = task?.id ?? ''

  return (
    <div className="flex-1 w-full bg-background overflow-y-auto h-full px-lg py-xl">
      <nav aria-label="Breadcrumb" className="flex items-center gap-xs text-label-sm text-on-surface-variant mb-lg">
        <Link to="/opc" className="hover:text-primary transition-colors">OPC Board</Link>
        <span className="material-symbols-outlined text-[14px]">chevron_right</span>
        <span className="text-on-surface">Task Detail</span>
      </nav>
      <div className="max-w-[1400px] mx-auto">
        <div className="grid grid-cols-1 xl:grid-cols-12 gap-lg pb-10">
          {/* Left Column */}
          <div className="xl:col-span-8 flex flex-col gap-lg">
            {/* Agent Workflow */}
            <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
              <div className="flex items-center gap-2 mb-8">
                <span className="material-symbols-outlined text-[20px] text-on-surface">account_tree</span>
                <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Agent Workflow</h3>
              </div>

              {agents.length === 0 ? (
                <p className="text-body-sm text-on-surface-variant text-center py-lg">No agents in this workflow. Start team coordination to see agent pipeline.</p>
              ) : (
                <>
                  <div className="relative flex items-center justify-between mb-10 px-4 md:px-10 overflow-x-auto">
                    <div className="absolute left-10 md:left-16 right-10 md:right-16 top-6 h-0.5 bg-outline-variant/20 z-0" />
                    {agents.map((agent) => {
                      const isActive = agent.status === 'active' || agent.status === 'running'
                      return (
                        <div key={agent.id} className="relative z-10 flex flex-col items-center gap-2 shrink-0">
                          <div className={`w-12 h-12 rounded-full flex items-center justify-center shrink-0 ${
                            isActive ? 'bg-primary/10' : 'border border-outline-variant bg-surface-container-lowest'
                          }`}>
                            {isActive ? (
                              <div className="w-12 h-12 rounded-full bg-primary text-on-primary flex items-center justify-center shadow-md">
                                <span className="material-symbols-outlined text-[20px]">smart_toy</span>
                              </div>
                            ) : (
                              <span className="material-symbols-outlined text-[20px] text-on-surface-variant">smart_toy</span>
                            )}
                          </div>
                          <span className={`font-label-sm text-[12px] ${isActive ? 'text-primary font-bold' : 'text-on-surface-variant'}`}>{agent.name}</span>
                        </div>
                      )
                    })}
                  </div>
                </>
              )}
            </div>

            {/* Task Description */}
            {task ? (
              <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
                <div className="flex items-center gap-2 mb-6">
                  <span className="material-symbols-outlined text-[20px] text-on-surface">description</span>
                  <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Task Description</h3>
                </div>
                <div className="space-y-sm">
                  <h4 className="font-body-lg font-bold text-on-surface">{task.title}</h4>
                  <p className="font-body-md text-on-surface-variant">{task.description ?? 'No description provided.'}</p>
                  <div className="flex items-center gap-md mt-md">
                    <span className={`px-sm py-xs rounded-full font-label-sm text-[11px] font-bold uppercase tracking-wider ${
                      task.status === 'completed' ? 'bg-tertiary/10 text-tertiary' :
                      task.status === 'running' || task.status === 'in_progress' ? 'bg-primary/10 text-primary' :
                      task.status === 'failed' ? 'bg-error/10 text-error' :
                      'bg-surface-container text-on-surface-variant'
                    }`}>{task.status}</span>
                    {task.assignee ? <span className="font-label-sm text-on-surface-variant">Assigned to: {task.assignee}</span> : null}
                    {task.priority ? <span className="font-label-sm text-on-surface-variant">Priority: {task.priority}</span> : null}
                  </div>
                </div>
              </div>
            ) : (
              <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm text-center">
                <span className="material-symbols-outlined text-[48px] text-outline-variant">task_alt</span>
                <p className="font-body-md text-on-surface-variant mt-md">No task selected. Navigate from the OPC board or Tasks page.</p>
              </div>
            )}

            {/* Execution Log */}
            <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
              <div className="flex items-center justify-between mb-8">
                <div className="flex items-center gap-2">
                  <span className="material-symbols-outlined text-[20px] text-on-surface">receipt_long</span>
                  <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Execution Log</h3>
                </div>
                <span className="bg-surface-container-low text-on-surface-variant font-label-sm text-[11px] px-3 py-1 rounded-full border border-outline-variant/20">{agents.length} Agents</span>
              </div>

              {agents.length === 0 ? (
                <p className="text-body-sm text-on-surface-variant text-center py-lg">No execution events yet.</p>
              ) : (
                <div className="relative pl-0 md:pl-2 space-y-10">
                  <div className="absolute left-[15px] md:left-[23px] top-4 bottom-8 w-px bg-outline-variant/30" />
                  {agents.map((agent) => {
                    const isActive = agent.status === 'active' || agent.status === 'running'
                    return (
                    <div key={agent.id} className="relative flex items-start gap-4">
                      <div className={`w-8 h-8 rounded-full flex items-center justify-center shrink-0 relative z-10 md:ml-2 ${
                        isActive ? 'bg-primary text-on-primary shadow-sm ring-4 ring-primary/10' : 'border-2 border-outline-variant/40 bg-surface-container-lowest text-on-surface-variant'
                      }`}>
                        <span className="material-symbols-outlined text-[16px]">smart_toy</span>
                      </div>
                      <div className="flex-1 -mt-1">
                        <div className="flex justify-between items-start mb-1">
                          <h4 className={`font-label-md text-[14px] ${isActive ? 'text-primary font-bold' : 'text-on-surface'}`}>{agent.name}</h4>
                          <span className={`font-label-sm text-[10px] uppercase tracking-wider ${isActive ? 'text-primary font-bold' : 'text-on-surface-variant'}`}>{agent.status}</span>
                        </div>
                        {agent.task ? <p className="text-body-sm text-[14px] mt-1 text-on-surface-variant">{agent.task}</p> : null}
                      </div>
                    </div>
                    )
                  })}
                </div>
              )}
            </div>

            {/* Agent Messages (Phase D C3) */}
            <AgentMessagesPanel limit={50} />

            {/* Human-in-the-Loop Review */}
            {hasRunningTasks && (
              <div className="glass-card bg-surface-container-lowest/80 rounded-2xl p-xl border border-outline-variant/40 shadow-sm">
                <div className="flex items-center gap-2 mb-6">
                  <span className="material-symbols-outlined text-[20px] text-on-surface-variant">verified_user</span>
                  <h3 className="font-headline-md text-[20px] font-bold text-on-surface">Human-in-the-Loop Review</h3>
                  <span className="ml-auto w-2.5 h-2.5 rounded-full bg-primary animate-pulse" />
                </div>

                <p className="text-body-sm text-on-surface-variant mb-lg">
                  The following actions require your review before proceeding. Approve to continue execution, or request adjustments.
                </p>

                <div className="flex flex-col gap-sm">
                  <button
                    className="w-full px-md py-sm rounded-xl bg-primary text-on-primary font-label-md hover:brightness-110 transition-all flex items-center justify-center gap-sm"
                    onClick={() => { respondPermission(taskId, true); toast.success('Approved — execution will continue') }}
                  >
                    <span className="material-symbols-outlined text-[18px]">check_circle</span>
                    Approve Final Merge
                  </button>
                  <button
                    className="w-full px-md py-sm rounded-xl border border-error/30 text-error font-label-md hover:bg-error/10 transition-all flex items-center justify-center gap-sm"
                    onClick={() => { respondPermission(taskId, false); toast.info('Rollback requested') }}
                  >
                    <span className="material-symbols-outlined text-[18px]">undo</span>
                    Rollback
                  </button>
                  <button
                    className="w-full px-md py-sm rounded-xl border border-outline-variant/50 text-on-surface-variant font-label-md hover:bg-surface-container-high/60 transition-all flex items-center justify-center gap-sm"
                    onClick={() => setShowRevisionInput(showRevisionInput === taskId ? null : taskId)}
                  >
                    <span className="material-symbols-outlined text-[18px]">rate_review</span>
                    Request Revision
                  </button>

                  {showRevisionInput === taskId && (
                    <div className="mt-sm flex flex-col gap-sm">
                      <textarea
                        className="w-full px-md py-sm rounded-xl border border-outline-variant/50 bg-surface-container-lowest text-body-sm text-on-surface resize-none focus:outline-none focus:border-primary transition-colors"
                        rows={3}
                        placeholder="Describe what needs to change (e.g., 'Refactor to use async/await', 'Add error handling for edge cases')..."
                        value={revisionNote}
                        onChange={e => setRevisionNote(e.target.value)}
                      />
                      <button
                        className="self-end px-md py-xs rounded-lg bg-primary/10 text-primary font-label-sm hover:bg-primary/20 transition-colors"
                        onClick={() => {
                          respondPermission(taskId, false, revisionNote || undefined)
                          toast.success('Revision request submitted')
                          setRevisionNote('')
                          setShowRevisionInput(null)
                        }}
                      >
                        Submit Revision Request
                      </button>
                    </div>
                  )}
                </div>
              </div>
            )}
          </div>

          {/* Right Column */}
          <div className="xl:col-span-4 flex flex-col gap-lg">
            {/* Agent Load (Phase D G10) */}
            <AgentLoadPanel agents={agents} />

            {/* Efficiency Metrics */}
            <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm flex flex-col gap-md">
              <div className="flex items-center gap-2 mb-2">
                <span className="material-symbols-outlined text-[20px] text-primary">monitoring</span>
                <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Efficiency Metrics</h3>
              </div>

              {/* Task Completion Rate */}
              {agents.length > 0 && (
                <div className="bg-primary/5 rounded-xl p-md border border-primary/10">
                  <div className="flex items-center justify-between mb-sm">
                    <span className="font-label-sm text-on-surface-variant uppercase tracking-wider">Task Completion</span>
                    <span className="font-headline-md text-primary font-bold">{tasks.length > 0 ? Math.round((tasks.filter(t => t.status === 'completed').length / tasks.length) * 100) : 0}%</span>
                  </div>
                  <div className="w-full h-2 bg-primary/10 rounded-full overflow-hidden">
                    <div className="h-full bg-gradient-to-r from-primary/60 to-primary rounded-full transition-all duration-700" style={{ width: `${tasks.length > 0 ? Math.round((tasks.filter(t => t.status === 'completed').length / tasks.length) * 100) : 0}%` }} />
                  </div>
                </div>
              )}

              <div className="grid grid-cols-2 gap-sm">
                <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                  <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Session Cost</div>
                  <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">${(usage?.cost_usd ?? 0).toFixed(4)}</div>
                </div>
                <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                  <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Token Usage</div>
                  <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">{((usage?.input_tokens ?? 0) + (usage?.output_tokens ?? 0)).toLocaleString()}</div>
                </div>
                <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                  <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Agents</div>
                  <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">{agents.length}</div>
                </div>
                <div className="bg-surface-container-lowest rounded-xl p-md border border-outline-variant/20">
                  <div className="font-label-sm text-[10px] text-on-surface-variant uppercase tracking-wider mb-2">Tasks</div>
                  <div className="font-headline-md text-[18px] font-bold text-on-surface mb-1">{tasks.length}</div>
                </div>
              </div>
            </div>

            {/* Active Tasks */}
            <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm flex flex-col gap-md">
              <div className="flex items-center gap-2 mb-2">
                <span className="material-symbols-outlined text-[20px] text-primary">inventory_2</span>
                <h3 className="font-headline-md text-[18px] font-bold text-on-surface">Related Tasks</h3>
              </div>
              {tasks.slice(0, 5).map(t => (
                <Link key={t.id} to={`/opc/task/${t.id}`} className="border border-outline-variant/30 rounded-xl p-md flex items-start gap-md hover:border-primary/40 hover:bg-surface-container-lowest transition-colors cursor-pointer group focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none">
                  <div className="w-10 h-10 rounded-lg bg-surface-container flex items-center justify-center shrink-0 text-on-surface-variant group-hover:text-primary group-hover:bg-primary/10 transition-colors">
                    <span className="material-symbols-outlined text-[20px]">task_alt</span>
                  </div>
                  <div>
                    <div className="font-label-md text-[14px] font-bold text-on-surface mb-0.5 group-hover:text-primary transition-colors">{t.title}</div>
                    <span className={`font-label-sm text-[11px] ${t.status === 'completed' ? 'text-tertiary' : t.status === 'in_progress' || t.status === 'running' ? 'text-primary' : 'text-on-surface-variant'}`}>{t.status}</span>
                  </div>
                </Link>
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

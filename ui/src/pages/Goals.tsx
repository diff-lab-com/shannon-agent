import { useState } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { useApp } from '@/context/AppContext'

export default function Goals() {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const { tasks, agents, loading, respondPermission, sendMessage } = useApp()
  const [searchQuery, setSearchQuery] = useState('')
  const [goalInput, setGoalInput] = useState('')
  const [aiMenuOpen, setAiMenuOpen] = useState(false)

  // Group tasks by status to create a goal-like view
  const activeTasks = tasks.filter(t => t.status === 'in_progress' || t.status === 'running')
  const completedTasks = tasks.filter(t => t.status === 'completed')
  const pendingTasks = tasks.filter(t => t.status === 'pending' || t.status === 'todo')

  // Apply search filter
  const query = searchQuery.toLowerCase()
  const filteredActive = query ? activeTasks.filter(t => t.title.toLowerCase().includes(query)) : activeTasks
  const filteredPending = query ? pendingTasks.filter(t => t.title.toLowerCase().includes(query)) : pendingTasks
  const filteredCompleted = query ? completedTasks.filter(t => t.title.toLowerCase().includes(query)) : completedTasks

  return (
    <div className="flex-1 flex w-full h-full pb-10">
      {/* Sidebar */}
      <aside className="hidden md:flex w-[320px] h-full border-r border-outline-variant/20 bg-surface-container-low/30 flex-col overflow-hidden shrink-0">
        <div className="p-md border-b border-outline-variant/20">
          <div className="relative">
            <span className="material-symbols-outlined absolute left-3 top-1/2 -translate-y-1/2 text-on-surface-variant/60 text-[20px]">search</span>
            <Input className="w-full pl-10 pr-4 py-2 bg-surface-container-lowest border border-outline-variant/50 rounded-lg text-body-sm focus:outline-none focus:border-primary transition-all outline-none" placeholder={t('goals.searchTasks')} type="text" value={searchQuery} onChange={e => setSearchQuery(e.target.value)} />
          </div>
        </div>
        <div className="flex-1 overflow-y-auto py-sm">
          <div className="px-md py-xs">
            <p className="font-label-sm text-on-surface-variant/60 uppercase tracking-wider">{t('goals.activeTasks', { count: filteredActive.length })}</p>
          </div>
          <div className="px-sm space-y-1">
            {filteredActive.length === 0 && filteredPending.length === 0 ? (
              <div className="text-center py-lg opacity-70">
                <span className="material-symbols-outlined text-on-surface-variant text-[28px]">task_alt</span>
                <p className="text-body-sm text-on-surface-variant mt-xs">{t('goals.noTasks')}</p>
              </div>
            ) : null}
            {filteredActive.map(task => {
              const progress = task.progress ?? Math.round((task.status === 'completed' ? 100 : task.status === 'in_progress' || task.status === 'running' ? 0 : 0))
              return (
              <div key={task.id} className="w-full flex flex-col gap-1 p-md rounded-xl bg-primary/10 border border-primary/20 cursor-pointer">
                <div className="flex justify-between items-start">
                  <span className="font-label-md text-primary font-bold truncate">{task.title}</span>
                  <span className="font-label-sm text-primary font-bold">{progress}%</span>
                </div>
                <div className="w-full h-1.5 bg-primary/10 rounded-full overflow-hidden">
                  <div className="h-full bg-primary rounded-full transition-all duration-500" style={{ width: `${progress}%` }} />
                </div>
                {task.assignee ? <span className="font-label-sm text-on-surface-variant">{t('goals.assignedTo', { assignee: task.assignee })}</span> : null}
              </div>
              )
            })}
            {filteredPending.map(task => (
              <div key={task.id} className="w-full flex flex-col gap-1 p-md rounded-xl hover:bg-surface-container-high/60 hover:shadow-sm hover:-translate-y-0.5 transition-all cursor-pointer duration-300">
                <div className="flex justify-between items-start">
                  <span className="font-label-md text-on-surface truncate">{task.title}</span>
                  <span className="font-label-sm text-on-surface-variant">{t('goals.pending')}</span>
                </div>
                {task.assignee ? <span className="font-label-sm text-on-surface-variant">{t('goals.assignedTo', { assignee: task.assignee })}</span> : null}
              </div>
            ))}
          </div>

          {filteredCompleted.length > 0 ? (
            <>
              <div className="px-md py-xs mt-lg">
                <p className="font-label-sm text-on-surface-variant/60 uppercase tracking-wider">{t('goals.completed', { count: filteredCompleted.length })}</p>
              </div>
              <div className="px-sm space-y-1">
                {filteredCompleted.map(task => (
                  <div key={task.id} className="w-full flex items-center gap-sm p-md rounded-xl opacity-60">
                    <span className="material-symbols-outlined text-tertiary text-[16px]">check_circle</span>
                    <span className="font-label-md text-on-surface truncate">{task.title}</span>
                  </div>
                ))}
              </div>
            </>
          ) : null}
        </div>
      </aside>

      {/* Main Canvas */}
      <div className="flex-1 flex flex-col overflow-y-auto p-xl pb-32 relative">
        {loading ? (
          <div className="flex-1 p-lg space-y-md">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : (
        <>
        <div className="flex items-end justify-between mb-xl">
          <div>
            <div className="flex items-center gap-sm mb-xs">
              <span className="px-sm py-xs bg-primary/10 text-primary font-label-sm rounded-full">{t('goals.tasksOverview')}</span>
              <span className="font-label-sm text-on-surface-variant/60">{t('goals.totalTasks', { count: tasks.length })}</span>
            </div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface">{t('goals.taskManagement')}</h2>
          </div>
        </div>

        {/* Agent Call Path */}
        <div className="flex gap-gutter w-full">
          <div className="w-1/4 max-w-[280px]">
            <div className="glass-card bg-surface-container-lowest/70 p-md rounded-xl sticky top-0">
              <h3 className="font-label-md text-on-surface mb-md flex items-center gap-sm">
                <span className="material-symbols-outlined text-primary text-[20px]">hub</span>
                {t('goals.activeAgents', { count: agents.length })}
              </h3>
              {agents.length === 0 ? (
                <div className="text-center py-md opacity-70">
                  <span className="material-symbols-outlined text-on-surface-variant text-[24px]">smart_toy</span>
                  <p className="text-body-sm text-on-surface-variant mt-xs">{t('goals.noAgentsActive')}</p>
                </div>
              ) : (
                <div className="space-y-lg relative">
                  <div className="absolute left-[15px] top-6 bottom-6 w-px border-l border-dashed border-primary/30" />
                  {agents.map(agent => (
                    <div key={agent.id} className="relative flex items-center gap-md">
                      <div className="z-10 w-8 h-8 rounded-full bg-primary text-on-primary flex items-center justify-center text-[18px]">
                        <span className="material-symbols-outlined">smart_toy</span>
                      </div>
                      <div>
                        <p className="font-label-md text-on-surface">{agent.name}</p>
                        <p className="font-label-sm text-on-surface-variant/70">{agent.status}</p>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          </div>

          {/* Task Tree */}
          <div className="flex-1 space-y-md">
            {tasks.length === 0 ? (
              <EmptyState
                icon="task_alt"
                title={t('goals.noTasksYet')}
                description={t('goals.noTasksDescription')}
              />
            ) : (
              tasks.map(task => {
                const isActive = task.status === 'in_progress' || task.status === 'running'
                const isDone = task.status === 'completed'
                return (
                  <div key={task.id} className={`flex items-start gap-lg ${!isActive && !isDone ? 'opacity-60 grayscale-[0.5]' : ''}`}>
                    <div className="mt-4 flex flex-col items-center">
                      <div className={`w-4 h-4 rounded-full z-10 ${isDone ? 'border-2 border-tertiary bg-background' : isActive ? 'border-2 border-primary bg-primary shadow-lg' : 'border-2 border-outline-variant bg-surface-container-highest'}`} />
                    </div>
                    <div className={`flex-1 glass-card p-lg rounded-xl transition-all ${isActive ? 'bg-surface-container-lowest border-primary/30 ring-1 ring-primary/10 shadow-lg' : isDone ? 'bg-surface-container-lowest/70' : 'bg-surface-container-lowest/50'}`}>
                      <div className="flex items-center gap-md mb-xs">
                        <h4 className={`font-headline-md ${isActive ? 'text-primary' : isDone ? 'text-on-surface' : 'text-on-surface-variant'}`}>{task.title}</h4>
                        <span className={`px-sm py-xs font-label-sm rounded-lg flex items-center gap-1 ${
                          isDone ? 'bg-tertiary/10 text-tertiary' : isActive ? 'bg-primary/10 text-primary' : 'bg-surface-container-high text-on-surface-variant'
                        }`}>
                          <span className="material-symbols-outlined text-[14px]">{isDone ? 'check_circle' : isActive ? 'sync' : 'lock'}</span>
                          {isDone ? t('goals.done') : isActive ? t('goals.inProgress') : t('goals.pending')}
                        </span>
                      </div>
                      {task.description ? <p className="text-on-surface-variant">{task.description}</p> : null}
                      <div className="flex items-center gap-md mt-sm">
                        <div className="flex-1 h-1.5 bg-outline-variant/10 rounded-full overflow-hidden">
                          <div
                            className={`h-full rounded-full transition-all duration-500 ${isDone ? 'bg-tertiary' : isActive ? 'bg-primary' : 'bg-outline-variant/30'}`}
                            style={{ width: `${isDone ? 100 : isActive ? (task.progress ?? 0) : 0}%` }}
                          />
                        </div>
                        <span className={`font-label-sm text-[11px] ${isDone ? 'text-tertiary' : isActive ? 'text-primary' : 'text-on-surface-variant'}`}>
                          {isDone ? '100%' : isActive ? `${task.progress ?? 0}%` : '0%'}
                        </span>
                      </div>
                      {task.assignee ? <p className="text-label-sm text-on-surface-variant mt-sm">{t('goals.assignedTo', { assignee: task.assignee })}</p> : null}
                    </div>
                  </div>
                )
              })
            )}
          </div>
        </div>

        {/* Agent Reasoning - Human-in-the-Loop Approval */}
        {activeTasks.length > 0 && (
          <div className="mt-xl">
            <div className="glass-card bg-surface-container-lowest/80 p-lg rounded-xl border-primary/20">
              <div className="flex items-center gap-sm mb-lg">
                <span className="material-symbols-outlined text-primary text-[22px]">smart_toy</span>
                <h3 className="font-headline-md text-on-surface">{t('goals.agentReasoning')}</h3>
                <span className="ml-auto px-sm py-xs bg-primary/10 text-primary font-label-sm rounded-full">{t('goals.activeCount', { count: activeTasks.length })}</span>
              </div>
              <div className="space-y-lg">
                {activeTasks.map(task => (
                  <div key={task.id} className="glass-card bg-surface-container-lowest/70 p-md rounded-xl">
                    <div className="flex items-center justify-between mb-sm">
                      <div className="flex items-center gap-sm">
                        <div className="w-3 h-3 rounded-full bg-primary animate-pulse" />
                        <span className="font-label-md text-on-surface font-bold">{task.title}</span>
                      </div>
                      <span className="px-sm py-xs bg-primary/10 text-primary font-label-sm rounded-lg flex items-center gap-1">
                        <span className="material-symbols-outlined text-[12px]">sync</span>
                        {task.status === 'in_progress' ? t('goals.inProgress') : t('goals.running')}
                      </span>
                    </div>

                    {/* Step visualization */}
                    <div className="flex items-start gap-md my-md pl-sm">
                      <div className="flex flex-col items-center">
                        <div className="w-6 h-6 rounded-full bg-primary/20 flex items-center justify-center">
                          <span className="material-symbols-outlined text-primary text-[14px]">psychology</span>
                        </div>
                        <div className="node-connector w-px flex-1 min-h-[24px]" />
                        <div className="w-6 h-6 rounded-full bg-surface-container-high flex items-center justify-center">
                          <span className="material-symbols-outlined text-on-surface-variant text-[14px]">pending</span>
                        </div>
                      </div>
                      <div className="flex flex-col gap-sm pt-xs">
                        <span className="font-label-sm text-on-surface-variant">{t('goals.analyzingRequirements')}</span>
                        <span className="font-label-sm text-on-surface-variant">{t('goals.awaitingApproval')}</span>
                      </div>
                    </div>

                    {/* Action buttons */}
                    <div className="flex gap-sm mt-md">
                      <button
                        className="px-md py-sm rounded-lg bg-primary/10 text-primary font-label-md hover:bg-primary/20 transition-colors flex items-center gap-1"
                        onClick={() => { respondPermission(task.id, true); toast.success(t('goals.approved')) }}
                      >
                        <span className="material-symbols-outlined text-[16px]">check</span>
                        {t('goals.approve')}
                      </button>
                      <button
                        className="px-md py-sm rounded-lg border border-outline-variant/50 text-on-surface-variant font-label-md hover:bg-surface-container-high/60 transition-colors flex items-center gap-1"
                        onClick={() => { respondPermission(task.id, false); toast.info(t('goals.adjustmentsRequested')) }}
                      >
                        <span className="material-symbols-outlined text-[16px]">tune</span>
                        {t('goals.adjust')}
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        )}
        </>
        )}
      </div>

      {/* Right Sidebar */}
      <aside className="hidden xl:flex w-[300px] border-l border-outline-variant/20 bg-surface-container-low/30 p-lg shrink-0 flex-col gap-lg">
        <div className="glass-card bg-surface-container-lowest/70 p-lg rounded-xl">
          <h5 className="font-label-md text-on-surface-variant mb-md">{t('goals.taskSummary')}</h5>
          <div className="space-y-sm">
            <div className="flex justify-between"><span className="text-body-sm text-on-surface-variant">{t('goals.active')}</span><span className="font-label-md text-primary font-bold">{activeTasks.length}</span></div>
            <div className="flex justify-between"><span className="text-body-sm text-on-surface-variant">{t('goals.pending')}</span><span className="font-label-md text-on-surface font-bold">{pendingTasks.length}</span></div>
            <div className="flex justify-between"><span className="text-body-sm text-on-surface-variant">{t('goals.completed')}</span><span className="font-label-md text-tertiary font-bold">{completedTasks.length}</span></div>
          </div>
        </div>
      </aside>

      {/* Sticky Bottom Input */}
      <div className="absolute bottom-0 left-0 md:left-[320px] right-0 xl:right-[300px] px-lg py-md bg-gradient-to-t from-background via-background/90 to-transparent">
        <div className="glass-card bg-surface-container-lowest/80 rounded-2xl border border-outline-variant/30 px-sm py-xs flex items-center shadow-lg">
          <span className="material-symbols-outlined p-md text-primary" aria-hidden="true">auto_awesome</span>
          <input
            aria-label={t('goals.askAboutGoals')}
            className="flex-1 bg-transparent border-none outline-none font-body-md text-on-surface placeholder:text-outline-variant/80"
            placeholder={t('goals.askAboutGoal')}
            type="text"
            value={goalInput}
            onChange={e => setGoalInput(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter' && goalInput.trim()) { sendMessage(goalInput); setGoalInput('') } }}
          />
          <div className="relative">
            <Button variant="ghost" aria-label={t('goals.aiAssistant')} className="p-md text-primary" onClick={() => setAiMenuOpen(!aiMenuOpen)}>
              <span className="material-symbols-outlined text-[20px]" aria-hidden="true">auto_awesome</span>
            </Button>
            {aiMenuOpen && (
              <div className="absolute bottom-full right-0 mb-sm w-[220px] bg-surface-container-lowest rounded-xl border border-outline-variant/20 shadow-xl py-xs z-50">
                {[
                  { label: t('goals.suggestNextSteps'), icon: 'lightbulb', prompt: 'Suggest next steps for my current tasks based on their progress' },
                  { label: t('goals.summarizeProgress'), icon: 'summarize', prompt: 'Summarize the progress of all my current tasks' },
                  { label: t('goals.identifyRisks'), icon: 'warning', prompt: 'Identify potential risks and blockers in my current tasks' },
                ].map(opt => (
                  <button key={opt.label} className="w-full text-left px-md py-sm hover:bg-primary/5 flex items-center gap-sm text-on-surface font-label-md transition-colors" onClick={() => { sendMessage(opt.prompt); setGoalInput(''); setAiMenuOpen(false) }}>
                    <span className="material-symbols-outlined text-[16px] text-primary">{opt.icon}</span>
                    {opt.label}
                  </button>
                ))}
              </div>
            )}
          </div>
          <Button aria-label={t('goals.sendMessage')} className="bg-primary text-on-primary p-2 rounded-xl hover:shadow-md hover:shadow-primary/30 transition-all" disabled={!goalInput.trim()} onClick={() => { if (goalInput.trim()) { sendMessage(goalInput); setGoalInput('') } }}>
            <span className="material-symbols-outlined text-[20px]" aria-hidden="true">arrow_upward</span>
          </Button>
        </div>
      </div>
    </div>
  )
}

import { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { useCatalog } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import { toastError } from '@/lib/errorToast'

export default function MyAgents() {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const { agents, backgroundTasks, models } = useCatalog()
  const navigate = useNavigate()
  const [configuring, setConfiguring] = useState<string | null>(null)
  const [showAddAgent, setShowAddAgent] = useState(false)
  const [agentName, setAgentName] = useState('')
  const [agentModel, setAgentModel] = useState('')
  const [agentPrompt, setAgentPrompt] = useState('')
  const [agentTools, setAgentTools] = useState<Record<string, boolean>>({ bash: true, read: true, write: true })
  const [showMenu, setShowMenu] = useState<string | null>(null)
  const [nameError, setNameError] = useState('')
  const menuRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!showMenu) return
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setShowMenu(null)
    }
    document.addEventListener('click', handleClick)
    return () => document.removeEventListener('click', handleClick)
  }, [showMenu])

  const statusFor = (status: string) => {
    switch (status) {
      case 'active': case 'running': return { color: 'bg-tertiary animate-pulse', bg: 'bg-primary/10 text-primary', label: t('extensions.myAgents.statusActive') }
      case 'idle': return { color: 'bg-outline', bg: 'bg-surface-container-high text-on-surface-variant', label: t('extensions.myAgents.statusIdle') }
      case 'error': return { color: 'bg-error', bg: 'bg-error/10 text-error', label: t('extensions.myAgents.statusError') }
      default: return { color: 'bg-outline', bg: 'bg-surface-container-high text-on-surface-variant', label: status }
    }
  }

  const iconFor = (name: string) => {
    const n = name.toLowerCase()
    if (n.includes('research')) return 'query_stats'
    if (n.includes('cod')) return 'code'
    if (n.includes('plan') || n.includes('sched')) return 'schedule'
    if (n.includes('test')) return 'bug_report'
    if (n.includes('design')) return 'palette'
    if (n.includes('data')) return 'database'
    return 'smart_toy'
  }

  const iconBgFor = (name: string) => {
    const n = name.toLowerCase()
    if (n.includes('research')) return 'bg-primary-container/20 text-primary'
    if (n.includes('cod') || n.includes('dev')) return 'bg-secondary-container/20 text-secondary'
    if (n.includes('plan') || n.includes('pa')) return 'bg-tertiary-container/20 text-tertiary'
    return 'bg-primary-container/20 text-primary'
  }

  const completedTasks = backgroundTasks.filter(t => t.status === 'completed').length
  const totalTasks = backgroundTasks.length

  return (
    <div className="max-w-[1200px] mx-auto px-lg py-xl">
      {/* Page Header */}
      <div className="flex flex-col md:flex-row md:items-center justify-between gap-lg mb-xl">
        <div>
          <h2 className="text-headline-lg font-headline-lg text-on-surface">{t('extensions.myAgents.title')}</h2>
          <p className="text-body-md text-on-surface-variant">{t('extensions.myAgents.subtitle')}</p>
        </div>
        <div className="flex items-center gap-md">
          <span className="font-label-md text-on-surface-variant">{intl.formatMessage({ id: 'extensions.myAgents.count' }, { count: agents.length })}</span>
        </div>
      </div>

      {/* Agents Bento Grid */}
      {agents.length === 0 && !showAddAgent ? (
        <EmptyState
          icon="smart_toy"
          title={t('extensions.myAgents.emptyTitle')}
          description={t('extensions.myAgents.emptyDesc')}
          action={{ label: t('extensions.myAgents.createFirstCta'), onClick: () => setShowAddAgent(true) }}
        />
      ) : (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-lg">
            {agents.map(agent => {
              const st = statusFor(agent.status)
              return (
                <div key={agent.id} className="glass-card p-lg rounded-xl shadow-sm flex flex-col group hover:-translate-y-1 transition-transform duration-300">
                  <div className="flex justify-between items-start mb-md">
                    <div className={`w-12 h-12 rounded-xl flex items-center justify-center ${iconBgFor(agent.name)}`}>
                      <span className="material-symbols-outlined text-[28px]" style={{fontVariationSettings: "'FILL' 1"}}>{iconFor(agent.name)}</span>
                    </div>
                    <div className={`flex items-center gap-xs px-sm py-1 rounded-full ${st.bg}`}>
                      <span className={`w-2 h-2 rounded-full ${st.color}`} />
                      <span className="text-label-sm">{st.label}</span>
                    </div>
                  </div>

                  <div className="mb-lg">
                    <h3 className="text-headline-md font-headline-md">{agent.name}</h3>
                    <p className="text-label-sm text-on-surface-variant">{intl.formatMessage({ id: 'extensions.myAgents.modelAutonomous' }, { model: (agent.model || t('extensions.myAgents.defaultModel')) })}</p>
                  </div>

                  <div className="space-y-sm mb-lg">
                    {agent.task ? (
                      <div className="flex justify-between items-center text-label-md">
                        <span className="text-on-surface-variant">{t('extensions.myAgents.currentTask')}</span>
                        <span className="font-bold truncate max-w-[140px]">{agent.task}</span>
                      </div>
                    ) : null}
                    {agent.progress != null ? (
                      <div className="flex justify-between items-center text-label-md">
                        <span className="text-on-surface-variant">{t('extensions.myAgents.progress')}</span>
                        <div className="flex items-center gap-sm">
                          <div className="w-16 h-1.5 bg-surface-container rounded-full overflow-hidden">
                            <div className="h-full bg-primary rounded-full" style={{ width: `${agent.progress}%` }} />
                          </div>
                          <span className="font-bold">{agent.progress}%</span>
                        </div>
                      </div>
                    ) : null}
                    {agent.tools_used != null ? (
                      <div className="flex justify-between items-center text-label-md">
                        <span className="text-on-surface-variant">{t('extensions.myAgents.toolsUsed')}</span>
                        <span className="font-bold">{agent.tools_used}</span>
                      </div>
                    ) : null}
                    {agent.duration != null ? (
                      <div className="flex justify-between items-center text-label-md">
                        <span className="text-on-surface-variant">{t('extensions.myAgents.duration')}</span>
                        <span className="font-bold">{agent.duration > 60000 ? `${(agent.duration / 60000).toFixed(1)}m` : `${(agent.duration / 1000).toFixed(0)}s`}</span>
                      </div>
                    ) : null}
                  </div>

                  <div className="mt-auto pt-md border-t border-outline-variant flex gap-sm">
                    <Button variant="ghost" className="flex-grow py-2 rounded-lg bg-surface-variant/50 font-bold text-label-md hover:bg-surface-variant transition-colors cursor-pointer" onClick={() => setConfiguring(configuring === agent.id ? null : agent.id)}>
                      {configuring === agent.id ? t('extensions.myAgents.close') : t('extensions.myAgents.configure')}
                    </Button>
                    <Button variant="ghost" className="p-2 rounded-lg border border-outline-variant hover:text-primary transition-colors cursor-pointer flex items-center justify-center relative" onClick={() => setShowMenu(showMenu === agent.id ? null : agent.id)}>
                      <span className="material-symbols-outlined">more_horiz</span>
                      {showMenu === agent.id && (
                        <div ref={menuRef} className="absolute right-0 top-full mt-1 bg-surface-container-lowest border border-outline-variant/30 rounded-lg shadow-lg py-xs z-10 min-w-[140px]">
                          <button className="w-full text-left px-md py-sm text-label-md hover:bg-surface-container-high transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" onClick={() => { setConfiguring(configuring === agent.id ? null : agent.id); setShowMenu(null) }}>{t('extensions.myAgents.viewStatus')}</button>
                          <button className="w-full text-left px-md py-sm text-label-md hover:bg-surface-container-high transition-colors text-error focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30" onClick={async () => { setShowMenu(null); try { await api.cancelBackgroundTask(agent.id); toast.success(intl.formatMessage({ id: 'extensions.myAgents.stopped' }, { name: agent.name })) } catch (e) { toastError(t('extensions.myAgents.stopFailed'), e) } }}>{t('extensions.myAgents.stopAgent')}</button>
                        </div>
                      )}
                    </Button>
                  </div>
                  {configuring === agent.id && (
                    <div className="mt-sm p-sm bg-surface-container-low rounded-lg text-label-sm text-on-surface-variant">
                      <p>{intl.formatMessage({ id: 'extensions.myAgents.configuration' }, { name: agent.name })}</p>
                      <p className="mt-xs opacity-70">{intl.formatMessage({ id: 'extensions.myAgents.modelLabel' }, { model: (agent.model || t('extensions.myAgents.default')) })}</p>
                    </div>
                  )}
                </div>
              )
            })}

            {/* Add New Agent */}
            {!showAddAgent ? (
              <div className="border-2 border-dashed border-outline-variant p-lg rounded-xl flex flex-col items-center justify-center text-center group cursor-pointer hover:border-primary/50 transition-colors" onClick={() => setShowAddAgent(true)}>
                <div className="w-12 h-12 rounded-full bg-surface-container flex items-center justify-center text-on-surface-variant group-hover:bg-primary-container/20 group-hover:text-primary transition-colors mb-md">
                  <span className="material-symbols-outlined icon-xl">add</span>
                </div>
                <h3 className="text-body-lg font-bold">{t('extensions.myAgents.newSpecialization')}</h3>
                <p className="text-label-md text-on-surface-variant max-w-[200px]">{t('extensions.myAgents.newSpecializationDesc')}</p>
              </div>
            ) : (
              <div className="border-2 border-primary/30 p-lg rounded-xl flex flex-col gap-md">
                <h3 className="text-body-lg font-bold">{t('extensions.myAgents.createTitle')}</h3>
                <div className="space-y-sm">
                  <label className="text-label-md text-on-surface-variant">{t('extensions.myAgents.nameLabel')}</label>
                  <input
                    className={`w-full p-sm bg-surface-container-low rounded-lg border text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30 ${nameError ? 'border-error' : 'border-outline-variant/30'}`}
                    placeholder={t('extensions.myAgents.namePlaceholder')}
                    value={agentName}
                    onChange={e => { setAgentName(e.target.value); setNameError('') }}
                  />
                  {nameError ? <p className="text-error text-label-sm">{nameError}</p> : null}
                </div>
                <div className="space-y-sm">
                  <label className="text-label-md text-on-surface-variant">{t('extensions.myAgents.modelLabelField')}</label>
                  <select
                    className="w-full p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30"
                    value={agentModel}
                    onChange={e => setAgentModel(e.target.value)}
                  >
                    <option value="">{t('extensions.myAgents.defaultModel')}</option>
                    {models.map(m => (
                      <option key={m.id} value={m.id}>{m.name} ({m.provider})</option>
                    ))}
                  </select>
                </div>
                <div className="space-y-sm">
                  <label className="text-label-md text-on-surface-variant">{t('extensions.myAgents.systemPrompt')}</label>
                  <textarea
                    className="w-full h-24 p-sm bg-surface-container-low rounded-lg border border-outline-variant/30 text-body-sm resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
                    placeholder={t('extensions.myAgents.promptPlaceholder')}
                    value={agentPrompt}
                    onChange={e => setAgentPrompt(e.target.value)}
                  />
                </div>
                <div className="space-y-sm">
                  <label className="text-label-md text-on-surface-variant">{t('extensions.myAgents.tools')}</label>
                  <div className="flex flex-wrap gap-md">
                    {['bash', 'read', 'write', 'search', 'mcp'].map(tool => (
                      <label key={tool} className="flex items-center gap-xs text-label-md cursor-pointer">
                        <input
                          type="checkbox"
                          checked={!!agentTools[tool]}
                          onChange={e => setAgentTools(prev => ({ ...prev, [tool]: e.target.checked }))}
                          className="accent-primary"
                        />
                        {tool.charAt(0).toUpperCase() + tool.slice(1)}
                      </label>
                    ))}
                  </div>
                </div>
                <div className="flex gap-sm">
                  <Button className="flex-1 py-2 bg-primary text-on-primary rounded-lg font-label-md cursor-pointer" onClick={async () => {
                    if (!agentName.trim()) { setNameError(t('extensions.myAgents.nameRequired')); return }
                    const tools = Object.entries(agentTools).filter(([, v]) => v).map(([k]) => k)
                    try {
                      await api.createAgentDefinition(agentName.trim(), agentModel || undefined, agentPrompt || undefined, tools)
                      toast.success(intl.formatMessage({ id: 'extensions.myAgents.created' }, { name: agentName }))
                      setAgentName(''); setAgentModel(''); setAgentPrompt(''); setAgentTools({ bash: true, read: true, write: true }); setNameError('')
                      setShowAddAgent(false)
                    } catch (e) {
                      console.warn('Failed to create agent:', e)
                      toast.error(e instanceof Error ? e.message : t('extensions.myAgents.createFailed'))
                    }
                  }}>
                    {t('extensions.myAgents.createButton')}
                  </Button>
                  <Button variant="ghost" className="py-2 px-md rounded-lg border border-outline-variant font-label-md cursor-pointer" onClick={() => {
                    setShowAddAgent(false); setAgentName(''); setAgentModel(''); setAgentPrompt(''); setNameError('')
                  }}>
                    {t('extensions.myAgents.cancel')}
                  </Button>
                </div>
              </div>
            )}
          </div>

          {/* Performance Section */}
          <section className="mt-xl grid grid-cols-1 lg:grid-cols-3 gap-lg mb-8">
            <div className="lg:col-span-2 glass-card p-xl rounded-xl">
              <h4 className="text-body-lg font-bold mb-lg flex items-center gap-md">
                <span className="material-symbols-outlined text-primary">insights</span>
                {t('extensions.myAgents.performance')}
              </h4>

              {backgroundTasks.length === 0 ? (
                <div className="h-48 flex items-center justify-center text-on-surface-variant opacity-60">
                  <p className="text-body-sm">{t('extensions.myAgents.noPerfData')}</p>
                </div>
              ) : (
                <div className="space-y-sm">
                  {backgroundTasks.slice(0, 5).map(bt => (
                    <div key={bt.task_id} className="flex items-center gap-md">
                      <span className="font-label-sm text-on-surface-variant w-24 truncate">{bt.prompt.slice(0, 20)}</span>
                      <div className="flex-grow h-4 bg-surface-container-low rounded overflow-hidden">
                        <div
                          className={`h-full rounded ${bt.status === 'completed' ? 'bg-primary' : bt.status === 'running' ? 'bg-primary/60 animate-pulse' : 'bg-error/60'}`}
                          style={{ width: bt.status === 'completed' ? '100%' : bt.status === 'running' ? '60%' : '30%' }}
                        />
                      </div>
                      <span className={`font-label-sm font-bold w-16 text-right ${bt.status === 'completed' ? 'text-primary' : bt.status === 'running' ? 'text-primary/60' : 'text-error'}`}>
                        {bt.status === 'completed' ? t('extensions.myAgents.done') : bt.status === 'running' ? t('extensions.myAgents.active') : t('extensions.myAgents.failed')}
                      </span>
                    </div>
                  ))}
                </div>
              )}
            </div>

            <div className="glass-card p-xl rounded-xl">
              <h4 className="text-body-lg font-bold mb-lg">{t('extensions.myAgents.taskCompletion')}</h4>
              <div className="text-center py-lg">
                <div className="text-display-lg text-[48px] text-primary font-bold">{totalTasks > 0 ? Math.round((completedTasks / totalTasks) * 100) : 0}%</div>
                <p className="text-on-surface-variant text-body-sm mt-sm">{intl.formatMessage({ id: 'extensions.myAgents.tasksCompleted' }, { completed: completedTasks, total: totalTasks })}</p>
              </div>
              <div className="mt-lg h-2 bg-surface-container-high rounded-full overflow-hidden">
                <div className="h-full bg-primary rounded-full" style={{ width: totalTasks > 0 ? `${(completedTasks / totalTasks) * 100}%` : '0%' }} />
              </div>
              <Button variant="ghost" className="w-full mt-lg text-primary text-label-md font-bold hover:underline cursor-pointer text-left" onClick={() => navigate('/tasks')}>{t('extensions.myAgents.viewAllTasks')}</Button>
            </div>
          </section>
        </>
      )}
    </div>
  )
}

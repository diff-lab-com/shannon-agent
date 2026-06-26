import { useState, useEffect, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import EmptyState from '@/components/ui/empty-state'
import { Input } from '@/components/ui/input'
import { useApp } from '@/context/AppContext'
import { useModalFocus } from '@/hooks/useModalFocus'
import * as api from '@/lib/tauri-api'
import type { AgentInfo, TaskItem } from '@/types'

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

// Shorten a worktree path to its tail for compact display: "/Users/x/worktrees/foo" → "/foo"
function shortWorktree(p: string): string {
  const trimmed = p.replace(/\/+$/, '')
  const slash = trimmed.lastIndexOf('/')
  return slash >= 0 ? trimmed.slice(slash) : trimmed
}

interface Props {
  agents: AgentInfo[]
  tasks: TaskItem[]
}

export default function OPCAgentSwarm({ agents, tasks }: Props) {
  const intl = useIntl()
  const navigate = useNavigate()
  const { switchSession } = useApp()
  const [openMenuId, setOpenMenuId] = useState<string | null>(null)
  const [spawnOpen, setSpawnOpen] = useState(false)
  const [reassignTarget, setReassignTarget] = useState<{ agentId: string; agentName: string; taskId: string; taskTitle: string } | null>(null)
  const menuRef = useRef<HTMLDivElement | null>(null)

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

  const handleAgentClick = async (agentId: string, sessionId?: string) => {
    setOpenMenuId(null)
    if (sessionId) {
      try {
        await switchSession(sessionId)
      } catch (e) {
        console.warn('switchSession failed, falling back to task detail:', e)
      }
      navigate(`/chat?agent=${agentId}`)
      return
    }
    navigate(`/opc/task?agent=${agentId}`)
  }

  const handleStopAgent = async (agentId: string, name: string) => {
    setOpenMenuId(null)
    try {
      await api.cancelBackgroundTask(agentId)
      toast.success(intl.formatMessage({ id: 'opc.agentSwarm.stopSuccess' }, { name }))
    } catch (e) {
      console.warn('Failed to stop agent:', e)
      toast.error(intl.formatMessage({ id: 'opc.agentSwarm.stopFailed' }, { name }))
    }
  }

  const handleViewLogs = (agentId: string, sessionId?: string) => {
    setOpenMenuId(null)
    const target = sessionId ? `/opc/task?agent=${agentId}&session=${sessionId}` : `/opc/task?agent=${agentId}`
    navigate(target)
  }

  const handlePauseAgent = (name: string) => {
    setOpenMenuId(null)
    toast.info(intl.formatMessage({ id: 'opc.agentSwarm.pauseNotSupported' }, { name }), {
      description: intl.formatMessage({ id: 'opc.agentSwarm.pauseDesc' })
    })
  }

  const handleReassignOpen = (agent: { id: string; name: string }) => {
    setOpenMenuId(null)
    const owned = tasks.find(t => t.assignee === agent.name && (t.status === 'in_progress' || t.status === 'running' || t.status === 'pending'))
    if (!owned) {
      toast.info(intl.formatMessage({ id: 'opc.agentSwarm.noActiveTask' }, { name: agent.name }))
      return
    }
    setReassignTarget({ agentId: agent.id, agentName: agent.name, taskId: owned.id, taskTitle: owned.title })
  }

  return (
    <div className="w-full lg:w-[320px] shrink-0 space-y-4">
      <div className="flex items-center gap-3">
        <h3 className="font-label-md text-[14px] font-bold text-on-surface-variant">{intl.formatMessage({ id: 'opc.agentSwarm.activeAgents' })}</h3>
        <span className="bg-secondary text-on-secondary text-[11px] font-bold px-2 py-0.5 rounded-full">{agents.length} {intl.formatMessage({ id: 'opc.agentSwarm.active' })}</span>
        <button
          type="button"
          className="ml-auto flex items-center gap-1 text-[11px] font-bold text-primary hover:bg-primary/10 rounded-md px-2 py-1 transition-colors cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
          onClick={() => setSpawnOpen(true)}
          aria-label={intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.aria' })}
        >
          <span className="material-symbols-outlined text-[14px]">add_circle</span>
          {intl.formatMessage({ id: 'opc.agentSwarm.spawn' })}
        </button>
      </div>

      {agents.length === 0 ? (
        <div className="bg-surface-container-lowest/70 backdrop-blur-md border border-outline-variant/20 rounded-xl p-lg">
          <EmptyState
            icon="group"
            title={intl.formatMessage({ id: 'opc.agentSwarm.noAgentsRunning' })}
            description={intl.formatMessage({ id: 'opc.agentSwarm.startTeamCoordination' })}
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
                      <div className="font-label-sm text-[11px] text-on-surface-variant">{agent.model || intl.formatMessage({ id: 'opc.agentSwarm.defaultModel' })}</div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1">
                    <span className={`w-2 h-2 rounded-full shrink-0 ${isActive ? 'bg-tertiary animate-pulse' : 'bg-outline-variant'}`} />
                    <button
                      type="button"
                      aria-label={intl.formatMessage({ id: 'opc.agentSwarm.actions.name' }, { name: agent.name })}
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
                {agent.worktree_path ? (
                  <div className="mt-sm flex items-center gap-1 font-label-sm text-[10px] text-on-surface-variant/80 bg-surface-container-low/60 rounded px-1.5 py-0.5 self-start">
                    <span className="material-symbols-outlined text-[12px]">fork_right</span>
                    <span className="font-mono truncate max-w-[200px]" title={agent.worktree_path}>{shortWorktree(agent.worktree_path)}</span>
                  </div>
                ) : null}

                {isMenuOpen ? (
                  <div
                    ref={menuRef}
                    role="menu"
                    aria-label={`${agent.name} actions`}
                    className="absolute right-2 top-12 z-50 w-40 bg-surface-container-lowest border border-outline-variant/30 rounded-lg shadow-lg py-1 text-on-surface"
                    onClick={e => e.stopPropagation()}
                  >
                    <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleStopAgent(agent.id, agent.name)}>
                      <span className="material-symbols-outlined text-[14px] text-error">stop_circle</span> {intl.formatMessage({ id: 'opc.agentSwarm.actions.stop' })}
                    </button>
                    <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handlePauseAgent(agent.name)}>
                      <span className="material-symbols-outlined text-[14px]">pause_circle</span> {intl.formatMessage({ id: 'opc.agentSwarm.actions.pause' })}
                    </button>
                    <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleViewLogs(agent.id, agent.session_id)}>
                      <span className="material-symbols-outlined text-[14px]">description</span> {intl.formatMessage({ id: 'opc.agentSwarm.actions.viewLogs' })}
                    </button>
                    <button role="menuitem" className="w-full text-left px-3 py-1.5 text-[12px] hover:bg-surface-container-high/60 flex items-center gap-2 cursor-pointer" onClick={() => handleReassignOpen({ id: agent.id, name: agent.name })}>
                      <span className="material-symbols-outlined text-[14px]">swap_horiz</span> {intl.formatMessage({ id: 'opc.agentSwarm.actions.reassign' })}
                    </button>
                  </div>
                ) : null}
              </div>
            )
          })}
        </div>
      )}

      <SpawnAgentModal open={spawnOpen} onClose={() => setSpawnOpen(false)} />
      <ReassignModal
        target={reassignTarget}
        agents={agents}
        onClose={() => setReassignTarget(null)}
        onSubmitted={() => setReassignTarget(null)}
      />
    </div>
  )
}

function SpawnAgentModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const intl = useIntl()
  const [name, setName] = useState('')
  const [model, setModel] = useState('')
  const [prompt, setPrompt] = useState('')
  const [tools, setTools] = useState<Record<string, boolean>>({ bash: true, read: true, write: true })
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)

  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(open, containerRef)

  const reset = () => {
    setName(''); setModel(''); setPrompt('')
    setTools({ bash: true, read: true, write: true })
    setError(null)
  }

  const submit = async () => {
    if (!name.trim()) {
      setError(intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.agentNameRequired' }))
      return
    }
    setSaving(true); setError(null)
    const toolList = Object.entries(tools).filter(([, v]) => v).map(([k]) => k)
    try {
      await api.createAgentDefinition(name.trim(), model || undefined, prompt || undefined, toolList)
      toast.success(intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.agentCreated' }, { name: name.trim() }))
      reset(); onClose()
    } catch (e) {
      console.warn('Failed to create agent:', e)
      setError(e instanceof Error ? e.message : intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.createFailed' }))
    } finally {
      setSaving(false)
    }
  }

  if (!open) return null

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="spawn-agent-title"
      onClick={onClose}
    >
      <div
        className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-md p-xl space-y-md"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 id="spawn-agent-title" className="font-headline-md text-[20px] font-bold flex items-center gap-2">
            <span className="material-symbols-outlined text-primary">smart_toy</span>
            {intl.formatMessage({ id: 'opc.agentSwarm.spawnNewAgent' })}
          </h3>
          <button
            type="button"
            className="text-on-surface-variant hover:text-on-surface cursor-pointer"
            onClick={onClose}
            aria-label={intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.close.aria' })}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>

        <div className="space-y-sm">
          <label htmlFor="spawn-name" className="font-label-md text-on-surface-variant block">{intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.name' })}</label>
          <Input
            id="spawn-name"
            type="text"
            placeholder={intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.name.placeholder' })}
            value={name}
            onChange={e => setName(e.target.value)}
            className="bg-surface-container-low"
            autoFocus
          />
        </div>

        <div className="space-y-sm">
          <label htmlFor="spawn-model" className="font-label-md text-on-surface-variant block">{intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.model' })}</label>
          <Input
            id="spawn-model"
            type="text"
            placeholder={intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.model.placeholder' })}
            value={model}
            onChange={e => setModel(e.target.value)}
            className="bg-surface-container-low"
          />
        </div>

        <div className="space-y-sm">
          <label htmlFor="spawn-prompt" className="font-label-md text-on-surface-variant block">{intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.systemPrompt' })}</label>
          <textarea
            id="spawn-prompt"
            className="w-full h-24 p-md bg-surface-container-low rounded-xl border border-outline-variant/30 text-body-md resize-none focus:outline-none focus:ring-2 focus:ring-primary/30"
            placeholder={intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.systemPrompt.placeholder' })}
            value={prompt}
            onChange={e => setPrompt(e.target.value)}
          />
        </div>

        <div className="space-y-sm">
          <span className="font-label-md text-on-surface-variant block">{intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.tools' })}</span>
          <div className="flex flex-wrap gap-md">
            {['bash', 'read', 'write', 'search', 'mcp'].map(tool => (
              <label key={tool} className="flex items-center gap-xs font-label-md cursor-pointer">
                <input
                  type="checkbox"
                  checked={!!tools[tool]}
                  onChange={e => setTools(prev => ({ ...prev, [tool]: e.target.checked }))}
                  className="accent-primary"
                />
                {tool.charAt(0).toUpperCase() + tool.slice(1)}
              </label>
            ))}
          </div>
        </div>

        {error ? <p role="alert" className="text-error font-label-sm">{error}</p> : null}

        <div className="flex gap-sm pt-sm">
          <Button
            className="flex-1 bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
            onClick={submit}
            disabled={saving}
          >
            {saving ? intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.creating' }) : intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.createAgent' })}
          </Button>
          <Button
            variant="ghost"
            className="px-md rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={() => { reset(); onClose() }}
            disabled={saving}
          >
            {intl.formatMessage({ id: 'opc.agentSwarm.spawnAgent.cancel' })}
          </Button>
        </div>
      </div>
    </div>
  )
}

function ReassignModal({
  target,
  agents,
  onClose,
  onSubmitted,
}: {
  target: { agentId: string; agentName: string; taskId: string; taskTitle: string } | null
  agents: AgentInfo[]
  onClose: () => void
  onSubmitted: () => void
}) {
  const intl = useIntl()
  const [pick, setPick] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const containerRef = useRef<HTMLDivElement>(null)
  useModalFocus(!!target, containerRef)

  useEffect(() => {
    if (target) { setPick(''); setError(null) }
  }, [target])

  if (!target) return null

  const submit = async () => {
    const trimmed = pick.trim()
    if (!trimmed) {
      setError(intl.formatMessage({ id: 'opc.agentSwarm.reassign.pickRequired' }))
      return
    }
    setSaving(true); setError(null)
    try {
      await api.updateTask({ id: target.taskId, assignee: trimmed })
      toast.success(intl.formatMessage({ id: 'opc.agentSwarm.reassign.reassigned' }, { taskTitle: target.taskTitle, name: trimmed }))
      onSubmitted()
    } catch (e) {
      console.warn('Failed to reassign:', e)
      setError(e instanceof Error ? e.message : intl.formatMessage({ id: 'opc.agentSwarm.reassign.failed' }))
    } finally {
      setSaving(false)
    }
  }

  return (
    <div
      ref={containerRef}
      className="fixed inset-0 z-[200] flex items-center justify-center bg-black/30 backdrop-blur-sm p-md"
      role="dialog"
      aria-modal="true"
      aria-labelledby="reassign-title"
      onClick={onClose}
    >
      <div
        className="bg-surface-container-lowest rounded-2xl shadow-2xl border border-outline-variant/30 w-full max-w-md p-xl space-y-md"
        onClick={e => e.stopPropagation()}
      >
        <div className="flex items-center justify-between">
          <h3 id="reassign-title" className="font-headline-md text-[20px] font-bold flex items-center gap-2">
            <span className="material-symbols-outlined text-primary">swap_horiz</span>
            {intl.formatMessage({ id: 'opc.agentSwarm.reassign.title' })}
          </h3>
          <button
            type="button"
            className="text-on-surface-variant hover:text-on-surface cursor-pointer"
            onClick={onClose}
            aria-label={intl.formatMessage({ id: 'opc.agentSwarm.reassign.close.aria' })}
          >
            <span className="material-symbols-outlined">close</span>
          </button>
        </div>
        <p className="font-body-md text-on-surface-variant">
          {intl.formatMessage({ id: 'opc.agentSwarm.reassign.moveOff' }, {
            taskTitle: <strong className="text-on-surface">{target.taskTitle}</strong>,
            agentName: <strong className="text-on-surface">{target.agentName}</strong>
          })}
        </p>
        <div className="space-y-sm">
          <label htmlFor="reassign-pick" className="font-label-md text-on-surface-variant block">{intl.formatMessage({ id: 'opc.agentSwarm.reassign.newAssignee' })}</label>
          <input
            id="reassign-pick"
            type="text"
            list="reassign-agent-options"
            placeholder={intl.formatMessage({ id: 'opc.agentSwarm.reassign.pickAgent' })}
            value={pick}
            onChange={e => setPick(e.target.value)}
            className="bg-surface-container-low rounded-lg border border-outline-variant/30 px-sm py-sm text-body-sm focus:outline-none focus:ring-2 focus:ring-primary/30 w-full"
            autoFocus
          />
          <datalist id="reassign-agent-options">
            {agents
              .map(a => a.name)
              .filter(n => n !== target.agentName)
              .map(n => <option key={n} value={n} />)}
          </datalist>
        </div>
        {error ? <p role="alert" className="text-error font-label-sm">{error}</p> : null}
        <div className="flex gap-sm pt-sm">
          <Button
            className="flex-1 bg-primary text-on-primary rounded-lg font-label-md cursor-pointer disabled:opacity-50"
            onClick={submit}
            disabled={saving || !pick.trim()}
          >
            {saving ? intl.formatMessage({ id: 'opc.agentSwarm.reassign.reassigning' }) : intl.formatMessage({ id: 'opc.agentSwarm.reassign.reassign' })}
          </Button>
          <Button
            variant="ghost"
            className="px-md rounded-lg border border-outline-variant font-label-md cursor-pointer"
            onClick={onClose}
            disabled={saving}
          >
            {intl.formatMessage({ id: 'opc.agentSwarm.reassign.cancel' })}
          </Button>
        </div>
      </div>
    </div>
  )
}

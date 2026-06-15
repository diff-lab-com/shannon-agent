import { useState, useEffect, useRef, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

interface PaletteItem {
  id: string
  label: string
  icon: string
  category: string
  action: () => void
}

export default function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const navigate = useNavigate()
  const { sessions, models, tasks, agents } = useApp()

  const items = useMemo<PaletteItem[]>(() => {
    const actions: PaletteItem[] = [
      { id: 'a-new-chat', label: 'Start new chat', icon: 'add_comment', category: 'Actions', action: () => navigate('/chat') },
      { id: 'a-new-task', label: 'Create scheduled task', icon: 'add_task', category: 'Actions', action: () => navigate('/tasks') },
      { id: 'a-new-routine', label: 'Create routine', icon: 'schedule', category: 'Actions', action: () => navigate('/routines') },
      { id: 'a-new-agent', label: 'Browse agents', icon: 'smart_toy', category: 'Actions', action: () => navigate('/agents') },
      { id: 'a-toggle-theme', label: 'Change theme', icon: 'palette', category: 'Actions', action: () => navigate('/settings/theme') },
    ]
    const pages: PaletteItem[] = [
      { id: 'p-chat', label: 'Chat', icon: 'chat_bubble', category: 'Pages', action: () => navigate('/chat') },
      { id: 'p-today', label: 'Today', icon: 'today', category: 'Pages', action: () => navigate('/today') },
      { id: 'p-tasks', label: 'Scheduled', icon: 'task_alt', category: 'Pages', action: () => navigate('/tasks') },
      { id: 'p-conversations', label: 'Conversations', icon: 'forum', category: 'Pages', action: () => navigate('/conversations') },
      { id: 'p-ext', label: 'Extensions Hub', icon: 'grid_view', category: 'Pages', action: () => navigate('/extensions') },
      { id: 'p-routines', label: 'Routines', icon: 'schedule', category: 'Pages', action: () => navigate('/routines') },
      { id: 'p-hooks', label: 'Hooks', icon: 'webhook', category: 'Pages', action: () => navigate('/hooks') },
      { id: 'p-profiles', label: 'Permission Profiles', icon: 'shield', category: 'Pages', action: () => navigate('/profiles') },
      { id: 'p-editor', label: 'Code Editor', icon: 'code', category: 'Pages', action: () => navigate('/editor') },
      { id: 'p-perf', label: 'Performance', icon: 'speed', category: 'Pages', action: () => navigate('/perf') },
      { id: 'p-set', label: 'Settings', icon: 'settings', category: 'Pages', action: () => navigate('/settings') },
      { id: 'p-theme', label: 'Theme Settings', icon: 'palette', category: 'Settings', action: () => navigate('/settings/theme') },
      { id: 'p-models', label: 'Model Settings', icon: 'neurology', category: 'Settings', action: () => navigate('/settings/models') },
      { id: 'p-billing', label: 'Usage & Billing', icon: 'credit_card', category: 'Settings', action: () => navigate('/settings/billing') },
    ]
    const taskItems: PaletteItem[] = tasks.slice(0, 8).map(t => ({
      id: `t-${t.id}`,
      label: t.title,
      icon: t.status === 'completed' ? 'task_alt' : t.status === 'in_progress' ? 'pending' : 'radio_button_unchecked',
      category: 'Tasks',
      action: () => navigate('/tasks'),
    }))
    const agentItems: PaletteItem[] = agents.slice(0, 5).map(a => ({
      id: `ag-${a.id}`,
      label: a.name,
      icon: 'smart_toy',
      category: 'Agents',
      action: () => navigate('/agents'),
    }))
    const sessionItems: PaletteItem[] = sessions.slice(0, 10).map(s => ({
      id: `s-${s.id}`, label: s.title || 'Untitled', icon: 'history', category: 'Recent chats', action: () => navigate('/chat'),
    }))
    const modelItems: PaletteItem[] = models.slice(0, 5).map(m => ({
      id: `m-${m.id}`, label: m.name, icon: 'neurology', category: 'Switch model', action: () => {
        api.switchProvider({ provider: m.provider, model: m.id }).then(() => toast.success(`Switched to ${m.name}`)).catch(() => toast.error('Failed to switch model'))
      },
    }))
    return [...actions, ...pages, ...taskItems, ...agentItems, ...sessionItems, ...modelItems]
  }, [navigate, sessions, models, tasks, agents])

  const filtered = query
    ? items.filter(i => i.label.toLowerCase().includes(query.toLowerCase()))
    : items

  useEffect(() => { setSelected(0) }, [query])
  useEffect(() => { if (open) { setQuery(''); inputRef.current?.focus() } }, [open])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') { e.preventDefault(); setSelected(s => Math.min(s + 1, filtered.length - 1)) }
    else if (e.key === 'ArrowUp') { e.preventDefault(); setSelected(s => Math.max(s - 1, 0)) }
    else if (e.key === 'Enter' && filtered[selected]) { filtered[selected].action(); onClose() }
    else if (e.key === 'Escape') { onClose() }
  }

  if (!open) return null

  let lastCategory = ''
  return (
    <div className="fixed inset-0 z-[200] flex items-start justify-center pt-[20vh]" onClick={onClose}>
      <div className="w-[520px] max-h-[400px] bg-surface-container-lowest rounded-2xl border border-outline-variant/20 shadow-2xl overflow-hidden flex flex-col" role="dialog" aria-modal="true" onClick={e => e.stopPropagation()}>
        <div className="flex items-center gap-sm px-lg py-md border-b border-outline-variant/10">
          <span className="material-symbols-outlined text-on-surface-variant">search</span>
          <input
            ref={inputRef}
            autoFocus
            className="flex-1 bg-transparent border-none outline-none font-body-lg text-on-surface placeholder:text-on-surface-variant/50"
            placeholder="Search pages, sessions, models..."
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">ESC</kbd>
        </div>
        <div className="flex-1 overflow-y-auto py-xs">
          {filtered.length === 0 && (
            <p className="text-body-sm text-on-surface-variant text-center py-lg opacity-60">No results</p>
          )}
          {filtered.map((item, i) => {
            const showCat = item.category !== lastCategory
            lastCategory = item.category
            return (
              <div key={item.id}>
                {showCat && <p className="px-lg pt-sm pb-xs font-label-sm text-on-surface-variant/60 uppercase tracking-wider">{item.category}</p>}
                <button
                  className={`w-full text-left px-lg py-sm flex items-center gap-md transition-colors ${i === selected ? 'bg-primary/10 text-primary' : 'text-on-surface hover:bg-surface-container-low'}`}
                  onClick={() => { item.action(); onClose() }}
                  onMouseEnter={() => setSelected(i)}
                >
                  <span className="material-symbols-outlined text-[18px]">{item.icon}</span>
                  <span className="font-label-md truncate">{item.label}</span>
                </button>
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}

import { useState, useEffect, useRef, useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
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
  const intl = useIntl()

  const t = (id: string) => intl.formatMessage({ id })

  const items = useMemo<PaletteItem[]>(() => {
    const actions: PaletteItem[] = [
      { id: 'a-new-chat', label: t('palette.action.newChat'), icon: 'add_comment', category: t('palette.category.actions'), action: () => navigate('/chat') },
      { id: 'a-new-task', label: t('palette.action.newTask'), icon: 'add_task', category: t('palette.category.actions'), action: () => navigate('/tasks') },
      { id: 'a-new-routine', label: t('palette.action.newRoutine'), icon: 'schedule', category: t('palette.category.actions'), action: () => navigate('/routines') },
      { id: 'a-new-agent', label: t('palette.action.browseAgents'), icon: 'smart_toy', category: t('palette.category.actions'), action: () => navigate('/agents') },
      { id: 'a-toggle-theme', label: t('palette.action.changeTheme'), icon: 'palette', category: t('palette.category.actions'), action: () => navigate('/settings/theme') },
    ]
    const pages: PaletteItem[] = [
      { id: 'p-chat', label: t('nav.chat'), icon: 'chat_bubble', category: t('palette.category.pages'), action: () => navigate('/chat') },
      { id: 'p-today', label: t('palette.page.today'), icon: 'today', category: t('palette.category.pages'), action: () => navigate('/today') },
      { id: 'p-tasks', label: t('nav.scheduled'), icon: 'task_alt', category: t('palette.category.pages'), action: () => navigate('/tasks') },
      { id: 'p-conversations', label: t('nav.conversations'), icon: 'forum', category: t('palette.category.pages'), action: () => navigate('/conversations') },
      { id: 'p-ext', label: t('palette.page.extensionsHub'), icon: 'grid_view', category: t('palette.category.pages'), action: () => navigate('/extensions') },
      { id: 'p-routines', label: t('palette.page.routines'), icon: 'schedule', category: t('palette.category.pages'), action: () => navigate('/routines') },
      { id: 'p-hooks', label: t('palette.page.hooks'), icon: 'webhook', category: t('palette.category.pages'), action: () => navigate('/hooks') },
      { id: 'p-profiles', label: t('palette.page.permissionProfiles'), icon: 'shield', category: t('palette.category.pages'), action: () => navigate('/profiles') },
      { id: 'p-editor', label: t('palette.page.codeEditor'), icon: 'code', category: t('palette.category.pages'), action: () => navigate('/editor') },
      { id: 'p-perf', label: t('nav.performance'), icon: 'speed', category: t('palette.category.pages'), action: () => navigate('/perf') },
      { id: 'p-set', label: t('nav.settings'), icon: 'settings', category: t('palette.category.pages'), action: () => navigate('/settings') },
      { id: 'p-theme', label: t('palette.page.themeSettings'), icon: 'palette', category: t('palette.category.settings'), action: () => navigate('/settings/theme') },
      { id: 'p-models', label: t('palette.page.modelSettings'), icon: 'neurology', category: t('palette.category.settings'), action: () => navigate('/settings/models') },
      { id: 'p-billing', label: t('nav.usageBilling'), icon: 'credit_card', category: t('palette.category.settings'), action: () => navigate('/settings/billing') },
    ]
    const taskItems: PaletteItem[] = tasks.slice(0, 8).map(task => ({
      id: `t-${task.id}`,
      label: task.title,
      icon: task.status === 'completed' ? 'task_alt' : task.status === 'in_progress' ? 'pending' : 'radio_button_unchecked',
      category: t('palette.category.tasks'),
      action: () => navigate('/tasks'),
    }))
    const agentItems: PaletteItem[] = agents.slice(0, 5).map(a => ({
      id: `ag-${a.id}`,
      label: a.name,
      icon: 'smart_toy',
      category: t('palette.category.agents'),
      action: () => navigate('/agents'),
    }))
    const sessionItems: PaletteItem[] = sessions.slice(0, 10).map(s => ({
      id: `s-${s.id}`, label: s.title || t('palette.untitled'), icon: 'history', category: t('palette.category.recentChats'), action: () => navigate('/chat'),
    }))
    const modelItems: PaletteItem[] = models.slice(0, 5).map(m => ({
      id: `m-${m.id}`, label: m.name, icon: 'neurology', category: t('palette.category.switchModel'), action: () => {
        api.switchProvider({ provider: m.provider, model: m.id })
          .then(() => toast.success(intl.formatMessage({ id: 'palette.toast.switched' }, { name: m.name })))
          .catch(() => toast.error(t('palette.toast.switchFailed')))
      },
    }))
    return [...actions, ...pages, ...taskItems, ...agentItems, ...sessionItems, ...modelItems]
  }, [intl, navigate, sessions, models, tasks, agents, t])

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
            placeholder={t('palette.search.placeholder')}
            value={query}
            onChange={e => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
          <kbd className="text-[10px] px-1.5 py-0.5 rounded bg-surface-container-high text-on-surface-variant font-mono">ESC</kbd>
        </div>
        <div className="flex-1 overflow-y-auto py-xs">
          {filtered.length === 0 && (
            <p className="text-body-sm text-on-surface-variant text-center py-lg opacity-60">{t('palette.noResults')}</p>
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

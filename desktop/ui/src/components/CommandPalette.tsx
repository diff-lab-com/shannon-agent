import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/ui/command'
import * as api from '@/lib/tauri-api'

interface PaletteItem {
  id: string
  label: string
  icon: string
  category: string
  action: () => void
}

export default function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate()
  const { sessions, switchSession } = useSessions()
  const { models, tasks, agents, refreshConfig } = useCatalog()
  const intl = useIntl()

  const t = useT()

  const grouped = useMemo<Record<string, PaletteItem[]>>(() => {
    const actions: PaletteItem[] = [
      { id: 'a-new-chat', label: t('palette.action.newChat'), icon: 'add_comment', category: t('palette.category.actions'), action: () => navigate('/chat') },
      { id: 'a-new-task', label: t('palette.action.newTask'), icon: 'add_task', category: t('palette.category.actions'), action: () => navigate('/tasks') },
      { id: 'a-new-agent', label: t('palette.action.browseAgents'), icon: 'smart_toy', category: t('palette.category.actions'), action: () => navigate('/extensions/agents') },
      { id: 'a-toggle-theme', label: t('palette.action.changeTheme'), icon: 'palette', category: t('palette.category.actions'), action: () => navigate('/settings/theme') },
    ]
    const pages: PaletteItem[] = [
      { id: 'p-chat', label: t('nav.chat'), icon: 'chat_bubble', category: t('palette.category.pages'), action: () => navigate('/chat') },
      { id: 'p-today', label: t('palette.page.today'), icon: 'today', category: t('palette.category.pages'), action: () => navigate('/tasks') },
      { id: 'p-tasks', label: t('nav.scheduled'), icon: 'task_alt', category: t('palette.category.pages'), action: () => navigate('/tasks') },
      { id: 'p-ext', label: t('palette.page.extensionsHub'), icon: 'grid_view', category: t('palette.category.pages'), action: () => navigate('/extensions') },
      { id: 'p-editor', label: t('palette.page.codeEditor'), icon: 'code', category: t('palette.category.pages'), action: () => navigate('/editor') },
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
      action: () => navigate('/extensions/agents'),
    }))
    const sessionItems: PaletteItem[] = sessions.slice(0, 10).map(s => ({
      id: `s-${s.id}`, label: s.title || t('palette.untitled'), icon: 'history', category: t('palette.category.recentChats'), action: () => {
        switchSession(s.id)
        navigate('/chat')
      },
    }))
    const modelItems: PaletteItem[] = models.slice(0, 5).map(m => ({
      id: `m-${m.id}`, label: m.name, icon: 'neurology', category: t('palette.category.switchModel'), action: () => {
        api.configure({ key: 'model', value: m.id })
          .then(async () => {
            await refreshConfig()
            toast.success(intl.formatMessage({ id: 'palette.toast.switched' }, { name: m.name }))
          })
          .catch((e) => toastError(t('palette.toast.switchFailed'), e))
      },
    }))

    // Preserve category order from the original implementation. cmdk renders
    // groups in insertion order, so this list doubles as the visual order.
    const order = [
      t('palette.category.actions'),
      t('palette.category.pages'),
      t('palette.category.settings'),
      t('palette.category.tasks'),
      t('palette.category.agents'),
      t('palette.category.recentChats'),
      t('palette.category.switchModel'),
    ]
    const all = [...actions, ...pages, ...taskItems, ...agentItems, ...sessionItems, ...modelItems]
    const map: Record<string, PaletteItem[]> = {}
    for (const cat of order) map[cat] = []
    for (const item of all) {
      if (!map[item.category]) map[item.category] = []
      map[item.category].push(item)
    }
    return map
  }, [intl, navigate, sessions, models, tasks, agents, refreshConfig, switchSession, t])

  return (
    <CommandDialog
      open={open}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('palette.search.placeholder')}
    >
      <CommandInput placeholder={t('palette.search.placeholder')} />
      <CommandList>
        <CommandEmpty>{t('palette.noResults')}</CommandEmpty>
        {Object.entries(grouped).map(([category, items]) =>
          items.length === 0 ? null : (
            <CommandGroup key={category} heading={category}>
              {items.map(item => (
                <CommandItem
                  key={item.id}
                  value={`${item.label} ${category}`}
                  onSelect={() => { item.action(); onClose() }}
                >
                  <span className="material-symbols-outlined text-[18px]">{item.icon}</span>
                  <span className="font-label-md truncate">{item.label}</span>
                </CommandItem>
              ))}
            </CommandGroup>
          ),
        )}
      </CommandList>
    </CommandDialog>
  )
}
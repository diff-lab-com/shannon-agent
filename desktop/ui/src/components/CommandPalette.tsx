import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import { exportSessionAsMarkdown } from '@/lib/sessionActions'
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
  /** Old or alternative terms the search should still match (UI audit §6.1
      migration window: a user who remembered "分流队列" can still reach the
      Inbox page by typing either word). */
  synonyms?: string[]
}

export default function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate()
  const { sessions, currentSessionId, switchSession } = useSessions()
  const { models, tasks, agents, refreshConfig } = useCatalog()
  const intl = useIntl()

  const t = useT()

  const grouped = useMemo<Record<string, PaletteItem[]>>(() => {
    const actions: PaletteItem[] = [
      { id: 'a-new-chat', label: t('palette.action.newChat'), icon: 'add_comment', category: t('palette.category.actions'), action: () => navigate('/chat') },
      { id: 'a-new-task', label: t('palette.action.newTask'), icon: 'add_task', category: t('palette.category.actions'), action: () => navigate('/tasks') },
      { id: 'a-new-agent', label: t('palette.action.browseAgents'), icon: 'smart_toy', category: t('palette.category.actions'), action: () => navigate('/extensions/agents') },
      { id: 'a-toggle-theme', label: t('palette.action.changeTheme'), icon: 'palette', category: t('palette.category.actions'), action: () => navigate('/settings/theme') },
      // Slash-command parity: /export is self-contained (native save dialog),
      // so it is reachable from the palette too; the diagnostics commands
      // (/context /cost /diff) render their result in the chat composer and
      // stay composer-only by design.
      { id: 'a-export-chat', label: t('slash.command.export.label'), icon: 'download', category: t('palette.category.actions'), action: () => {
          if (!currentSessionId) { toast.info(t('slash.needsSession')); return }
          void exportSessionAsMarkdown(currentSessionId, sessions, t)
        } },
    ]
    const pages: PaletteItem[] = [
      { id: 'p-chat', label: t('nav.chat'), icon: 'chat_bubble', category: t('palette.category.pages'), action: () => navigate('/chat') },
      { id: 'p-today', label: t('palette.page.today'), icon: 'today', category: t('palette.category.pages'), action: () => navigate('/tasks') },
      { id: 'p-tasks', label: t('nav.scheduled'), icon: 'task_alt', category: t('palette.category.pages'), action: () => navigate('/tasks'),
        // IA 2026-09 (T1 术语一轨): the page now displays as「自动化」, but
        // users who still think of it as「任务」must keep landing on it —
        // the old term survives in the search layer only, never in the copy.
        synonyms: ['已排程', 'scheduled', '定时任务', '任务', 'tasks'] },
      { id: 'p-inbox', label: t('nav.triage'), icon: 'inbox', category: t('palette.category.pages'), action: () => navigate('/triage'),
        synonyms: ['triage', '分流队列', '分诊'] },
      { id: 'p-ext', label: t('palette.page.extensionsHub'), icon: 'grid_view', category: t('palette.category.pages'), action: () => navigate('/extensions'),
        synonyms: ['extensions', '扩展', 'connectors', '连接'] },
      { id: 'p-opc', label: t('nav.opc'), icon: 'dashboard', category: t('palette.category.pages'), action: () => navigate('/opc'),
        synonyms: ['opc', 'mission control', '指挥台', '单人公司'] },
      { id: 'p-editor', label: t('palette.page.codeEditor'), icon: 'code', category: t('palette.category.pages'), action: () => { navigate('/chat'); window.dispatchEvent(new Event('shannon:open-editor')) },
        synonyms: ['editor', '编辑器'] },
      { id: 'p-set', label: t('nav.settings'), icon: 'settings', category: t('palette.category.pages'), action: () => navigate('/settings') },
      { id: 'p-theme', label: t('palette.page.themeSettings'), icon: 'palette', category: t('palette.category.settings'), action: () => navigate('/settings/theme') },
      { id: 'p-models', label: t('palette.page.modelSettings'), icon: 'neurology', category: t('palette.category.settings'), action: () => navigate('/settings/models') },
    ]
    // 2026-09 review: remove the hard 5/8/10 slice caps so the palette
    // surfaces every task / agent / session / model (cmdk filters the
    // visible set by the user's query anyway). We surface a "+N more"
    // counter on each group heading so the user knows the list is
    // unfiltered vs. query-narrowed.
    const taskItems: PaletteItem[] = tasks.map(task => ({
      id: `t-${task.id}`,
      label: task.title,
      icon: task.status === 'completed' ? 'task_alt' : task.status === 'in_progress' ? 'pending' : 'radio_button_unchecked',
      category: t('palette.category.tasks'),
      action: () => navigate('/tasks'),
    }))
    const agentItems: PaletteItem[] = agents.map(a => ({
      id: `ag-${a.id}`,
      label: a.name,
      icon: 'smart_toy',
      category: t('palette.category.agents'),
      action: () => navigate('/extensions/agents'),
    }))
    const sessionItems: PaletteItem[] = sessions.map(s => ({
      id: `s-${s.id}`, label: s.title || t('palette.untitled'), icon: 'history', category: t('palette.category.recentChats'), action: () => {
        switchSession(s.id)
        navigate('/chat')
      },
    }))
    const modelItems: PaletteItem[] = models.map(m => ({
      id: `m-${m.id}`, label: m.name, icon: 'neurology', category: t('palette.category.switchModel'), action: () => {
        // Decision 1 (review P1-2 / B1-8): write the catalog id and pin the
        // model's OWN provider. `configure('model')` targets the currently
        // active provider, so switching to another provider's model without
        // the provider write would nail the foreign id onto the wrong
        // provider (the exact bug the review caught on this path).
        api.configure({ key: 'model', value: m.id })
          .then(() => api.configure({ key: 'provider', value: m.provider }))
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
  }, [intl, navigate, sessions, currentSessionId, models, tasks, agents, refreshConfig, switchSession, t])

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
            <CommandGroup
              key={category}
              heading={
                // Show a "+N more" pill on each group so users see the
                // total count without scrolling past cmdk's viewport.
                // Counts include all items in the group, not just the
                // query-filtered ones, so it reads as "5 tasks available".
                <>
                  {category}
                  <span className="ml-2 px-xs py-[1px] rounded-full bg-surface-container-high text-on-surface-variant font-label-xs tabular-nums">
                    {items.length}
                  </span>
                </>
              }
            >
              {items.map(item => (
                <CommandItem
                  key={item.id}
                  // cmdk fuzzy-filters against `value`; appending synonyms keeps
                  // old terms reachable (audit §6.1 migration window).
                  value={`${item.label} ${category} ${(item.synonyms ?? []).join(' ')}`}
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
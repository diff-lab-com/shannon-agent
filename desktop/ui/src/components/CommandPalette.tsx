import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { Command as CommandPrimitive } from 'cmdk'
import { useT } from '@/i18n'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import { exportSessionAsMarkdown } from '@/lib/sessionActions'
import { formatRelativeTime } from '@/components/SidebarSessions'
import {
  CommandDialog,
  CommandEmpty,
  CommandGroup,
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
  /** Aurora 2026-10 (14 页 P2): right-edge meta — relative time on session
   *  entries; a "current" badge on the active model. Rendered beside the
   *  label, never part of the search value. */
  meta?: string
  metaTone?: 'current' | 'muted'
}

export default function CommandPalette({ open, onClose }: { open: boolean; onClose: () => void }) {
  const navigate = useNavigate()
  const { sessions, currentSessionId, switchSession } = useSessions()
  const { models, status, tasks, agents, refreshConfig } = useCatalog()
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
      // G5 P1-11: the companion Quick Capture window was tray-only before —
      // the palette is the second in-app entry (the header icon is the first).
      { id: 'a-open-companion', label: t('palette.action.openCompanion'), icon: 'picture_in_picture', category: t('palette.category.actions'), action: () => {
          api.openCompanionWindow().catch(e => toastError(t('header.companion.failed'), e))
        } },
    ]
    const pages: PaletteItem[] = [
      { id: 'p-chat', label: t('nav.chat'), icon: 'chat_bubble', category: t('palette.category.pages'), action: () => navigate('/chat') },
      { id: 'p-today', label: t('palette.page.today'), icon: 'today', category: t('palette.category.pages'), action: () => navigate('/tasks') },
      { id: 'p-tasks', label: t('nav.tasks'), icon: 'task_alt', category: t('palette.category.pages'), action: () => navigate('/tasks'),
        // R3 术语终检 (§一 wording audit): sidebar/header/H1 all say「任务」—
        // the palette was the last surface still showing「自动化」(IA 2026-09
        // T1 半截迁移). The retired term survives in the search layer only.
        synonyms: ['自动化', '已排程', 'scheduled', '定时任务', 'tasks'] },
      { id: 'p-usage', label: t('header.title.usage'), icon: 'monitoring', category: t('palette.category.pages'), action: () => navigate('/usage'),
        synonyms: ['成本', 'cost', 'budget', '预算', 'usage'] },
      { id: 'p-inbox', label: t('nav.triage'), icon: 'inbox', category: t('palette.category.pages'), action: () => navigate('/triage'),
        synonyms: ['triage', '分流队列', '分诊'] },
      { id: 'p-ext', label: t('palette.page.extensionsHub'), icon: 'grid_view', category: t('palette.category.pages'), action: () => navigate('/extensions'),
        synonyms: ['extensions', '扩展', 'connectors', '连接'] },
      { id: 'p-opc', label: t('nav.opc'), icon: 'dashboard', category: t('palette.category.pages'), action: () => navigate('/opc'),
        synonyms: ['opc', 'mission control', '指挥台', '单人公司'] },
      { id: 'p-editor', label: t('palette.page.codeEditor'), icon: 'code', category: t('palette.category.pages'), action: () => { navigate('/chat'); window.dispatchEvent(new Event('shannon:open-editor')) },
        synonyms: ['editor', '编辑器'] },
      { id: 'p-set', label: t('nav.settings'), icon: 'settings', category: t('palette.category.pages'), action: () => navigate('/settings') },
      // R2-P1-9: every settings subpage is reachable from the palette, not
      // just theme/models — same group, rail icons, and plain navigate as
      // the existing entries. IA 2026-10 (ADVERSARIAL-REVIEW §2): the group
      // mirrors the 8-section rail exactly (network/session/remotes deep
      // links redirect in App.tsx, so they no longer get palette entries).
      { id: 'p-set-general', label: t('nav.general'), icon: 'tune', category: t('palette.category.settings'), action: () => navigate('/settings/general') },
      { id: 'p-theme', label: t('nav.theme'), icon: 'palette', category: t('palette.category.settings'), action: () => navigate('/settings/theme') },
      { id: 'p-models', label: t('nav.models'), icon: 'smart_toy', category: t('palette.category.settings'), action: () => navigate('/settings/models') },
      { id: 'p-set-permissions', label: t('nav.permissions'), icon: 'shield', category: t('palette.category.settings'), action: () => navigate('/settings/permissions') },
      { id: 'p-set-notifications', label: t('nav.notifications'), icon: 'notifications', category: t('palette.category.settings'), action: () => navigate('/settings/notifications') },
      { id: 'p-set-connections', label: t('nav.connections'), icon: 'cloud', category: t('palette.category.settings'), action: () => navigate('/settings/connections') },
      { id: 'p-set-about', label: t('nav.about'), icon: 'info', category: t('palette.category.settings'), action: () => navigate('/settings/about') },
      { id: 'p-set-advanced', label: t('nav.advanced'), icon: 'developer_mode', category: t('palette.category.settings'), action: () => navigate('/settings/advanced') },
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
    // Aurora 2026-10 (14 页 P2): session entries carry their last-activity
    // time on the right edge (the sidebar's compact relative labels); the
    // active model wears a "current" badge (design: 当前 sonnet-4-6).
    // `now` is deliberately render-time — the palette is ephemeral, a stale
    // "17h" for the seconds it is open is invisible; no tick machinery.
    const now = Date.now()
    const sessionItems: PaletteItem[] = sessions.map(s => ({
      id: `s-${s.id}`, label: s.title || t('palette.untitled'), icon: 'history', category: t('palette.category.recentChats'),
      meta: formatRelativeTime(s.updated_at ?? s.created_at, now, t) || undefined,
      metaTone: 'muted',
      action: () => {
        switchSession(s.id)
        navigate('/chat')
      },
    }))
    const modelItems: PaletteItem[] = models.map(m => {
      const isCurrent = m.name === status?.model || m.id === status?.model
      return {
        id: `m-${m.id}`, label: m.name, icon: 'neurology', category: t('palette.category.switchModel'),
        meta: isCurrent ? t('palette.item.current') : undefined,
        metaTone: isCurrent ? 'current' : undefined,
        action: () => {
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
      }
    })

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
  }, [intl, navigate, sessions, currentSessionId, models, status, tasks, agents, refreshConfig, switchSession, t])

  return (
    // G1-A parity batch (2026-10-15 review §1.2, design 14-palette): the
    // panel anchors at top 14vh (was the 1/3 vertical center) at the design's
    // 560px width; the aurora top line stays OFF this surface (the signature
    // belongs to the composer and main glass panels only).
    <CommandDialog
      open={open}
      onOpenChange={(o) => { if (!o) onClose() }}
      title={t('palette.search.placeholder')}
      className="top-[14vh] w-[560px] sm:max-w-[560px]"
    >
      {/* Flush pin row (design .palette .pin): [search icon] [typed text,
          14px] … [esc keycap] on ONE line, no boxed input; the hairline below
          separates it from the list. Typing filters exactly as before — the
          cmdk Input keeps the whole keyboard contract (uncontrolled, focused
          on open). */}
      <div
        data-testid="palette-input-row"
        className="flex items-center gap-sm px-md py-[13px] border-b border-outline-variant/20 shrink-0"
      >
        <span className="material-symbols-outlined icon-sm text-on-surface-variant shrink-0" aria-hidden="true">search</span>
        <CommandPrimitive.Input
          data-testid="palette-input"
          placeholder={t('palette.search.placeholder')}
          className="flex-1 min-w-0 bg-transparent border-none outline-none p-0 text-body-sm text-on-surface placeholder:text-muted-foreground"
        />
        <span className="kbd shrink-0" aria-hidden="true">esc</span>
      </div>
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
                  <span className="ml-sm px-xs py-[1px] rounded-full bg-surface-container-high text-on-surface-variant font-label-xs tabular-nums">
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
                  <span className="material-symbols-outlined icon-sm text-muted-foreground group-data-[selected=true]/command-item:text-primary">{item.icon}</span>
                  <span className="font-label-md truncate flex-1 min-w-0">{item.label}</span>
                  {item.meta && (
                    <span
                      data-testid={item.metaTone === 'current' ? 'palette-item-current-badge' : 'palette-item-meta'}
                      className={
                        item.metaTone === 'current'
                          ? 'shrink-0 px-xs py-[1px] rounded-full bg-primary-container text-on-primary-container font-label-xs font-bold'
                          : 'shrink-0 font-mono text-label-xs text-on-surface-variant tabular-nums'
                      }
                    >
                      {item.meta}
                    </span>
                  )}
                  {/* The design row's right-edge enter keycap — visible on the
                      keyboard-selected row only. */}
                  <span className="kbd hidden shrink-0 group-data-[selected=true]/command-item:inline-flex" aria-hidden="true">↵</span>
                </CommandItem>
              ))}
            </CommandGroup>
          ),
        )}
      </CommandList>
      {/* G1-A parity batch: the design's bottom key-hint bar as keycaps —
          ↑↓ navigate · ↵ select · esc close, with the
          muted product signature at the right edge. (The pre-G1-A rendering
          was plain text via palette.footer.shortcuts; that key is retired.
          The design mockup also advertises tab 切组 — no group-cycling
          handler exists, so the cap is NOT rendered: caps only advertise
          bindings that work.) */}
      <div
        data-testid="palette-footer"
        className="flex items-center gap-xs px-md py-xs border-t border-outline-variant/20 text-label-xs text-on-surface-variant shrink-0"
      >
        <span className="kbd" aria-hidden="true">↑</span>
        <span className="kbd" aria-hidden="true">↓</span>
        <span className="mr-sm">{t('palette.footer.navigate')}</span>
        <span className="kbd" aria-hidden="true">↵</span>
        <span className="mr-sm">{t('palette.footer.select')}</span>
        <span className="kbd" aria-hidden="true">esc</span>
        <span>{t('palette.footer.close')}</span>
        <span className="ml-auto font-label-xs text-muted-foreground">{t('app.brandName')}</span>
      </div>
    </CommandDialog>
  )
}

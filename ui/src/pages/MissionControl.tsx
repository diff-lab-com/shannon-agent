// Mission Control — full-screen kanban grid aggregating tasks across all teams.
//
// SCOPE: READ-ONLY observation surface. No create/edit/cancel actions.
// Click a card to open the TaskDetailDrawer (which itself has the write actions).
//
// DISTINCTION from Tasks and OPC:
//   - Tasks: full CRUD for scheduled routines + history + worktrees.
//   - OPC: agent-orchestration workspace with optimistic DnD (write surface).
//   - MissionControl (this page): cross-team status grid — observation only.
//
// This page now consumes the shared KanbanBoard primitive (mode='observe') and
// the unified task-status taxonomy in lib/task-status.ts. The column taxonomy
// is identical to OPC; only the interaction mode differs (click vs drag).

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { useApp } from '@/context/AppContext'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import { KanbanBoard, STATUS_FAMILY, type TaskStatusFamily } from '@/components/shared/KanbanBoard'
import ConversationsToday from '@/components/conversations/ConversationsToday'
import ConversationsList from '@/components/conversations/ConversationsList'
import { useSidebarMode } from '@/components/Sidebar'

type TabKey = 'today' | 'all' | 'board'

const ALL_TABS: { key: TabKey; label: string; icon: string; devOnly?: boolean }[] = [
  { key: 'today', label: 'missionControl.today', icon: 'today' },
  { key: 'all', label: 'missionControl.all', icon: 'forum' },
  { key: 'board', label: 'missionControl.board', icon: 'dashboard', devOnly: true },
]

// F5: persist the user's last-selected Conversations tab so they land back on
// Today (or All/Board) when they return. Default to 'today' so the MVP view
// (today's chats + due tasks) is surfaced first.
const TAB_STORAGE_KEY = 'shannon-conversations-tab'

function loadInitialTab(): TabKey {
  if (typeof window === 'undefined') return 'today'
  const saved = window.localStorage.getItem(TAB_STORAGE_KEY)
  if (saved === 'today' || saved === 'all' || saved === 'board') return saved
  return 'today'
}

interface MissionControlProps {
  onSelectTask?: (id: string) => void
}

export default function MissionControl({ onSelectTask }: MissionControlProps) {
  const intl = useIntl()
  const t = (id: string, values?: any) => intl.formatMessage({ id }, values)
  const { tasks, sessions, refreshTasks } = useApp()
  const [sidebarMode] = useSidebarMode()
  const isDev = sidebarMode === 'dev'
  const tabs = ALL_TABS.filter(tab => !tab.devOnly || isDev)
  const [tab, setTab] = useState<TabKey>(loadInitialTab)
  const [localSelectedId, setLocalSelectedId] = useState<string | null>(null)
  const [boardFilter, setBoardFilter] = useState<TaskStatusFamily | null>(null)
  // If the active tab is dev-only but the user is in simple mode, fall back.
  const activeTab: TabKey = tab === 'board' && !isDev ? 'all' : tab
  const selectedTask = localSelectedId ? tasks.find(task => task.id === localSelectedId) ?? null : null

  const handleSelect = (id: string) => {
    setLocalSelectedId(id)
    onSelectTask?.(id)
  }

  const handleTabChange = (next: TabKey) => {
    setTab(next)
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(TAB_STORAGE_KEY, next)
    }
  }

  const totals = tasks.reduce<Record<TaskStatusFamily, number>>(
    (acc, t) => {
      // cheap inline classify to avoid recomputing the whole group; KanbanBoard
      // does its own grouping for rendering — this is just for the header chips.
      const s = (t.status ?? '').toLowerCase()
      for (const fam of Object.values(STATUS_FAMILY)) {
        if (fam.statuses.includes(s)) { acc[fam.key]++; break }
      }
      return acc
    },
    { queued: 0, active: 0, blocked: 0, done: 0, failed: 0 },
  )

  const headerExtra = (
    <div className="flex gap-xs flex-wrap">
      {(Object.keys(STATUS_FAMILY) as TaskStatusFamily[]).map(key => {
        const meta = STATUS_FAMILY[key]
        const active = boardFilter === key
        return (
          <button
            key={key}
            type="button"
            aria-pressed={active}
            title={active ? t('missionControl.filter.clear') : t('missionControl.filter.focus', { family: t(meta.titleKey) })}
            onClick={() => {
              if (activeTab !== 'board') handleTabChange('board')
              setBoardFilter(active ? null : key)
            }}
            className={`flex items-center gap-xs px-sm py-xs rounded-full text-label-sm font-label-md transition-all cursor-pointer hover:scale-[1.03] ${meta.bgClass} ${active ? 'ring-2 ring-primary ring-offset-1 ring-offset-surface-container-lowest' : ''}`}
          >
            <span className={`w-2 h-2 rounded-full ${meta.dotClass}`} />
            <span className="text-on-surface-variant">{t(meta.titleKey)}</span>
            <span className="text-on-surface font-bold">{totals[key]}</span>
          </button>
        )
      })}
    </div>
  )

  return (
    <div className="flex-1 flex flex-col w-full overflow-hidden">
      <header className="px-lg py-md border-b border-outline-variant/20 bg-surface-container-lowest/60 backdrop-blur-md">
        <div className="flex items-end justify-between gap-md flex-wrap">
          <div>
            <h2 className="font-headline-lg text-headline-lg text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">dashboard</span>
              {t('missionControl.title')}
            </h2>
            <p className="text-on-surface-variant mt-xs text-body-sm">
              {t('missionControl.subtitle', { count: tasks.length })}
            </p>
          </div>
          {headerExtra}
        </div>
      </header>

      <nav
        aria-label="Conversations view tabs"
        className="flex items-center gap-xs px-lg pt-md border-b border-outline-variant/20 bg-surface-container-lowest/40"
      >
        {tabs.map(tab => {
          const active = activeTab === tab.key
          return (
            <button
              key={tab.key}
              onClick={() => handleTabChange(tab.key)}
              className={`flex items-center gap-sm px-md py-sm rounded-t-lg font-label-md text-label-md transition-all cursor-pointer ${
                active
                  ? 'text-primary border-b-2 border-primary -mb-px font-bold'
                  : 'text-on-surface-variant hover:text-primary hover:bg-surface-container-low'
              }`}
              aria-current={active ? 'page' : undefined}
              aria-pressed={active}
              aria-label={`${t(tab.label)} tab`}
            >
              <span className="material-symbols-outlined text-[18px]">{tab.icon}</span>
              <span>{t(tab.label)}</span>
            </button>
          )
        })}
      </nav>

      {activeTab === 'today' && <ConversationsToday sessions={sessions} tasks={tasks} />}
      {activeTab === 'all' && <ConversationsList sessions={sessions} />}

      {activeTab === 'board' && (
        <div className="flex-1 flex min-h-0 px-lg py-lg">
          <KanbanBoard
            tasks={tasks}
            mode="observe"
            columns={boardFilter ? [boardFilter] : undefined}
            onSelectTask={handleSelect}
          />
        </div>
      )}

      <TaskDetailDrawer
        task={selectedTask}
        onClose={() => setLocalSelectedId(null)}
        onUpdated={() => void refreshTasks()}
      />
    </div>
  )
}

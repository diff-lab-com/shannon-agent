// ConversationsToday — daily dashboard shown on the "Today" tab.
//
// Surfaced at-a-glance answers to: what did I do today, what's running,
// what's due. Heavily filtered to "today" so users don't have to scroll.

import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import { useApp } from '@/context/AppContext'
import type { SessionInfo, TaskItem } from '@/types'
import { classifyStatus, STATUS_FAMILY } from '@/lib/task-status'

function isToday(epochMs: number): boolean {
  if (!epochMs) return false
  const d = new Date(epochMs)
  const now = new Date()
  return d.getFullYear() === now.getFullYear()
    && d.getMonth() === now.getMonth()
    && d.getDate() === now.getDate()
}

function isWithinDays(epochMs: number, days: number): boolean {
  if (!epochMs) return false
  return Date.now() - epochMs < days * 86_400_000
}

function relativeTime(epochMs: number, t: (id: string, values?: Record<string, string | number>) => string): string {
  if (!epochMs) return ''
  const diff = Date.now() - epochMs
  if (diff < 60_000) return t('conversations.today.justNow')
  if (diff < 3_600_000) return t('conversations.today.minutesAgo', { count: Math.floor(diff / 60_000) })
  if (diff < 86_400_000) return t('conversations.today.hoursAgo', { count: Math.floor(diff / 3_600_000) })
  return new Date(epochMs).toLocaleDateString()
}

interface Props {
  sessions: SessionInfo[]
  tasks: TaskItem[]
}

export default function ConversationsToday({ sessions, tasks }: Props) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values)
  const { agents, switchSession } = useApp()
  const navigate = useNavigate()

  const openSession = async (id: string) => {
    await switchSession(id)
    navigate('/chat')
  }

  const todaySessions = useMemo(
    () => sessions.filter(s => isToday(s.created_at)).sort((a, b) => b.created_at - a.created_at),
    [sessions],
  )

  // North-star metric: Weekly Active Conversations (WAC) — unique chat
  // sessions touched in the last 7 days. Cheap client-side approximation
  // off the sessions list; replace with a server-side aggregate when the
  // backend exposes it.
  const wac = useMemo(
    () => sessions.filter(s => isWithinDays(s.created_at, 7)).length,
    [sessions],
  )

  const runningTasks = useMemo(
    () => tasks.filter(t => {
      const fam = classifyStatus(t.status)
      return fam === 'active' || fam === 'queued'
    }),
    [tasks],
  )

  const completedToday = useMemo(
    () => tasks.filter(t => classifyStatus(t.status) === 'done'),
    [tasks],
  )

  const dueToday = useMemo(
    () => tasks
      .filter(t => t.due_date && isToday(t.due_date * 1000))
      .sort((a, b) => (a.due_date ?? 0) - (b.due_date ?? 0))
      .slice(0, 5),
    [tasks],
  )

  const runningAgents = useMemo(
    () => agents.filter(a => a.status === 'running').slice(0, 3),
    [agents],
  )

  return (
    <div className="flex-1 overflow-y-auto px-lg py-lg space-y-xl">
      {/* North-star metric: Weekly Active Conversations */}
      <section
        aria-label={t('conversations.today.wacAria')}
        className="p-lg rounded-2xl bg-gradient-to-br from-primary-container/40 via-primary/10 to-transparent border border-primary/30 flex items-center gap-lg"
      >
        <div className="w-14 h-14 rounded-2xl bg-primary/15 flex items-center justify-center shrink-0">
          <span className="material-symbols-outlined text-primary text-[32px]">insights</span>
        </div>
        <div className="flex-1">
          <div className="font-label-sm text-on-surface-variant uppercase tracking-wider">{t('conversations.today.weeklyActive')}</div>
          <div className="font-headline-lg text-on-surface text-[40px] leading-none mt-xs">{wac}</div>
          <div className="font-label-sm text-on-surface-variant mt-xs">{t('conversations.today.chats7d')}</div>
        </div>
      </section>

      {/* Hero stats */}
      <section className="grid grid-cols-1 sm:grid-cols-3 gap-md">
        <StatCard
          icon="forum"
          label={t('conversations.today.chatsToday')}
          value={todaySessions.length}
          tone="primary"
        />
        <StatCard
          icon="play_circle"
          label={t('conversations.today.runningTasks')}
          value={runningTasks.length}
          tone="tertiary"
        />
        <StatCard
          icon="task_alt"
          label={t('conversations.today.completed')}
          value={completedToday.length}
          tone="success"
        />
      </section>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-xl">
        {/* Recent chats */}
        <section>
          <header className="flex items-center justify-between mb-md">
            <h3 className="font-headline-md text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">forum</span>
              {t('conversations.today.recentChats')}
            </h3>
            <button
              onClick={() => navigate('/chat')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              {t('conversations.today.openChat')}
            </button>
          </header>
          {todaySessions.length === 0 ? (
            <EmptyHint icon="chat_bubble_outline" text={t('conversations.today.noChatsToday')} />
          ) : (
            <ul className="space-y-sm">
              {todaySessions.slice(0, 5).map(s => (
                <li key={s.id}>
                  <button
                    onClick={() => openSession(s.id)}
                    className="w-full text-left p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30 hover:border-primary/40 transition-colors cursor-pointer flex items-center gap-md"
                  >
                    <span className="material-symbols-outlined text-on-surface-variant">chat_bubble</span>
                    <div className="flex-1 min-w-0">
                      <div className="font-label-md text-on-surface truncate">{s.title || t('conversations.today.untitledChat')}</div>
                      <div className="font-label-sm text-on-surface-variant mt-xs">
                        {t('conversations.today.messages', { count: s.message_count })} · {relativeTime(s.created_at, t)}
                      </div>
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* Due today */}
        <section>
          <header className="flex items-center justify-between mb-md">
            <h3 className="font-headline-md text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">event</span>
              {t('conversations.today.dueToday')}
            </h3>
            <button
              onClick={() => navigate('/tasks')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              {t('conversations.today.allTasks')}
            </button>
          </header>
          {dueToday.length === 0 ? (
            <EmptyHint icon="event_available" text={t('conversations.today.nothingDue')} />
          ) : (
            <ul className="space-y-sm">
              {dueToday.map(task => {
                const fam = classifyStatus(task.status)
                const meta = STATUS_FAMILY[fam]
                return (
                  <li key={task.id} className="p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30 flex items-center gap-md">
                    <span className={`w-2 h-2 rounded-full shrink-0 ${meta.dotClass}`} />
                    <div className="flex-1 min-w-0">
                      <div className="font-label-md text-on-surface truncate">{task.title}</div>
                      <div className="font-label-sm text-on-surface-variant mt-xs">
                        {t(meta.titleKey)}
                        {task.due_date && <> · {t('conversations.today.due', { time: new Date(task.due_date * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }) })}</>}
                      </div>
                    </div>
                  </li>
                )
              })}
            </ul>
          )}
        </section>
      </div>

      {/* Active agents */}
      {runningAgents.length > 0 && (
        <section>
          <header className="flex items-center justify-between mb-md">
            <h3 className="font-headline-md text-on-surface flex items-center gap-sm">
              <span className="material-symbols-outlined text-primary">smart_toy</span>
              {t('conversations.today.activeAgents')}
            </h3>
            <button
              onClick={() => navigate('/opc')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              {t('conversations.today.openOpc')}
            </button>
          </header>
          <ul className="grid grid-cols-1 sm:grid-cols-3 gap-sm">
            {runningAgents.map(a => (
              <li key={a.id} className="p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30">
                <div className="flex items-center gap-sm">
                  <span className="w-2 h-2 rounded-full bg-tertiary animate-pulse shrink-0" />
                  <div className="font-label-md text-on-surface truncate">{a.name}</div>
                </div>
                {a.task && <div className="font-label-sm text-on-surface-variant mt-xs truncate">{a.task}</div>}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  )
}

function StatCard({ icon, label, value, tone }: { icon: string; label: string; value: number; tone: 'primary' | 'tertiary' | 'success' }) {
  const toneClass = tone === 'primary'
    ? 'bg-primary/10 text-primary'
    : tone === 'tertiary'
      ? 'bg-tertiary/10 text-tertiary'
      : 'bg-success/10 text-success'
  return (
    <div className="p-md rounded-2xl bg-surface-container-lowest border border-outline-variant/30 flex items-center gap-md">
      <div className={`w-12 h-12 rounded-xl flex items-center justify-center shrink-0 ${toneClass}`}>
        <span className="material-symbols-outlined">{icon}</span>
      </div>
      <div>
        <div className="font-headline-lg text-on-surface leading-none">{value}</div>
        <div className="font-label-sm text-on-surface-variant mt-xs">{label}</div>
      </div>
    </div>
  )
}

function EmptyHint({ icon, text }: { icon: string; text: string }) {
  return (
    <div className="p-lg rounded-xl bg-surface-container-lowest border border-dashed border-outline-variant/50 flex flex-col items-center text-center">
      <span className="material-symbols-outlined text-outline-variant mb-sm">{icon}</span>
      <p className="font-body-sm text-on-surface-variant">{text}</p>
    </div>
  )
}

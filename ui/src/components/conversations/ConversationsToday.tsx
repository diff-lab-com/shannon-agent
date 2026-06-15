// ConversationsToday — daily dashboard shown on the "Today" tab.
//
// Surfaced at-a-glance answers to: what did I do today, what's running,
// what's due. Heavily filtered to "today" so users don't have to scroll.

import { useMemo } from 'react'
import { useNavigate } from 'react-router-dom'
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

function relativeTime(epochMs: number): string {
  if (!epochMs) return ''
  const diff = Date.now() - epochMs
  if (diff < 60_000) return 'just now'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`
  return new Date(epochMs).toLocaleDateString()
}

interface Props {
  sessions: SessionInfo[]
  tasks: TaskItem[]
}

export default function ConversationsToday({ sessions, tasks }: Props) {
  const { agents } = useApp()
  const navigate = useNavigate()

  const todaySessions = useMemo(
    () => sessions.filter(s => isToday(s.created_at)).sort((a, b) => b.created_at - a.created_at),
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
      {/* Hero stats */}
      <section className="grid grid-cols-1 sm:grid-cols-3 gap-md">
        <StatCard
          icon="forum"
          label="Chats today"
          value={todaySessions.length}
          tone="primary"
        />
        <StatCard
          icon="play_circle"
          label="Running tasks"
          value={runningTasks.length}
          tone="tertiary"
        />
        <StatCard
          icon="task_alt"
          label="Completed"
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
              Recent chats
            </h3>
            <button
              onClick={() => navigate('/chat')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              Open Chat →
            </button>
          </header>
          {todaySessions.length === 0 ? (
            <EmptyHint icon="chat_bubble_outline" text="No chats yet today. Start one in Chat." />
          ) : (
            <ul className="space-y-sm">
              {todaySessions.slice(0, 5).map(s => (
                <li key={s.id}>
                  <button
                    onClick={() => navigate('/chat')}
                    className="w-full text-left p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30 hover:border-primary/40 transition-colors cursor-pointer flex items-center gap-md"
                  >
                    <span className="material-symbols-outlined text-on-surface-variant">chat_bubble</span>
                    <div className="flex-1 min-w-0">
                      <div className="font-label-md text-on-surface truncate">{s.title || 'Untitled chat'}</div>
                      <div className="font-label-sm text-on-surface-variant mt-xs">
                        {s.message_count} message{s.message_count === 1 ? '' : 's'} · {relativeTime(s.created_at)}
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
              Due today
            </h3>
            <button
              onClick={() => navigate('/tasks')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              All tasks →
            </button>
          </header>
          {dueToday.length === 0 ? (
            <EmptyHint icon="event_available" text="Nothing due today." />
          ) : (
            <ul className="space-y-sm">
              {dueToday.map(t => {
                const fam = classifyStatus(t.status)
                const meta = STATUS_FAMILY[fam]
                return (
                  <li key={t.id} className="p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30 flex items-center gap-md">
                    <span className={`w-2 h-2 rounded-full shrink-0 ${meta.dotClass}`} />
                    <div className="flex-1 min-w-0">
                      <div className="font-label-md text-on-surface truncate">{t.title}</div>
                      <div className="font-label-sm text-on-surface-variant mt-xs">
                        {meta.title}
                        {t.due_date && <> · due {new Date(t.due_date * 1000).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</>}
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
              Active agents
            </h3>
            <button
              onClick={() => navigate('/opc')}
              className="font-label-sm text-primary hover:underline cursor-pointer"
            >
              Open OPC →
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

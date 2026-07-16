// AgentMessagesPanel — Phase D C3 deliverable.
//
// Surfaces inter-agent SendMessage history from `~/.shannon/agent-messages/`.
// Renders a vertical timeline of recorded messages with from→to, priority,
// content preview, and timestamp. Includes a manual inject form for testing
// until real team agents are wired into the desktop runtime.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { useIntl } from 'react-intl'
import EmptyState from '@/components/ui/empty-state'
import { ListSkeleton } from '@/components/SkeletonLoader'
import * as api from '@/lib/tauri-api'
import type { AgentMessageEntry } from '@/types'

const PRIORITIES = ['low', 'normal', 'high', 'critical'] as const
type Priority = (typeof PRIORITIES)[number]

function priorityBadge(p: string): { bg: string; label: string } {
  switch (p) {
    case 'critical':
      return { bg: 'bg-error/15 text-error border-error/30', label: 'CRIT' }
    case 'high':
      return { bg: 'bg-primary/15 text-primary border-primary/30', label: 'HIGH' }
    case 'low':
      return { bg: 'bg-surface-container-high text-on-surface-variant border-outline-variant', label: 'LOW' }
    default:
      return { bg: 'bg-tertiary/15 text-tertiary border-tertiary/30', label: 'NORM' }
  }
}

function formatTimestamp(ts: number): string {
  return new Date(ts * 1000).toLocaleString()
}

function kindLabel(kind: string): string {
  switch (kind) {
    case 'structured':
      return 'STRUCT'
    case 'protocol':
      return 'PROTO'
    default:
      return 'TEXT'
  }
}

interface AgentMessagesPanelProps {
  /** Optional team filter. If omitted, lists messages across all known teams. */
  team?: string
  /** Max messages to fetch (default 100, server caps at 500). */
  limit?: number
}

export default function AgentMessagesPanel({ team, limit = 100 }: AgentMessagesPanelProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [rows, setRows] = useState<AgentMessageEntry[]>([])
  const [teams, setTeams] = useState<string[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(true)

  // Manual inject form state.
  const [from, setFrom] = useState('lead')
  const [to, setTo] = useState('*')
  const [content, setContent] = useState('')
  const [priority, setPriority] = useState<Priority>('normal')
  const [injecting, setInjecting] = useState(false)

  const activeTeam = team ?? '<adhoc>'

  const reload = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [msgs, allTeams] = await Promise.all([
        api.listAgentMessages(team, limit),
        api.listAgentMessageTeams(),
      ])
      setRows(msgs)
      setTeams(allTeams)
    } catch (e) {
      setError(e instanceof Error ? e.message : t('tasks.agentMessagesPanel.loadFailed'))
    } finally {
      setLoading(false)
    }
  }, [team, limit])

  useEffect(() => {
    void reload()
  }, [reload])

  // Poll for new messages every 5s when auto-refresh is enabled.
  useEffect(() => {
    if (!autoRefresh) return
    const id = window.setInterval(() => void reload(), 5000)
    return () => window.clearInterval(id)
  }, [autoRefresh, reload])

  const handleInject = useCallback(async () => {
    if (!content.trim()) return
    setInjecting(true)
    try {
      await api.recordAgentMessage(activeTeam, from, to, content, priority)
      setContent('')
      await reload()
    } catch (e) {
      setError(e instanceof Error ? e.message : t('tasks.agentMessagesPanel.recordFailed'))
    } finally {
      setInjecting(false)
    }
  }, [activeTeam, from, to, content, priority, reload])

  const empty = useMemo(() => rows.length === 0, [rows])

  return (
    <div className="bg-surface-container-lowest rounded-2xl p-xl border border-outline-variant/30 shadow-sm">
      <div className="flex items-center justify-between mb-md">
        <div className="flex items-center gap-2">
          <span className="material-symbols-outlined icon-md text-on-surface">forum</span>
          <h3 className="font-headline-md text-[18px] font-bold text-on-surface">{t('tasks.agentMessagesPanel.title')}</h3>
          {team && (
            <span className="text-label-sm text-on-surface-variant bg-surface-container px-2 py-0.5 rounded-full border border-outline-variant/20">
              {team}
            </span>
          )}
        </div>
        <div className="flex items-center gap-sm">
          <label className="flex items-center gap-1 text-label-sm text-on-surface-variant cursor-pointer select-none">
            <input
              type="checkbox"
              checked={autoRefresh}
              onChange={e => setAutoRefresh(e.target.checked)}
              className="accent-primary"
            />
            {t('tasks.agentMessagesPanel.autoRefresh')}
          </label>
          <button
            onClick={() => void reload()}
            disabled={loading}
            aria-label={t('tasks.agentMessagesPanel.reloadAria')}
            className="p-xs rounded-lg hover:bg-surface-container text-on-surface-variant cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50"
          >
            <span className="material-symbols-outlined text-[18px]">refresh</span>
          </button>
        </div>
      </div>

      {/* Manual inject form (testing aid). */}
      <details className="mb-md">
        <summary className="cursor-pointer text-label-sm text-on-surface-variant hover:text-primary select-none">
          {t('tasks.agentMessagesPanel.recordTest')}
        </summary>
        <div className="mt-sm flex flex-col gap-sm p-md bg-surface-container-low rounded-xl border border-outline-variant/20">
          <div className="grid grid-cols-1 sm:grid-cols-4 gap-sm">
            <input
              type="text"
              value={from}
              onChange={e => setFrom(e.target.value)}
              placeholder={t('tasks.agentMessagesPanel.fromPlaceholder')}
              aria-label={t('tasks.agentMessagesPanel.fromAria')}
              className="px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
            />
            <input
              type="text"
              value={to}
              onChange={e => setTo(e.target.value)}
              placeholder={t('tasks.agentMessagesPanel.toPlaceholder')}
              aria-label={t('tasks.agentMessagesPanel.toAria')}
              className="px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
            />
            <select
              value={priority}
              onChange={e => setPriority(e.target.value as Priority)}
              aria-label={t('tasks.agentMessagesPanel.priorityAria')}
              className="px-md py-xs rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              {PRIORITIES.map(p => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
            <span className="text-label-sm text-on-surface-variant self-center">
              {t('tasks.agentMessagesPanel.team')}: <span className="text-on-surface">{activeTeam}</span>
            </span>
          </div>
          <textarea
            value={content}
            onChange={e => setContent(e.target.value)}
            placeholder={t('tasks.agentMessagesPanel.contentPlaceholder')}
            rows={2}
            aria-label={t('tasks.agentMessagesPanel.contentAria')}
            className="px-md py-sm rounded-lg border border-outline-variant/50 bg-surface-container-lowest font-body-md text-on-surface resize-none focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30"
          />
          <div className="flex justify-end gap-sm">
            <button
              onClick={() => setContent('')}
              className="px-md py-xs rounded-lg text-on-surface-variant font-label-md hover:bg-surface-container cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
              disabled={injecting}
            >
              {t('tasks.agentMessagesPanel.clear')}
            </button>
            <button
              onClick={() => void handleInject()}
              disabled={injecting || !content.trim()}
              className="px-md py-xs rounded-lg bg-primary text-on-primary font-label-md hover:brightness-110 transition-all cursor-pointer focus:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {injecting ? t('tasks.agentMessagesPanel.sending') : t('tasks.agentMessagesPanel.send')}
            </button>
          </div>
        </div>
      </details>

      {loading ? (
        <ListSkeleton count={4} />
      ) : error ? (
        <div className="rounded-xl border border-error/30 bg-error/10 px-md py-md text-error font-body-md">
          {error}
        </div>
      ) : empty ? (
        <EmptyState
          icon="forum"
          title={t('tasks.agentMessagesPanel.emptyTitle')}
          description={
            team
              ? intl.formatMessage({ id: 'tasks.agentMessagesPanel.emptyDescTeam' }, { team })
              : t('tasks.agentMessagesPanel.emptyDesc')
          }
        />
      ) : (
        <>
          {team === undefined && teams.length > 0 && (
            <p className="text-label-sm text-on-surface-variant mb-sm">
              {intl.formatMessage({ id: 'tasks.agentMessagesPanel.aggregatedFrom' }, { count: teams.length, teams: teams.join(', ') })}
            </p>
          )}
          <ol className="relative pl-md space-y-md before:absolute before:left-[7px] before:top-2 before:bottom-2 before:w-px before:bg-outline-variant/30">
            {rows.map(m => {
              const badge = priorityBadge(m.priority)
              const isBroadcast = m.to === '*'
              return (
                <li key={m.message_id} className="relative pl-md">
                  <span
                    className={`absolute left-0 top-2 w-2.5 h-2.5 rounded-full ${
                      isBroadcast ? 'bg-primary' : 'bg-tertiary'
                    } ring-2 ring-surface-container-lowest`}
                  />
                  <div className="flex flex-wrap items-baseline gap-x-sm gap-y-xs">
                    <span className="font-label-md text-on-surface font-bold">{m.from}</span>
                    <span className="material-symbols-outlined icon-xs text-on-surface-variant">
                      {isBroadcast ? 'campaign' : 'arrow_forward'}
                    </span>
                    <span className="font-label-md text-on-surface">{m.to}</span>
                    <span className={`text-[10px] font-bold px-1.5 py-0.5 rounded border ${badge.bg}`}>
                      {badge.label}
                    </span>
                    <span className="text-[10px] font-bold px-1.5 py-0.5 rounded border border-outline-variant/30 bg-surface-container-low text-on-surface-variant uppercase tracking-wider">
                      {kindLabel(m.content_kind)}
                    </span>
                    <span className="text-label-sm text-on-surface-variant ml-auto">
                      {formatTimestamp(m.timestamp)}
                    </span>
                  </div>
                  <p className="font-body-md text-on-surface-variant mt-xs whitespace-pre-wrap break-words">
                    {m.content_preview}
                  </p>
                </li>
              )
            })}
          </ol>
        </>
      )}
    </div>
  )
}

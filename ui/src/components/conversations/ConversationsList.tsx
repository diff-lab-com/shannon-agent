// ConversationsList — searchable sortable list of all chat sessions.
//
// Designed for users who want to find an old conversation fast: type a few
// letters, sort by date or message count, click to open.

import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useIntl } from 'react-intl'
import type { SessionInfo } from '@/types'

type SortKey = 'recent' | 'messages'
type FilterKey = 'all' | 'agent_run' | 'scheduled' | 'pinned'

interface Props {
  sessions: SessionInfo[]
}

const FILTER_TABS: { key: FilterKey; labelId: string }[] = [
  { key: 'all', labelId: 'conversations.list.filterAll' },
  { key: 'agent_run', labelId: 'conversations.list.filterAgentRun' },
  { key: 'scheduled', labelId: 'conversations.list.filterScheduled' },
  { key: 'pinned', labelId: 'conversations.list.filterPinned' },
]

export default function ConversationsList({ sessions }: Props) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<SortKey>('recent')
  const [filter, setFilter] = useState<FilterKey>('all')

  const counts = useMemo(() => ({
    all: sessions.length,
    agent_run: sessions.filter(s => s.is_agent_run).length,
    scheduled: sessions.filter(s => s.is_scheduled).length,
    pinned: sessions.filter(s => s.is_pinned).length,
  }), [sessions])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    const matchesFilter = (s: SessionInfo) => {
      if (filter === 'agent_run') return !!s.is_agent_run
      if (filter === 'scheduled') return !!s.is_scheduled
      if (filter === 'pinned') return !!s.is_pinned
      return true
    }
    const matchesQuery = (s: SessionInfo) => !q || (s.title ?? '').toLowerCase().includes(q)
    const list = sessions.filter(s => matchesFilter(s) && matchesQuery(s))
    if (sort === 'recent') list.sort((a, b) => b.created_at - a.created_at)
    else list.sort((a, b) => b.message_count - a.message_count)
    return list
  }, [sessions, query, sort, filter])

  const groupByDate = useMemo(() => {
    const groups: Record<string, SessionInfo[]> = {}
    for (const s of filtered) {
      const d = new Date(s.created_at)
      const key = d.toLocaleDateString(undefined, { weekday: 'long', year: 'numeric', month: 'long', day: 'numeric' })
      ;(groups[key] ??= []).push(s)
    }
    return Object.entries(groups)
  }, [filtered])

  return (
    <div className="flex-1 overflow-y-auto px-lg py-lg">
      {/* Filter tabs */}
      <div role="tablist" aria-label={t('conversations.list.filterAria')} className="flex items-center gap-xs mb-md flex-wrap">
        {FILTER_TABS.map(tab => {
          const active = filter === tab.key
          const label = t(tab.labelId)
          return (
            <button
              key={tab.key}
              role="tab"
              aria-selected={active}
              aria-label={intl.formatMessage({ id: 'conversations.list.tabAria' }, { label })}
              onClick={() => setFilter(tab.key)}
              className={`px-md py-xs rounded-full text-[12px] font-label-md cursor-pointer transition-colors ${
                active
                  ? 'bg-primary text-on-primary'
                  : 'bg-surface-container-low text-on-surface-variant hover:bg-surface-container-high'
              }`}
            >
              {label} <span className={active ? 'text-on-primary/80' : 'text-outline'}>({counts[tab.key]})</span>
            </button>
          )
        })}
      </div>

      {/* Controls */}
      <div className="flex items-center gap-sm mb-lg sticky top-0 bg-background/80 backdrop-blur-sm py-sm z-10">
        <div className="flex-1 relative">
          <span className="material-symbols-outlined absolute left-md top-1/2 -translate-y-1/2 text-outline-variant text-[18px]">search</span>
          <input
            type="search"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder={t('conversations.list.searchPlaceholder')}
            aria-label={t('conversations.list.searchAria')}
            className="w-full pl-xl pr-md py-sm bg-surface-container-lowest border border-outline-variant/40 rounded-xl text-body-sm focus:ring-2 focus:ring-primary outline-none"
          />
        </div>
        <div className="flex items-center gap-xs">
          <label htmlFor="conv-sort" className="font-label-sm text-on-surface-variant">{t('conversations.list.sort')}</label>
          <select
            id="conv-sort"
            value={sort}
            onChange={e => setSort(e.target.value as SortKey)}
            className="px-md py-sm bg-surface-container-lowest border border-outline-variant/40 rounded-xl text-body-sm cursor-pointer focus:ring-2 focus:ring-primary outline-none"
            aria-label={t('conversations.list.sortAria')}
          >
            <option value="recent">{t('conversations.list.sortRecent')}</option>
            <option value="messages">{t('conversations.list.sortMessages')}</option>
          </select>
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="p-xl rounded-xl bg-surface-container-lowest border border-dashed border-outline-variant/50 flex flex-col items-center text-center mt-xl">
          <span className="material-symbols-outlined text-outline-variant mb-sm">search_off</span>
          <p className="font-body-md text-on-surface-variant">
            {query
              ? intl.formatMessage({ id: 'conversations.list.noMatching' }, { query })
              : filter === 'all'
                ? t('conversations.list.noneYet')
                : intl.formatMessage({ id: 'conversations.list.noFilterType' }, { label: t(FILTER_TABS.find(tab => tab.key === filter)?.labelId ?? 'conversations.list.filterAll').toLowerCase() })}
          </p>
        </div>
      ) : (
        <div className="space-y-xl">
          {groupByDate.map(([date, items]) => (
            <section key={date}>
              <h3 className="font-label-md text-on-surface-variant uppercase tracking-wider mb-sm">{date}</h3>
              <ul className="space-y-sm">
                {items.map(s => (
                  <li key={s.id}>
                    <button
                      onClick={() => navigate('/chat')}
                      className="w-full text-left p-md rounded-xl bg-surface-container-lowest border border-outline-variant/30 hover:border-primary/40 hover:bg-surface-container-low transition-colors cursor-pointer flex items-center gap-md group"
                    >
                      <span className="material-symbols-outlined text-on-surface-variant group-hover:text-primary transition-colors">chat_bubble</span>
                      <div className="flex-1 min-w-0">
                        <div className="font-label-md text-on-surface truncate">{s.title || t('conversations.list.untitled')}</div>
                        <div className="font-label-sm text-on-surface-variant mt-xs">
                          {intl.formatMessage({ id: 'conversations.list.messageCount' }, { count: s.message_count })}
                          {' · '}
                          {new Date(s.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                        </div>
                      </div>
                      <span className="material-symbols-outlined text-outline-variant opacity-0 group-hover:opacity-100 transition-opacity">chevron_right</span>
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  )
}

// ConversationsList — searchable sortable list of all chat sessions.
//
// Designed for users who want to find an old conversation fast: type a few
// letters, sort by date or message count, click to open.

import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import type { SessionInfo } from '@/types'

type SortKey = 'recent' | 'messages'

interface Props {
  sessions: SessionInfo[]
}

export default function ConversationsList({ sessions }: Props) {
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [sort, setSort] = useState<SortKey>('recent')

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    const list = q
      ? sessions.filter(s => (s.title ?? '').toLowerCase().includes(q))
      : sessions.slice()
    if (sort === 'recent') list.sort((a, b) => b.created_at - a.created_at)
    else list.sort((a, b) => b.message_count - a.message_count)
    return list
  }, [sessions, query, sort])

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
      {/* Controls */}
      <div className="flex items-center gap-sm mb-lg sticky top-0 bg-background/80 backdrop-blur-sm py-sm z-10">
        <div className="flex-1 relative">
          <span className="material-symbols-outlined absolute left-md top-1/2 -translate-y-1/2 text-outline-variant text-[18px]">search</span>
          <input
            type="search"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search conversations..."
            aria-label="Search conversations"
            className="w-full pl-xl pr-md py-sm bg-surface-container-lowest border border-outline-variant/40 rounded-xl text-body-sm focus:ring-2 focus:ring-primary outline-none"
          />
        </div>
        <div className="flex items-center gap-xs">
          <label htmlFor="conv-sort" className="font-label-sm text-on-surface-variant">Sort</label>
          <select
            id="conv-sort"
            value={sort}
            onChange={e => setSort(e.target.value as SortKey)}
            className="px-md py-sm bg-surface-container-lowest border border-outline-variant/40 rounded-xl text-body-sm cursor-pointer focus:ring-2 focus:ring-primary outline-none"
            aria-label="Sort conversations"
          >
            <option value="recent">Most recent</option>
            <option value="messages">Most messages</option>
          </select>
        </div>
      </div>

      {filtered.length === 0 ? (
        <div className="p-xl rounded-xl bg-surface-container-lowest border border-dashed border-outline-variant/50 flex flex-col items-center text-center mt-xl">
          <span className="material-symbols-outlined text-outline-variant mb-sm">search_off</span>
          <p className="font-body-md text-on-surface-variant">
            {query ? `No conversations matching "${query}"` : 'No conversations yet.'}
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
                        <div className="font-label-md text-on-surface truncate">{s.title || 'Untitled chat'}</div>
                        <div className="font-label-sm text-on-surface-variant mt-xs">
                          {s.message_count} message{s.message_count === 1 ? '' : 's'}
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

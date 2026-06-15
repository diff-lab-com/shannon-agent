import { useState, useEffect, useMemo } from 'react'
import { toast } from 'sonner'
import * as api from '@/lib/tauri-api'
import type { HookEventInfo } from '@/types'

// Events that are defined in the backend enum but never actually emitted in
// production (per Phase E E4 audit, commit 03343c1). Hide them from the catalog
// so users don't wire routines to events that will never fire.
const DEAD_EVENTS = new Set([
  'UserPromptExpansion',
  'ConfigChange',
  'InstructionsLoaded',
  'Elicitation',
  'ElicitationResult',
])

const CATEGORY_ORDER = [
  'Tools',
  'Session',
  'Prompt',
  'Context',
  'Agents',
  'Worktree',
  'Permissions',
] as const

export default function Hooks() {
  const [events, setEvents] = useState<HookEventInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [query, setQuery] = useState('')
  const [activeCategory, setActiveCategory] = useState<string>('all')

  useEffect(() => {
    (async () => {
      try {
        setEvents(await api.listHookEvents())
      } catch (e) {
        console.warn('Failed to load hook events:', e)
        toast.error('Failed to load hook catalog')
      }
      setLoading(false)
    })()
  }, [])

  const liveEvents = useMemo(() => events.filter(e => !DEAD_EVENTS.has(e.name)), [events])

  const categories = useMemo(() => {
    const seen = new Set(liveEvents.map(e => e.category))
    return CATEGORY_ORDER.filter(c => seen.has(c))
  }, [liveEvents])

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase()
    return liveEvents.filter(e => {
      if (activeCategory !== 'all' && e.category !== activeCategory) return false
      if (!q) return true
      return (
        e.name.toLowerCase().includes(q) ||
        e.description.toLowerCase().includes(q) ||
        e.payload_fields.some(f => f.toLowerCase().includes(q))
      )
    })
  }, [liveEvents, query, activeCategory])

  return (
    <div className="p-xl space-y-lg max-w-5xl">
      <header>
        <h1 className="font-headline-lg text-on-surface mb-xs">Triggers</h1>
        <p className="font-body-md text-on-surface-variant max-w-2xl">
          Shannon can run shell commands on {liveEvents.length || 'many'} lifecycle events. Browse the catalog, then head to{' '}
          <code className="font-mono bg-surface-container-high px-xs rounded text-[12px]">/routines</code>{' '}
          to wire a command to one of them.
        </p>
      </header>

      <div className="flex items-center gap-md flex-wrap">
        <div className="relative flex-1 min-w-[260px]">
          <span className="material-symbols-outlined text-[18px] text-on-surface-variant absolute left-md top-1/2 -translate-y-1/2">search</span>
          <input
            type="search"
            value={query}
            onChange={e => setQuery(e.target.value)}
            placeholder="Search events, descriptions, or payload fields"
            aria-label="Search hook events"
            className="w-full pl-xl pr-md py-sm bg-surface border border-outline-variant/50 rounded-lg focus:ring-2 focus:ring-primary outline-none font-body-sm"
          />
        </div>
        <div className="flex items-center gap-xs flex-wrap" role="tablist" aria-label="Filter by category">
          <CategoryChip active={activeCategory === 'all'} onClick={() => setActiveCategory('all')}>
            All
          </CategoryChip>
          {categories.map(c => (
            <CategoryChip key={c} active={activeCategory === c} onClick={() => setActiveCategory(c)}>
              {c}
            </CategoryChip>
          ))}
        </div>
      </div>

      {loading ? (
        <div className="flex items-center justify-center py-xl">
          <span className="material-symbols-outlined text-[32px] text-primary animate-spin">progress_activity</span>
        </div>
      ) : filtered.length === 0 ? (
        <div className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-xl text-center">
          <span className="material-symbols-outlined text-[48px] text-outline-variant block mb-sm">search_off</span>
          <p className="font-headline-md text-on-surface mb-xs">No events match</p>
          <p className="font-body-sm text-on-surface-variant">Try a different search or category.</p>
        </div>
      ) : (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-sm">
          {filtered.map(e => (
            <article
              key={e.name}
              className="bg-surface-container-lowest border border-outline-variant/30 rounded-xl p-md flex flex-col gap-sm"
            >
              <header className="flex items-center justify-between gap-sm">
                <code className="font-mono font-headline-md text-on-surface truncate">{e.name}</code>
                <span className="text-[10px] font-mono px-xs py-[2px] rounded bg-primary-container/40 text-primary shrink-0">{e.category}</span>
              </header>
              <p className="font-body-sm text-on-surface-variant">{e.description}</p>
              <div className="flex items-center gap-xs flex-wrap mt-auto pt-xs">
                {e.payload_fields.map(f => (
                  <code key={f} className="text-[11px] font-mono px-xs py-[2px] rounded bg-surface-container-high text-on-surface-variant">
                    {f}
                  </code>
                ))}
              </div>
            </article>
          ))}
        </div>
      )}
    </div>
  )
}

function CategoryChip({ active, onClick, children }: { active: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      role="tab"
      aria-pressed={active}
      onClick={onClick}
      className={`px-md py-xs rounded-full text-[12px] font-label-md cursor-pointer transition-colors focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary ${
        active
          ? 'bg-primary text-on-primary'
          : 'bg-surface-container-low text-on-surface-variant hover:bg-surface-container-high'
      }`}
    >
      {children}
    </button>
  )
}

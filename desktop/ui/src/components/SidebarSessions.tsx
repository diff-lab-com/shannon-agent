// SessionsSection — the single session rail for the whole app (U1, D1=A).
// Lives in the app sidebar (desktop docked + mobile drawer share this
// component). Owns: search (client title filter + debounced backend
// full-text), drag-to-reorder + pin (both persisted to localStorage),
// inline rename, export/print, and delete-with-confirm. The former Chat-page
// session sidebar was removed (U1); this list is its replacement.
//
// Persisted keys:
//   shannon-sessions-order  — Record<sessionId, index> written on drag reorder
//   shannon-sessions-pinned — string[] of pinned session ids

import { useState, useCallback, useEffect, useMemo, useRef } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { exportSessionAsMarkdown, printSession } from '@/lib/sessionActions'
import type { SessionInfo } from '@/types'
import DeleteSessionModal from '@/pages/chat/DeleteSessionModal'
import HighlightText from './HighlightText'

const SESSIONS_ORDER_KEY = 'shannon-sessions-order'
const SESSIONS_PINNED_KEY = 'shannon-sessions-pinned'

function readOrderOverride(): Record<string, number> {
  if (typeof window === 'undefined') return {}
  try {
    const raw = window.localStorage.getItem(SESSIONS_ORDER_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch { return {} }
}

function readPinned(): ReadonlySet<string> {
  if (typeof window === 'undefined') return new Set()
  try {
    const raw = window.localStorage.getItem(SESSIONS_PINNED_KEY)
    return new Set(raw ? JSON.parse(raw) : [])
  } catch { return new Set() }
}

function persist(key: string, value: unknown) {
  try { window.localStorage.setItem(key, JSON.stringify(value)) } catch { /* noop */ }
}

interface SessionsSectionProps {
  sessions: SessionInfo[]
  currentSessionId: string | null
  switchSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  closeMobile?: () => void
}

export function SessionsSection({ sessions, currentSessionId, switchSession, renameSession, deleteSession, closeMobile }: SessionsSectionProps) {
  const t = useT()
  const navigate = useNavigate()
  const [query, setQuery] = useState('')
  const [backendHits, setBackendHits] = useState<SessionInfo[] | null>(null)
  const [draggedId, setDraggedId] = useState<string | null>(null)
  const [menuFor, setMenuFor] = useState<string | null>(null)
  const [editingId, setEditingId] = useState<string | null>(null)
  const [editTitle, setEditTitle] = useState('')
  const [deleteTarget, setDeleteTarget] = useState<string | null>(null)
  const [orderOverride, setOrderOverride] = useState<Record<string, number>>(readOrderOverride)
  const [pinnedIds, setPinnedIds] = useState<ReadonlySet<string>>(readPinned)
  // U5: touch long-press (500ms) opens the ⋯ menu; the click that follows a
  // completed long-press must not also switch the session.
  const longPressTimer = useRef<number | null>(null)
  const suppressClickRef = useRef(false)
  const clearLongPress = useCallback(() => {
    if (longPressTimer.current !== null) {
      clearTimeout(longPressTimer.current)
      longPressTimer.current = null
    }
  }, [])
  useEffect(() => clearLongPress, [clearLongPress])
  const startLongPress = useCallback((id: string) => {
    clearLongPress()
    longPressTimer.current = window.setTimeout(() => {
      suppressClickRef.current = true
      setMenuFor(id)
    }, 500)
  }, [clearLongPress])

  const untitled = t('chat.session.untitled')

  // Sort: pinned sessions first (U4 priority rule), then explicit drag-order
  // override (ascending), then created_at desc.
  const sorted = useMemo(() => {
    return [...sessions].sort((a, b) => {
      const pa = pinnedIds.has(a.id) ? 0 : 1
      const pb = pinnedIds.has(b.id) ? 0 : 1
      if (pa !== pb) return pa - pb
      const oa = orderOverride[a.id] ?? Number.MAX_SAFE_INTEGER
      const ob = orderOverride[b.id] ?? Number.MAX_SAFE_INTEGER
      if (oa !== ob) return oa - ob
      return b.created_at - a.created_at
    })
  }, [sessions, orderOverride, pinnedIds])

  // Debounced backend full-text search. Backend matches title first, then
  // message content. Short queries fall back to a client-side title filter
  // (cheaper, instant feedback, no IPC round-trip).
  useEffect(() => {
    const q = query.trim()
    if (q.length < 3) {
      setBackendHits(null)
      return
    }
    let cancelled = false
    const handle = setTimeout(() => {
      api.searchSessions(q)
        .then(hits => { if (!cancelled) setBackendHits(hits) })
        .catch(e => {
          console.warn('searchSessions failed, falling back to client filter:', e)
          if (!cancelled) setBackendHits(null)
        })
    }, 250)
    return () => { cancelled = true; clearTimeout(handle) }
  }, [query])

  const filtered = useMemo(() => {
    const q = query.trim()
    if (!q) return sorted
    if (backendHits === null) {
      const ql = q.toLowerCase()
      return sorted.filter(s => (s.title || '').toLowerCase().includes(ql))
    }
    // Backend hits: keep our own ordering (pins, drag order), and surface
    // sessions the backend knows about but the local list doesn't.
    const hitIds = new Set(backendHits.map(h => h.id))
    const known = new Set(sessions.map(s => s.id))
    return [...sorted.filter(s => hitIds.has(s.id)), ...backendHits.filter(h => !known.has(h.id))]
  }, [sorted, query, backendHits, sessions])

  const persistOrder = useCallback((next: Record<string, number>) => {
    setOrderOverride(next)
    persist(SESSIONS_ORDER_KEY, next)
  }, [])

  const handleDrop = useCallback((targetId: string) => {
    if (!draggedId || draggedId === targetId) return
    setDraggedId(null)
    const ids = sorted.map(s => s.id)
    const fromIdx = ids.indexOf(draggedId)
    const toIdx = ids.indexOf(targetId)
    if (fromIdx === -1 || toIdx === -1) return
    // Rebuild order map based on new sequence
    const reordered = [...ids]
    reordered.splice(fromIdx, 1)
    reordered.splice(toIdx, 0, draggedId)
    const next: Record<string, number> = {}
    reordered.forEach((id, idx) => { next[id] = idx })
    persistOrder(next)
  }, [draggedId, sorted, persistOrder])

  // U5: keyboard alternative to drag reorder — Alt+↑/↓ on a focused row
  // swaps it with its neighbor and writes through the same order-override
  // path as handleDrop. Inert mid-search: reordering a filtered subset is
  // ambiguous (the neighbor may be filtered out).
  const moveRow = useCallback((id: string, dir: -1 | 1) => {
    const ids = sorted.map(s => s.id)
    const fromIdx = ids.indexOf(id)
    const toIdx = fromIdx + dir
    if (fromIdx === -1 || toIdx < 0 || toIdx >= ids.length) return
    ;[ids[fromIdx], ids[toIdx]] = [ids[toIdx], ids[fromIdx]]
    const next: Record<string, number> = {}
    ids.forEach((sid, idx) => { next[sid] = idx })
    persistOrder(next)
  }, [sorted, persistOrder])

  const handleRowKeyDown = useCallback((e: React.KeyboardEvent<HTMLButtonElement>, id: string) => {
    if (!e.altKey) return
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault()
      if (query.trim()) return
      moveRow(id, e.key === 'ArrowUp' ? -1 : 1)
    }
  }, [moveRow, query])

  const togglePin = useCallback((id: string) => {
    setPinnedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id); else next.add(id)
      persist(SESSIONS_PINNED_KEY, [...next])
      return next
    })
  }, [])

  const startRename = useCallback((session: SessionInfo) => {
    setEditingId(session.id)
    setEditTitle(session.title)
  }, [])

  const commitRename = useCallback((session: SessionInfo) => {
    const next = editTitle.trim()
    setEditingId(null)
    if (next && next !== session.title) void renameSession(session.id, next)
  }, [editTitle, renameSession])

  const handleSwitch = useCallback((id: string) => {
    if (suppressClickRef.current) { suppressClickRef.current = false; return }
    void switchSession(id)
    navigate('/chat')
    closeMobile?.()
  }, [switchSession, navigate, closeMobile])

  const menuItems = useCallback((session: SessionInfo): DropdownMenuItem[] => {
    const pinned = pinnedIds.has(session.id)
    return [
      { id: 'rename', label: t('chat.session.rename'), icon: 'edit', onSelect: () => startRename(session) },
      { id: 'pin', label: pinned ? t('chat.session.unpin') : t('chat.session.pin'), icon: 'push_pin', onSelect: () => togglePin(session.id) },
      { id: 'export', label: t('chat.session.export'), icon: 'download', onSelect: () => { void exportSessionAsMarkdown(session.id, sessions, t) } },
      { id: 'print', label: t('chat.session.print'), icon: 'print', onSelect: () => { void printSession(session.id, sessions, t) } },
      { id: 'delete', label: t('chat.session.delete'), icon: 'delete', destructive: true, onSelect: () => setDeleteTarget(session.id) },
    ]
  }, [pinnedIds, t, sessions, startRename, togglePin])

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex items-center justify-between px-2 mb-xs shrink-0">
        <span className="font-label-sm text-label-sm text-on-surface-variant uppercase tracking-wider">
          {t('sidebar.sessions.title')}
        </span>
        <span className="font-label-sm text-label-sm text-outline-variant">
          {filtered.length}{filtered.length !== sessions.length ? `/${sessions.length}` : ''}
        </span>
      </div>
      <input
        type="search"
        value={query}
        onChange={e => setQuery(e.target.value)}
        placeholder={t('sidebar.sessions.search.placeholder')}
        aria-label={t('sidebar.sessions.search.aria')}
        className="w-full mb-xs px-2 py-1 rounded-md bg-surface-container-lowest border border-outline-variant/30 font-label-md text-label-md text-on-surface placeholder:text-outline-variant focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 shrink-0"
      />
      <ScrollArea className="flex-1 min-h-0">
        {filtered.length === 0 ? (
          <div className="px-2 py-3 text-center font-label-sm text-label-sm text-outline-variant">
            {t('sidebar.sessions.noResults')}
          </div>
        ) : (
          <div className="space-y-0.5 pr-1" role="list" aria-label={t('sidebar.sessions.list.aria')}>
            {filtered.map((session) => {
              const isActive = session.id === currentSessionId
              const isEditing = editingId === session.id
              const isMenuOpen = menuFor === session.id
              return (
                <div
                  key={session.id}
                  role="listitem"
                  draggable={!isEditing}
                  onDragStart={() => setDraggedId(session.id)}
                  onDragOver={e => e.preventDefault()}
                  onDrop={() => handleDrop(session.id)}
                  onTouchStart={() => startLongPress(session.id)}
                  onTouchEnd={clearLongPress}
                  onTouchMove={clearLongPress}
                  onTouchCancel={clearLongPress}
                  className={cn('group relative flex items-center gap-1', draggedId === session.id && 'opacity-40')}
                >
                  {isEditing ? (
                    <Input
                      className="w-full text-label-md py-1 px-2 rounded-lg bg-surface-container-lowest border-primary/40"
                      value={editTitle}
                      onChange={e => setEditTitle(e.target.value)}
                      onBlur={() => commitRename(session)}
                      onKeyDown={e => {
                        if (e.key === 'Enter') commitRename(session)
                        else if (e.key === 'Escape') setEditingId(null)
                      }}
                      aria-label={t('chat.session.rename')}
                      autoFocus
                    />
                  ) : (
                    <>
                      <button
                        type="button"
                        aria-current={isActive ? 'page' : undefined}
                        aria-label={t('chat.session.aria', { title: session.title || untitled })}
                        title={session.title || untitled}
                        onClick={() => handleSwitch(session.id)}
                        onKeyDown={e => handleRowKeyDown(e, session.id)}
                        className={cn(
                          'flex-1 min-w-0 text-left px-3 py-2 rounded-lg font-label-md text-label-md transition-all duration-200 flex items-center gap-2 cursor-pointer select-none',
                          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
                          isActive
                            ? 'bg-primary/10 text-primary font-bold'
                            : 'text-on-surface-variant hover:bg-surface-container-low hover:text-primary',
                        )}
                      >
                        {/* U5: affordance only — the grip shows on hover/focus
                            (Alt+↑/↓ or drag does the work), cutting per-row
                            visual noise. */}
                        <span className="material-symbols-outlined text-[14px] text-outline-variant shrink-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100" aria-hidden="true">drag_indicator</span>
                        {pinnedIds.has(session.id) && (
                          <span className="material-symbols-outlined text-[14px] text-primary shrink-0" aria-hidden="true">push_pin</span>
                        )}
                        <span className="flex-1 truncate">
                          <HighlightText text={session.title || untitled} query={query.trim()} />
                        </span>
                      </button>
                      <div className="relative shrink-0">
                        <Button
                          variant="ghost"
                          size="icon-xs"
                          aria-label={t('chat.session.menu.aria', { title: session.title || untitled })}
                          className={cn(
                            'rounded hover:bg-surface-container text-on-surface-variant hover:text-primary transition-opacity focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none',
                            isMenuOpen ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
                          )}
                          onClick={e => { e.stopPropagation(); setMenuFor(isMenuOpen ? null : session.id) }}
                        >
                          <span className="material-symbols-outlined text-[16px]">more_horiz</span>
                        </Button>
                        {isMenuOpen && (
                          <DropdownMenu
                            open
                            onClose={() => setMenuFor(null)}
                            items={menuItems(session)}
                            align="end"
                            className="w-40 min-w-0"
                            ariaLabel={t('chat.session.menu.aria', { title: session.title || untitled })}
                          />
                        )}
                      </div>
                    </>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </ScrollArea>

      <DeleteSessionModal
        t={t}
        deleteTarget={deleteTarget}
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget) { void deleteSession(deleteTarget); setDeleteTarget(null) }
        }}
      />
    </div>
  )
}

// SessionsSection — the single session rail for the whole app (U1, D1=A).
// Lives in the app sidebar (desktop docked + mobile drawer share this
// component). Owns: search (client title filter + debounced backend
// full-text), drag-to-reorder + pin (both persisted to localStorage),
// inline rename, export/print, and delete-with-confirm. The former Chat-page
// session sidebar was removed (U1); this list is its replacement.
//
// P0 (ZCode delta ②/④): the rail is also a run monitor — every row carries
// a live running dot + elapsed badge (from SessionActivity), and the list
// supports two grouping views, 按项目 (Codex/Claude "project" mental model,
// default) and 按时间 (today/yesterday/this-week/earlier, the ZCode mental
// model). Grouping preference persists to localStorage.
//
// Persisted keys:
//   shannon-sessions-order    — Record<sessionId, index> written on drag reorder
//   shannon-sessions-pinned   — string[] of pinned session ids
//   shannon-sessions-grouping — 'project' | 'time'

import { useState, useCallback, useEffect, useMemo, useRef, Fragment } from 'react'
import { useNavigate } from 'react-router-dom'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { exportSessionAsMarkdown, printSession } from '@/lib/sessionActions'
import { toastError } from '@/lib/errorToast'
import type { GoalRunDto, ScheduledRoutine, SessionActivity, SessionInfo } from '@/types'
import DeleteSessionModal from '@/pages/chat/DeleteSessionModal'
import HighlightText from './HighlightText'

const SESSIONS_ORDER_KEY = 'shannon-sessions-order'
const SESSIONS_PINNED_KEY = 'shannon-sessions-pinned'
const SESSIONS_GROUPING_KEY = 'shannon-sessions-grouping'
const SESSIONS_FOLDED_KEY = 'shannon-sessions-folded'
// Batch F2: project registry — display names for derived project folders
// (the first slice of "projects as entities"; deeper registries need engine
// support). dirname → display name.
const PROJECTS_KEY = 'shannon-projects'

type ProjectRegistry = Record<string, string>

function readProjectRegistry(): ProjectRegistry {
  if (typeof window === 'undefined') return {}
  try {
    const raw = window.localStorage.getItem(PROJECTS_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch { return {} }
}

// IA (2026-09 review): two grouping modes — by project (folders nest
// their conversations, the ZCode mental model) and by session (one flat
// list sorted by recency). The session mode is what users want when
// they're hunting for "the chat I had an hour ago"; project mode wins
// once they have more than ~10 conversations.
// Batch F6: a third 智能 mode pins the actionable sessions first —
// 运行中 → 需要关注（审批/失败）→ 全部 — a deterministic stand-in for the
// reference's AI grouping that still answers "what needs me right now".
type GroupingMode = 'project' | 'session' | 'smart'

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

function readGrouping(): GroupingMode {
  if (typeof window === 'undefined') return 'project'
  try {
    const raw = window.localStorage.getItem(SESSIONS_GROUPING_KEY)
    return raw === 'session' || raw === 'smart' ? raw : 'project'
  } catch { return 'project' }
}

function readFolded(): ReadonlySet<string> {
  if (typeof window === 'undefined') return new Set()
  try {
    const raw = window.localStorage.getItem(SESSIONS_FOLDED_KEY)
    return new Set(raw ? JSON.parse(raw) : [])
  } catch { return new Set() }
}

function persist(key: string, value: unknown) {
  try { window.localStorage.setItem(key, JSON.stringify(value)) } catch { /* noop */ }
}

function persistGrouping(mode: GroupingMode) {
  // Raw string (not JSON-encoded) — readGrouping compares bare values.
  try { window.localStorage.setItem(SESSIONS_GROUPING_KEY, mode) } catch { /* noop */ }
}

/** Compact elapsed label for a running session: 42s · 8m · 1h12m. */
function formatElapsed(ms: number): string {
  const sec = Math.max(0, Math.floor(ms / 1000))
  if (sec < 60) return `${sec}s`
  const min = Math.floor(sec / 60)
  if (min < 60) return `${min}m`
  return `${Math.floor(min / 60)}h${min % 60}m`
}

/**
 * Batch B1: compact relative-time label for an idle session's last activity
 * (刚刚 / 17小时 / 12天 — the ZCode rail pattern). Beyond a week it degrades
 * to a short numeric date. `t` is the useT translator. Exported for tests.
 */
export function formatRelativeTime(
  ts: number | undefined,
  now: number,
  t: (id: string, values?: Record<string, number>) => string,
): string {
  if (!ts || ts <= 0) return ''
  const sec = Math.max(0, Math.floor((now - ts) / 1000))
  if (sec < 60) return t('sidebar.sessions.lastActivity.now')
  const min = Math.floor(sec / 60)
  if (min < 60) return t('sidebar.sessions.lastActivity.minutes', { n: min })
  const hr = Math.floor(min / 60)
  if (hr < 24) return t('sidebar.sessions.lastActivity.hours', { n: hr })
  const day = Math.floor(hr / 24)
  if (day < 7) return t('sidebar.sessions.lastActivity.days', { n: day })
  try {
    return new Intl.DateTimeFormat(undefined, { month: 'numeric', day: 'numeric' }).format(ts)
  } catch { return '' }
}

/** Project name from a session/working dir — the last path segment.
 *  Exported for the dock's document breadcrumb (batch D2). */
export function projectOf(s: { working_dir?: string | null }): string | null {
  const dir = s.working_dir?.trim()
  if (!dir) return null
  const parts = dir.split('/').filter(Boolean)
  return parts.length > 0 ? parts[parts.length - 1] : null
}

interface SessionGroup {
  key: string
  icon: string
  label: string
  sessions: SessionInfo[]
  /** Project groups render as collapsible folder rows (ZCode 项目 tree). */
  isProject?: boolean
}

interface SessionsSectionProps {
  sessions: SessionInfo[]
  /** P0 sidebar telemetry: live run state per session id. */
  sessionActivity: Record<string, SessionActivity>
  /** P2-⑥: goal runs keyed by the session they own (badge + iterations). */
  goalRunsBySession: Record<string, GoalRunDto>
  currentSessionId: string | null
  switchSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  closeMobile?: () => void
}

export function SessionsSection({ sessions, sessionActivity, goalRunsBySession = {}, currentSessionId, switchSession, renameSession, deleteSession, closeMobile }: SessionsSectionProps) {
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
  const [grouping, setGrouping] = useState<GroupingMode>(readGrouping)
  // ZCode 项目 tree: folded project folders persist; the active session's
  // project always auto-expands so the current conversation stays visible.
  const [foldedProjects, setFoldedProjects] = useState<ReadonlySet<string>>(readFolded)
  // Batch F2: project display-name registry (localStorage) + inline rename.
  const [projectNames, setProjectNames] = useState<ProjectRegistry>(readProjectRegistry)
  const [editingProject, setEditingProject] = useState<string | null>(null)
  const [projectNameDraft, setProjectNameDraft] = useState('')
  // Batch F1-v1: enabled scheduled routines surface as an 自动化 section on
  // the rail (clock icon + next fire). Nesting them under projects needs the
  // engine to expose working_dir on routines — deferred until that contract
  // exists.
  const [routines, setRoutines] = useState<ScheduledRoutine[]>([])
  // Wall-clock tick that drives the elapsed badges while anything runs.
  const [nowTick, setNowTick] = useState(() => Date.now())
  // U5: touch long-press (500ms) opens the ⋯ menu; the click that follows a
  // completed long-press must not also switch the session.
  const longPressTimer = useRef<number | null>(null)
  const suppressClickRef = useRef(false)

  const anyRunning = useMemo(
    () => Object.values(sessionActivity).some(a => a.running) || sessions.some(s => s.running === true),
    [sessionActivity, sessions],
  )

  // Refresh "now" immediately when activity changes, then keep the elapsed
  // badges ticking while a run is live. Batch B1: when nothing runs, a slow
  // 30s tick keeps the idle rows' time-ago badges (刚刚/17小时/12天) fresh.
  useEffect(() => {
    setNowTick(Date.now())
  }, [sessionActivity])
  useEffect(() => {
    if (anyRunning) {
      const id = window.setInterval(() => setNowTick(Date.now()), 5000)
      return () => window.clearInterval(id)
    }
    if (sessions.length === 0) return
    const id = window.setInterval(() => setNowTick(Date.now()), 30000)
    return () => window.clearInterval(id)
  }, [anyRunning, sessions.length])

  const setGroupingPersisted = useCallback((mode: GroupingMode) => {
    setGrouping(mode)
    persistGrouping(mode)
  }, [])

  const toggleFold = useCallback((key: string) => {
    setFoldedProjects(prev => {
      const next = new Set(prev)
      if (next.has(key)) next.delete(key); else next.add(key)
      try { window.localStorage.setItem(SESSIONS_FOLDED_KEY, JSON.stringify([...next])) } catch { /* noop */ }
      return next
    })
  }, [])

  // The active session's project folder always stays expanded — switching to
  // a conversation in a folded project reveals it instead of hiding the row.
  const activeProject = currentSessionId
    ? projectOf(sessions.find(s => s.id === currentSessionId) ?? {})
    : null
  useEffect(() => {
    if (!activeProject) return
    setFoldedProjects(prev => {
      if (!prev.has(activeProject)) return prev
      const next = new Set(prev)
      next.delete(activeProject)
      return next
    })
  }, [activeProject])

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

  // Batch F1-v1: enabled scheduled routines for the rail's 自动化 section.
  // Fully defensive: engines without the command, partial test mocks, or
  // undefined payloads all just hide the section.
  useEffect(() => {
    let cancelled = false
    const load = () => {
      try {
        api.listScheduledTasks()
          .then(tasks => {
            if (!cancelled) setRoutines(Array.isArray(tasks) ? tasks.filter(r => r.enabled) : [])
          })
          .catch(() => { /* section stays hidden */ })
      } catch { /* section stays hidden */ }
    }
    load()
    const id = window.setInterval(load, 60_000)
    return () => {
      cancelled = true
      window.clearInterval(id)
    }
  }, [])

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

  // Grouping (P0-④). null = render flat. While searching the list stays
  // flat (matches hit ranking); project mode stays flat while there is at
  // most one project (small lists stay uncluttered).
  //
  // Session mode (2026-09): a single flat list sorted by recency — no
  // today/yesterday/this-week/earlier buckets. The buckets read as
  // redundant once the user knows the list is "all my sessions, newest
  // first"; pinning the active session to the top of an un-bucketed list
  // is the ZCode-style mental model.
  const groups = useMemo<SessionGroup[] | null>(() => {
    if (query.trim()) return null
    if (grouping === 'smart') {
      // Batch F6: deterministic smart rail — actionable first. Each session
      // lands in exactly one section; the remainder stays a flat "最近" list
      // (the session-mode mental model, preserved underneath).
      const running = filtered.filter(s => sessionActivity[s.id]?.running === true || s.running === true)
      const attention = filtered.filter(s => {
        const a = sessionActivity[s.id]
        if (!a || a.running) return false
        if (s.running === true) return false
        return a.awaitingApproval === true || a.failed === true
      })
      const attended = new Set([...running, ...attention].map(s => s.id))
      const rest = filtered.filter(s => !attended.has(s.id))
      const out: SessionGroup[] = []
      if (running.length > 0) out.push({ key: 'smart-running', icon: 'play_circle', label: t('sidebar.groups.running'), sessions: running })
      if (attention.length > 0) out.push({ key: 'smart-attention', icon: 'priority', label: t('sidebar.groups.attention'), sessions: attention })
      if (rest.length > 0) out.push({ key: 'smart-rest', icon: 'history', label: t('sidebar.groups.recent'), sessions: rest })
      return out.length > 0 ? out : null
    }
    if (grouping === 'project') {
      const distinct = new Set(filtered.map(projectOf))
      if (distinct.size <= 1) return null
      const buckets = new Map<string, SessionInfo[]>()
      for (const s of filtered) {
        const key = projectOf(s) ?? ''
        if (!buckets.has(key)) buckets.set(key, [])
        buckets.get(key)!.push(s)
      }
      return [...buckets.entries()].map(([key, list]) => ({
        key,
        icon: 'folder',
        label: key === '' ? t('sidebar.sessions.project.untitled') : (projectNames[key] ?? key),
        sessions: list,
        isProject: true,
      }))
    }
    // Session mode — flat, no headers. The list itself is the order.
    return null
  }, [filtered, grouping, query, t, sessionActivity, projectNames])

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
  // path as handleDrop. Inert mid-search and in time grouping: reordering a
  // filtered/bucketed subset is ambiguous (the neighbor may be filtered out
  // or live in another day bucket).
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
      if (query.trim() || grouping === 'session') return
      moveRow(id, e.key === 'ArrowUp' ? -1 : 1)
    }
  }, [moveRow, query, grouping])

  const togglePin = useCallback((id: string) => {
    setPinnedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id); else next.add(id)
      persist(SESSIONS_PINNED_KEY, [...next])
      return next
    })
  }, [])

  // Batch F2: persist a project display name (rename via double-click).
  const commitProjectRename = useCallback((dirKey: string) => {
    const next = projectNameDraft.trim()
    setEditingProject(null)
    if (!next || !dirKey) return
    setProjectNames(prev => {
      const reg = { ...prev, [dirKey]: next }
      persist(PROJECTS_KEY, reg)
      return reg
    })
  }, [projectNameDraft])

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
      // §4.14 — visualize the session's turns/tools/token-cost curve.
      { id: 'timeline', label: t('chat.session.timeline'), icon: 'timeline', onSelect: () => navigate(`/timeline/${session.id}`) },
      // P1-1 — dedicated window for this session (backend dedupes by
      // focusing an existing `session-<uuid>` window).
      { id: 'open-in-window', label: t('chat.session.openInWindow'), icon: 'open_in_new', onSelect: () => { void api.openSessionWindow(session.id).catch(e => toastError(t('chat.session.openInWindow.failed'), e)) } },
      { id: 'export', label: t('chat.session.export'), icon: 'download', onSelect: () => { void exportSessionAsMarkdown(session.id, sessions, t) } },
      { id: 'print', label: t('chat.session.print'), icon: 'print', onSelect: () => { void printSession(session.id, sessions, t) } },
      { id: 'delete', label: t('chat.session.delete'), icon: 'delete', destructive: true, onSelect: () => setDeleteTarget(session.id) },
    ]
  }, [pinnedIds, t, sessions, startRename, togglePin, navigate])

  const renderGroupHeader = (group: SessionGroup) => {
    if (group.isProject) {
      // ZCode 项目 tree row: a folder button (chevron + name + count) that
      // folds/unfolds its conversations. The active session's project is
      // force-expanded by the effect above. Batch F2: double-click renames
      // the project in place (localStorage registry).
      const isFolded = foldedProjects.has(group.key)
      if (editingProject === group.key) {
        return (
          <Input
            className="w-full text-label-sm py-1 px-2 rounded-lg bg-surface-container-lowest border-primary/40"
            value={projectNameDraft}
            onChange={e => setProjectNameDraft(e.target.value)}
            onBlur={() => commitProjectRename(group.key)}
            onKeyDown={e => {
              if (e.key === 'Enter') commitProjectRename(group.key)
              else if (e.key === 'Escape') setEditingProject(null)
            }}
            aria-label={t('chat.session.rename')}
            autoFocus
          />
        )
      }
      return (
        <button
          type="button"
          role="presentation"
          aria-expanded={!isFolded}
          title={group.label}
          onClick={() => toggleFold(group.key)}
          onDoubleClick={() => {
            if (!group.key) return
            setEditingProject(group.key)
            setProjectNameDraft(group.label)
          }}
          className="w-full flex items-center gap-1.5 px-3 pt-2 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 hover:text-primary transition-colors min-w-0 cursor-pointer"
        >
          <span
            className="material-symbols-outlined text-[14px] shrink-0 transition-transform duration-150"
            style={{ transform: isFolded ? 'rotate(-90deg)' : 'rotate(0deg)' }}
            aria-hidden="true"
          >
            expand_more
          </span>
          <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">{group.icon}</span>
          <span className="truncate flex-1 min-w-0 text-left">{group.label}</span>
          <span className="font-mono text-[10px] tabular-nums text-on-surface-variant/70 shrink-0">{group.sessions.length}</span>
        </button>
      )
    }
    return (
      <div
        key={`group-${group.key}`}
        role="presentation"
        className="px-3 pt-2 pb-1 font-label-sm text-[10px] font-bold uppercase tracking-wider text-on-surface-variant/80 flex items-center gap-1.5 min-w-0"
      >
        <span className="material-symbols-outlined text-[12px] shrink-0" aria-hidden="true">{group.icon}</span>
        <span className="truncate min-w-0">{group.label}</span>
      </div>
    )
  }

  const renderRow = (session: SessionInfo) => {
    const isActive = session.id === currentSessionId
    const isEditing = editingId === session.id
    const isMenuOpen = menuFor === session.id
    const activity = sessionActivity[session.id]
    const isRunning = activity?.running === true || session.running === true
    const goalRun = goalRunsBySession[session.id]
    const elapsed = isRunning && activity?.startedAt != null
      ? formatElapsed(nowTick - activity.startedAt)
      : null
    // Batch B1/B2 rail semantics: a live green pulse while running; otherwise
    // amber while a permission prompt pends, red while the last run failed,
    // and finally a relative time-ago badge (ZCode 刚刚/17小时/12天 pattern).
    const idleState = !isRunning && activity?.awaitingApproval
      ? 'approval'
      : !isRunning && activity?.failed ? 'failed' : null
    const agoBadge = !isRunning
      ? formatRelativeTime(session.updated_at ?? session.created_at, nowTick, t)
      : ''
    return (
      <div
        key={session.id}
        role="listitem"
        draggable={!isEditing && grouping !== 'session'}
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
              data-testid={`desktop-session-row-${session.id}`}
              aria-label={t('chat.session.aria', { title: session.title || untitled })}
              title={session.title || untitled}
              onClick={() => handleSwitch(session.id)}
              onKeyDown={e => handleRowKeyDown(e, session.id)}
              className={cn(
                'flex-1 min-w-0 text-left px-3 py-2 rounded-lg font-label-md text-label-md transition-all duration-200 flex items-center gap-2 cursor-pointer select-none whitespace-nowrap',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30',
                isActive
                  ? 'bg-primary-container text-on-primary-container font-bold'
                  : 'text-on-surface-variant hover:bg-surface-container-low hover:text-primary',
              )}
            >
              {/* U5: affordance only — the grip shows on hover/focus
                  (Alt+↑/↓ or drag does the work), cutting per-row
                  visual noise. Superseded while the run dot is live. */}
              <span className="material-symbols-outlined text-[14px] text-outline-variant shrink-0 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100" aria-hidden="true">drag_indicator</span>
              {isRunning && (
                <span
                  role="img"
                  aria-label={activity?.activeTool
                    ? t('sidebar.sessions.running.tool', { tool: activity.activeTool })
                    : t('sidebar.sessions.running.badge')}
                  title={activity?.activeTool
                    ? t('sidebar.sessions.running.tool', { tool: activity.activeTool })
                    : t('sidebar.sessions.running.badge')}
                  className="w-2 h-2 rounded-full bg-secondary animate-pulse shrink-0"
                />
              )}
              {!isRunning && idleState === 'approval' && (
                <span
                  role="img"
                  aria-label={t('sidebar.sessions.awaitingApproval.badge')}
                  title={t('sidebar.sessions.awaitingApproval.badge')}
                  className="w-2 h-2 rounded-full bg-warning shrink-0"
                />
              )}
              {!isRunning && idleState === 'failed' && (
                <span
                  role="img"
                  aria-label={t('sidebar.sessions.error.badge')}
                  title={t('sidebar.sessions.error.badge')}
                  className="w-2 h-2 rounded-full bg-error shrink-0"
                />
              )}
              {pinnedIds.has(session.id) && (
                // U8: filled pin marks the active state; the menu
                // action stays outlined.
                <span className="material-symbols-outlined text-[14px] text-primary shrink-0" style={{ fontVariationSettings: "'FILL' 1" }} aria-hidden="true">push_pin</span>
              )}
              {/* P2-⑥: goal-run badge — a session a goal run owns shows the
                  run's iteration progress right on the rail. */}
              {goalRun && (
                <span
                  role="img"
                  aria-label={t('sidebar.sessions.goal.badge.aria', {
                    title: goalRun.title,
                    done: goalRun.iterations,
                    total: goalRun.maxTurns ?? goalRun.iterations,
                  })}
                  title={t('sidebar.sessions.goal.badge.aria', {
                    title: goalRun.title,
                    done: goalRun.iterations,
                    total: goalRun.maxTurns ?? goalRun.iterations,
                  })}
                  className={cn(
                    'flex items-center gap-[2px] shrink-0 rounded px-[3px]',
                    // E10: a stalled run (repeated no-progress strikes) gets a
                    // red badge so the user can spot it on the rail itself.
                    (goalRun.stallStrikes ?? 0) > 0 && 'bg-error/15',
                  )}
                >
                  <span
                    className={cn('material-symbols-outlined text-[13px]', (goalRun.stallStrikes ?? 0) > 0 ? 'text-error' : 'text-primary')}
                    style={{ fontVariationSettings: goalRun.status === 'running' ? "'FILL' 1" : undefined }}
                    aria-hidden="true"
                  >
                    {(goalRun.stallStrikes ?? 0) > 0 ? 'warning' : 'flag'}
                  </span>
                  <span className="font-mono text-[10px] tabular-nums text-on-surface" aria-hidden="true">
                    {goalRun.iterations}/{goalRun.maxTurns ?? goalRun.iterations}
                  </span>
                </span>
              )}
              <span className="flex-1 truncate">
                <HighlightText text={session.title || untitled} query={query.trim()} />
              </span>
              {/* P0-②: live elapsed badge while running; Batch B1: relative
                  time-ago on idle rows — the rail answers "which session is
                  live, how long, and when was the rest last active". */}
              {elapsed ? (
                <span className="font-mono text-[10px] tabular-nums text-secondary shrink-0" aria-hidden="true">
                  {elapsed}
                </span>
              ) : agoBadge ? (
                <span
                  role="img"
                  aria-label={t('sidebar.sessions.lastActivity.aria', { time: agoBadge })}
                  title={t('sidebar.sessions.lastActivity.aria', { time: agoBadge })}
                  className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0"
                >
                  {agoBadge}
                </span>
              ) : null}
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
  }

  return (
    <div className="flex flex-col h-full min-h-0">
      <div className="flex items-center justify-between px-2 mb-xs shrink-0 gap-1 min-w-0">
        <span className="font-label-sm text-label-sm text-on-surface-variant uppercase tracking-wider truncate">
          {t('sidebar.sessions.title')}
        </span>
        <span className="font-label-sm text-label-sm text-on-surface-variant shrink-0">
          {filtered.length}{filtered.length !== sessions.length ? `/${sessions.length}` : ''}
        </span>
        {/* P0-④ grouping view switch — ZCode 项目|时间 style labeled
            segmented control (按项目文件夹 vs 按会话顺序). Renamed
            "time → session" in 2026-09: the second mode is just a flat
            session list sorted by recency — labelling it "session" reads
            more honestly to the user than abstract "time". */}
        <div role="group" aria-label={t('sidebar.sessions.grouping.aria')} className="flex items-center rounded-md bg-surface-container-low p-0.5 shrink-0">
          {([
            { mode: 'project' as const, icon: 'folder', label: t('sidebar.sessions.grouping.project') },
            { mode: 'smart' as const, icon: 'auto_awesome', label: t('sidebar.sessions.grouping.smart') },
            { mode: 'session' as const, icon: 'view_list', label: t('sidebar.sessions.grouping.session') },
          ]).map(opt => (
            <button
              key={opt.mode}
              type="button"
              aria-pressed={grouping === opt.mode}
              aria-label={opt.label}
              title={opt.label}
              onClick={() => setGroupingPersisted(opt.mode)}
              className={cn(
                'flex items-center gap-0.5 px-1.5 py-0.5 rounded font-label-xs transition-colors cursor-pointer whitespace-nowrap',
                grouping === opt.mode
                  ? 'bg-primary text-on-primary shadow-sm'
                  : 'text-on-surface-variant hover:text-primary',
              )}
            >
              <span className="material-symbols-outlined text-[12px]" aria-hidden="true">{opt.icon}</span>
              <span className="hidden xl:inline">{opt.label}</span>
            </button>
          ))}
        </div>
      </div>
      <input
        type="search"
        value={query}
        onChange={e => setQuery(e.target.value)}
        placeholder={t('sidebar.sessions.search.placeholder')}
        aria-label={t('sidebar.sessions.search.aria')}
        className="w-full mb-xs px-2 py-1 rounded-md bg-surface-container-lowest border border-outline-variant/30 font-label-md text-label-md text-on-surface placeholder:text-on-surface-variant/70 focus-visible:border-primary focus-visible:ring-2 focus-visible:ring-primary/30 shrink-0 min-w-0"
      />
      {/* Batch F1-v1: the rail's 自动化 section — enabled scheduled routines
          with their next fire. Project-nesting needs engine working_dir on
          routines (see plan batch F). */}
      {!query.trim() && routines.length > 0 && (
        <div className="mb-xs" data-testid="sidebar-automations">
          <div className="flex items-center gap-1.5 px-3 pt-1 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 min-w-0">
            <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">event_repeat</span>
            <span className="truncate flex-1 min-w-0">{t('sidebar.automations.title')}</span>
            <span className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0">{routines.length}</span>
          </div>
          {routines.slice(0, 3).map(r => {
            const soon = r.next_fire_at != null && r.next_fire_at - nowTick < 3600_000
            return (
              <button
                key={r.id}
                type="button"
                onClick={() => navigate('/tasks')}
                className="w-full flex items-center gap-2 px-3 py-1.5 rounded-lg font-label-md text-label-md text-on-surface-variant hover:bg-surface-container-low hover:text-primary transition-colors cursor-pointer min-w-0"
                title={r.name}
              >
                <span className={cn('material-symbols-outlined text-[13px] shrink-0', soon ? 'text-warning' : 'text-on-surface-variant')} aria-hidden="true">schedule</span>
                <span className="truncate flex-1 min-w-0 text-left">{r.name}</span>
                {soon && (
                  <span className="font-label-xs px-1 py-[1px] rounded bg-warning/15 text-warning shrink-0" role="img" aria-label={t('sidebar.automations.soon')}>
                    {t('sidebar.automations.soon')}
                  </span>
                )}
              </button>
            )
          })}
          {routines.length > 3 && (
            <button
              type="button"
              onClick={() => navigate('/tasks')}
              className="w-full px-3 py-1 text-left font-label-xs text-on-surface-variant hover:text-primary hover:underline cursor-pointer"
            >
              {t('sidebar.automations.more', { n: routines.length - 3 })}
            </button>
          )}
        </div>
      )}
      <ScrollArea className="flex-1 min-h-0">
        {filtered.length === 0 ? (
          <div className="px-2 py-3 text-center font-label-sm text-label-sm text-on-surface-variant">
            {t('sidebar.sessions.noResults')}
          </div>
        ) : groups === null ? (
          <div className="space-y-0.5 pr-1" role="list" aria-label={t('sidebar.sessions.list.aria')}>
            {filtered.map(renderRow)}
          </div>
        ) : (
          <div className="space-y-0.5 pr-1" role="list" aria-label={t('sidebar.sessions.list.aria')}>
            {groups.map(group => (
              <Fragment key={group.key}>
                {renderGroupHeader(group)}
                {/* Project conversations nest under their folder (ZCode
                    项目 tree); folded projects collapse their rows. */}
                {!(group.isProject && foldedProjects.has(group.key)) && (
                  <div className={group.isProject ? 'pl-4' : undefined}>
                    {group.sessions.map(renderRow)}
                  </div>
                )}
              </Fragment>
            ))}
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

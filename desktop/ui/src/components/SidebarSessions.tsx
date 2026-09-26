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
//   shannon-sessions-folded   — string[] of folded project keys (full paths)
//
// P-U2 (2026-09): project names/appearance/archive moved to the engine
// registry (projects.db via list/rename/set_project_appearance/archive/
// unarchive_project). The old localStorage `shannon-projects` registry is
// read once on mount, migrated through rename_project, then removed.

import { useState, useCallback, useEffect, useMemo, useRef, Fragment } from 'react'
import { useNavigate } from 'react-router-dom'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { cn } from '@/lib/utils'
import { useT } from '@/i18n'
import * as api from '@/lib/tauri-api'
import { exportSessionAsMarkdown, printSession } from '@/lib/sessionActions'
import { toastError } from '@/lib/errorToast'
import type {
  ArchivedSessionRow,
  GoalRunDto,
  ProjectRecord,
  ScheduledRoutine,
  SessionActivity,
  SessionInfo,
} from '@/types'
import DeleteSessionModal from '@/pages/chat/DeleteSessionModal'
import HighlightText from './HighlightText'

const SESSIONS_ORDER_KEY = 'shannon-sessions-order'
const SESSIONS_PINNED_KEY = 'shannon-sessions-pinned'
const SESSIONS_GROUPING_KEY = 'shannon-sessions-grouping'
const SESSIONS_FOLDED_KEY = 'shannon-sessions-folded'
// B4 P2-4: per-list render cap — the rail used to map every session into
// DOM (thousands of rows on old accounts). Each list (flat / project /
// smart section / archived lens) renders the newest CAP rows plus the
// active one, with a 显示全部 expander; the sections themselves are
// untouched.
const SESSIONS_VISIBLE_CAP = 50
// P-U2: legacy localStorage project-name registry (dirname → display name).
// The engine registry (projects.db, P-E3) replaced it; this key is only
// read once on mount to migrate custom names into the registry, then
// removed. Never written again.
const PROJECTS_KEY = 'shannon-projects'

/** P-U2 color palette for the project ⋯ menu's swatch popover. Values are
 *  theme token references (the chart series palette) stored in the registry
 *  row's `color` — they adapt to the active theme and dodge the design-token
 *  guardrails; the icon field stays API-only (no icon UI by ruling). */
const PROJECT_COLORS = [
  'var(--chart-series-4)', // red
  'var(--chart-series-3)', // amber
  'var(--chart-series-2)', // green
  'var(--chart-series-1)', // blue
  'var(--chart-series-5)', // purple
  'var(--chart-series-6)', // pink
]

/** Normalize a working-dir/project path into a stable group key: trimmed,
 *  trailing separators stripped (the P-U1 upgrade — the key is the FULL
 *  path, so /a/x and /b/x are different projects even though they share a
 *  tail segment). */
function normalizePathKey(dir: string | null | undefined): string | null {
  const d = dir?.trim()
  if (!d) return null
  const stripped = d.replace(/[\\/]+$/, '')
  return stripped || '/'
}

/** Tail segment of a path for default display labels (slash or backslash
 *  separated — the engine canonicalizes, but never assume the separator).
 *  Exported for the P-U3 deep-link chips' fallback label (registry name
 *  ?? tail). */
export function pathTail(dir: string): string {
  const parts = dir.split(/[\\/]/).filter(Boolean)
  return parts.length > 0 ? parts[parts.length - 1] : dir
}

/** Invoke a tauri-api wrapper defensively: partial test mocks (and engines
 *  without a command) throw synchronously on the missing property — turn
 *  that into a rejection so callers can share one catch path. */
function callSafe<T>(fn: () => Promise<T>): Promise<T> {
  try { return fn() } catch (e) { return Promise.reject(e) }
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

/** P-U1 grouping key: the FULL working dir (trailing separators stripped),
 *  not its tail segment — /w/x and /h/x are different projects even though
 *  both render as "x". Kept alongside the untouched `projectOf` (the dock's
 *  breadcrumb only needs the tail). null = session has no working dir. */
export function projectKeyOf(s: { working_dir?: string | null }): string | null {
  return normalizePathKey(s.working_dir)
}

interface SessionGroup {
  key: string
  icon: string
  label: string
  sessions: SessionInfo[]
  /** P-U1: enabled routines whose working_dir maps to this project; they
   *  render interleaved with the sessions (recency = next_fire_at). */
  routines?: ScheduledRoutine[]
  /** P-U1: merged session/routine render order for project groups. */
  rows?: GroupRow[]
  /** P-U2: registry appearance + emptiness (registry-only placeholder). */
  color?: string | null
  isEmpty?: boolean
  /** Project groups render as collapsible folder rows (ZCode 项目 tree). */
  isProject?: boolean
}

/** One renderable row inside a project group: a session or a nested routine. */
type GroupRow =
  | { kind: 'session'; session: SessionInfo }
  | { kind: 'routine'; routine: ScheduledRoutine }

/** Group-internal sort timestamp: sessions by last activity, routines by
 *  their next fire (the plan's "例行用 next_fire_at"). */
function rowTime(row: GroupRow): number {
  if (row.kind === 'routine') return row.routine.next_fire_at ?? 0
  return row.session.updated_at ?? row.session.created_at ?? 0
}

/** Merge routines into a project's session list by recent activity. Sessions
 *  keep the rail's own order (pins → drag order → recency, i.e. the incoming
 *  `sessions` order is preserved — drag/pin semantics stay intact); each
 *  routine slots in before the first session that is strictly less recent
 *  than its next_fire_at. */
function mergeGroupRows(sessions: SessionInfo[], routines: ScheduledRoutine[]): GroupRow[] {
  const rows: GroupRow[] = sessions.map(session => ({ kind: 'session' as const, session }))
  const sorted = [...routines]
    .sort((a, b) => (b.next_fire_at ?? 0) - (a.next_fire_at ?? 0))
    .map(routine => ({ kind: 'routine' as const, routine }))
  const merged: GroupRow[] = []
  let ri = 0
  for (const row of rows) {
    while (ri < sorted.length && rowTime(sorted[ri]) >= rowTime(row)) merged.push(sorted[ri++])
    merged.push(row)
  }
  while (ri < sorted.length) merged.push(sorted[ri++])
  return merged
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

/** B4 P2-6: what the delete dialog is pointed at. `permanent` selects the
 *  archived-session 永久删除 variant; `title` feeds the confirm copy so the
 *  dialog names its target. */
interface DeleteTarget {
  id: string
  title: string
  permanent?: boolean
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
  const [deleteTarget, setDeleteTarget] = useState<DeleteTarget | null>(null)
  // B4 P2-6: true while the confirm's delete call is in flight (buttons
  // disabled); a failed delete drops it and keeps the dialog open.
  const [deletePending, setDeletePending] = useState(false)
  // B4 P2-4: lists the user expanded past the initial render cap
  // ('flat' | group key | 'archived').
  const [expandedLists, setExpandedLists] = useState<ReadonlySet<string>>(new Set())
  const [orderOverride, setOrderOverride] = useState<Record<string, number>>(readOrderOverride)
  const [pinnedIds, setPinnedIds] = useState<ReadonlySet<string>>(readPinned)
  const [grouping, setGrouping] = useState<GroupingMode>(readGrouping)
  // ZCode 项目 tree: folded project folders persist; the active session's
  // project always auto-expands so the current conversation stays visible.
  const [foldedProjects, setFoldedProjects] = useState<ReadonlySet<string>>(readFolded)
  // P-U2: the engine project registry (~/.shannon/projects.db) is the single
  // source of project names/appearance/archive state. Loaded at mount and
  // refreshed whenever the active list changes (sessions-updated) or after a
  // local mutation; the old localStorage registry is migrated once (below).
  const [projects, setProjects] = useState<ProjectRecord[]>([])
  // Bumps the registry load when the one-time localStorage migration lands —
  // renames committed after the initial fetch would otherwise be invisible
  // until the next sessions-updated.
  const [projectsReload, setProjectsReload] = useState(0)
  const [editingProject, setEditingProject] = useState<string | null>(null)
  const [projectNameDraft, setProjectNameDraft] = useState('')
  // P-U2: the project header's ⋯ menu (mirrors the session-row menu) and its
  // color-swatch popover (only one open at a time).
  const [projectMenuFor, setProjectMenuFor] = useState<string | null>(null)
  const [colorPickerFor, setColorPickerFor] = useState<string | null>(null)
  // Batch F1-v1: enabled scheduled routines surface as an 自动化 section on
  // the rail. P-U1: routines WITH a working_dir nest into their project
  // group in project mode; the standalone section below lists only the
  // unhoused ones (no working_dir) and hides entirely when empty.
  const [routines, setRoutines] = useState<ScheduledRoutine[]>([])
  // 卡A archive: the collapsed 已归档 section at the bottom of the rail.
  // Rows come from the backend's archived lens; the load refires whenever
  // the active list changes (archive/unarchive both emit sessions-updated,
  // which the parent refreshes from) so restore/archive reflect instantly.
  const [archivedRows, setArchivedRows] = useState<ArchivedSessionRow[]>([])
  // 卡A 收尾: collapsed by default, but when every session is archived (the
  // active list is empty) the section opens by default — the onboarding
  // EmptyState no longer covers the rail, so restore must be reachable
  // without an extra click. An explicit toggle wins over the derived
  // default; the existing collapse interaction and aria-expanded stay.
  const [archivedOpenOverride, setArchivedOpen] = useState<boolean | null>(null)
  const archivedOpen = archivedOpenOverride ?? sessions.length === 0
  // P-U2: 已归档项目 section — same collapsed lens as the sessions' one, but
  // always collapsed by default (a project has no "open" action to lose).
  const [archivedProjectsOpen, setArchivedProjectsOpen] = useState(false)
  // Wall-clock tick that drives the elapsed badges while anything runs.
  const [nowTick, setNowTick] = useState(() => Date.now())
  // U5: touch long-press (500ms) opens the ⋯ menu; the click that follows a
  // completed long-press must not also switch the session / fold the project.
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
  // (Keys are full paths since P-U1 — same keys the fold set persists.)
  const activeProject = currentSessionId
    ? projectKeyOf(sessions.find(s => s.id === currentSessionId) ?? {})
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
  const startLongPress = useCallback((id: string, kind: 'session' | 'project') => {
    clearLongPress()
    longPressTimer.current = window.setTimeout(() => {
      suppressClickRef.current = true
      if (kind === 'project') setProjectMenuFor(id)
      else setMenuFor(id)
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

  // P-U2: load the engine project registry — archived rows included (the
  // 已归档项目 section reads them). Fully defensive: engines without the
  // command or partial test mocks just leave the registry empty (name falls
  // back to the path tail). Refires with the active list (sessions-updated
  // fires around session mutations) and after the localStorage migration.
  //
  // A3 polish: a FAILED refetch keeps the previous registry rows — wiping
  // projects to [] on a transient rejection would erase the optimistic
  // rename/appearance/archive updates applied on top of them. Only the
  // first load (never succeeded) may land the empty default.
  const projectsLoadedRef = useRef(false)
  useEffect(() => {
    let cancelled = false
    const load = () => {
      try {
        api.listProjects(true)
          .then(rows => {
            if (cancelled) return
            projectsLoadedRef.current = true
            setProjects(Array.isArray(rows) ? rows : [])
          })
          .catch(() => {
            // Keep whatever we have — the empty default only sticks when no
            // successful load ever landed.
            if (!cancelled && !projectsLoadedRef.current) setProjects([])
          })
      } catch { /* registry stays empty */ }
    }
    load()
    return () => { cancelled = true }
  }, [sessions, projectsReload])

  // P-U2: one-time migration of the legacy localStorage project registry
  // (`shannon-projects`: Record<dir, name>) into the engine registry — each
  // custom name is committed via rename_project (skipped when the registry
  // already carries that name), then the localStorage key is removed so
  // group names come only from the registry from here on. Best-effort per
  // entry: a failed rename never blocks the retirement of the key.
  const migratedProjectsRef = useRef(false)
  useEffect(() => {
    if (migratedProjectsRef.current) return
    migratedProjectsRef.current = true
    let raw: string | null = null
    try { raw = window.localStorage.getItem(PROJECTS_KEY) } catch { raw = null }
    if (raw === null) return
    let legacy: Record<string, unknown> = {}
    try { legacy = raw ? JSON.parse(raw) : {} } catch { legacy = {} }
    const entries = Object.entries(legacy).filter(
      (entry): entry is [string, string] => typeof entry[1] === 'string' && entry[1].trim().length > 0,
    )
    void (async () => {
      let known: ProjectRecord[] = []
      try {
        const rows = await callSafe(() => api.listProjects(true))
        if (Array.isArray(rows)) known = rows
      } catch { /* treat as empty — every entry just gets renamed */ }
      // Legacy F2 keys were path TAILS (the old projectOf grouping), not
      // full dirs. Absolute keys migrate verbatim; a tail key resolves
      // against the registry only when it names exactly one project —
      // anything else is skipped rather than registered as a junk path.
      const looksAbsolute = (p: string) => p.startsWith('/') || /^[a-zA-Z]:[\\/]/.test(p)
      for (const [dir, name] of entries) {
        if (known.some(p => p.path === dir && p.name === name)) continue
        let target: string | null = looksAbsolute(dir) ? dir : null
        if (!target) {
          const matches = known.filter(p => pathTail(normalizePathKey(p.path) ?? p.path) === dir)
          if (matches.length === 1) target = matches[0].path
        }
        if (!target) continue
        try { await callSafe(() => api.renameProject(target, name)) } catch { /* best-effort */ }
      }
      // The localStorage registry is retired unconditionally once read —
      // valid entries were migrated, malformed ones held nothing of value.
      try { window.localStorage.removeItem(PROJECTS_KEY) } catch { /* noop */ }
      if (entries.length > 0) setProjectsReload(n => n + 1)
    })()
  }, [])

  // 卡A: refresh the archived lens alongside the active list — the backend
  // emits sessions-updated on every archive/unarchive, which re-renders
  // `sessions` here. Fully defensive: engines without the command or test
  // mocks just keep the section empty (hidden).
  useEffect(() => {
    let cancelled = false
    try {
      api.listArchivedSessions()
        .then(rows => { if (!cancelled) setArchivedRows(Array.isArray(rows) ? rows : []) })
        .catch(() => { if (!cancelled) setArchivedRows([]) })
    } catch { /* section stays hidden */ }
    return () => { cancelled = true }
  }, [sessions])

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

  // P-U2 derived registry views: name/appearance lookups by normalized path,
  // plus the active/archived splits for the tree and the 已归档项目 section.
  const registryByKey = useMemo(() => {
    const m = new Map<string, ProjectRecord>()
    for (const p of projects) {
      const key = normalizePathKey(p.path)
      if (key) m.set(key, p)
    }
    return m
  }, [projects])
  const activeRegistryProjects = useMemo(
    () => projects.filter(p => !p.archivedAtMs && normalizePathKey(p.path)),
    [projects],
  )
  const archivedRegistryProjects = useMemo(
    () => projects.filter(p => p.archivedAtMs && normalizePathKey(p.path)),
    [projects],
  )
  // P-U1: housed routines (working_dir set) nest into their project group;
  // unhoused ones keep the standalone 自动化 section.
  const housedRoutines = useMemo(
    () => routines.filter(r => !!normalizePathKey(r.working_dir)),
    [routines],
  )
  const unhousedRoutines = useMemo(
    () => routines.filter(r => !normalizePathKey(r.working_dir)),
    [routines],
  )
  // I3 (review fix): only the project lens nests housed routines into their
  // project tree. In the time/smart lenses the tree is not rendered at all,
  // so the standalone 自动化 section must carry ALL enabled routines (housed
  // ∪ unhoused) — pre-branch, every enabled routine was always visible
  // there. In project lens it stays unhoused-only (housed ones live in the
  // tree above).
  const lensRoutines = useMemo(
    () => (grouping === 'project' ? unhousedRoutines : routines),
    [grouping, routines, unhousedRoutines],
  )

  // Grouping (P0-④). null = render flat. While searching the list stays
  // flat (matches hit ranking); project mode stays flat while there is at
  // most one project (small lists stay uncluttered).
  //
  // P-U1/P-U2 (2026-09): the tree also engages when there is anything
  // tree-worthy beyond sessions — enabled routines with a working_dir, or
  // registry projects (which may be empty). Only a rail of sessions in a
  // single project (and nothing else) stays flat, exactly as before.
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
      // P-U1: the group key is the FULL working dir (trailing separators
      // normalized away) — no more collapsing /w/x and /h/x into one "x".
      const keyOf = (s: SessionInfo) => projectKeyOf(s) ?? ''
      const distinct = new Set(filtered.map(keyOf))
      const routineKeyOf = (r: ScheduledRoutine) => normalizePathKey(r.working_dir)
      // Tree-worthiness: >1 session groups, or any nested automation, or any
      // registry project — otherwise today's flat rendering stands.
      if (distinct.size <= 1 && housedRoutines.length === 0 && activeRegistryProjects.length === 0) return null
      const buckets = new Map<string, SessionInfo[]>()
      for (const s of filtered) {
        const key = keyOf(s)
        if (!buckets.has(key)) buckets.set(key, [])
        buckets.get(key)!.push(s)
      }
      const routinesByKey = new Map<string, ScheduledRoutine[]>()
      for (const r of housedRoutines) {
        const key = routineKeyOf(r)!
        if (!routinesByKey.has(key)) routinesByKey.set(key, [])
        routinesByKey.get(key)!.push(r)
      }
      const labelOf = (key: string) =>
        key === '' ? t('sidebar.sessions.project.untitled') : (registryByKey.get(key)?.name ?? pathTail(key))
      const out: SessionGroup[] = []
      const seen = new Set<string>()
      // I5 (review fix): a bucket whose key maps to a REGISTRY-ARCHIVED row
      // is skipped — the project must not render twice (once from its live
      // sessions/routines, once in 已归档项目). Its sessions stay reachable
      // via the time/smart lenses and search; the archived section keeps
      // the 恢复 action.
      const archivedKeys = new Set(
        archivedRegistryProjects
          .map(p => normalizePathKey(p.path))
          .filter((k): k is string => !!k),
      )
      const pushGroup = (key: string, list: SessionInfo[], rs: ScheduledRoutine[]) => {
        if (archivedKeys.has(key)) return
        seen.add(key)
        const isEmpty = list.length === 0 && rs.length === 0
        out.push({
          key,
          icon: 'folder',
          label: labelOf(key),
          sessions: list,
          routines: rs,
          rows: isEmpty ? [] : mergeGroupRows(list, rs),
          color: registryByKey.get(key)?.color ?? null,
          isEmpty,
          isProject: true,
        })
      }
      // Session-derived groups first (rail order), routines mixed in.
      for (const [key, list] of buckets) pushGroup(key, list, routinesByKey.get(key) ?? [])
      // Routines whose project has no sessions yet (registry row optional).
      for (const [key, rs] of routinesByKey) {
        if (!seen.has(key)) pushGroup(key, [], rs)
      }
      // Registry-only projects (P-U2): empty placeholder group.
      for (const p of activeRegistryProjects) {
        const key = normalizePathKey(p.path)!
        if (!seen.has(key)) pushGroup(key, [], [])
      }
      return out
    }
    // Session mode — flat, no headers. The list itself is the order.
    return null
  }, [filtered, grouping, query, t, sessionActivity, registryByKey, housedRoutines, activeRegistryProjects, archivedRegistryProjects])

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

  // Apply a registry record returned by a mutation (rename/appearance/
  // archive/unarchive) to local state so the UI reflects it instantly; the
  // next registry fetch will agree with it.
  const applyProjectRecord = useCallback((rec: ProjectRecord) => {
    setProjects(prev => {
      const key = normalizePathKey(rec.path)
      const idx = prev.findIndex(p => normalizePathKey(p.path) === key)
      if (idx === -1) return [...prev, rec]
      const next = [...prev]
      next[idx] = rec
      return next
    })
  }, [])

  // B4 P2-7: drop a session's entries from the persisted order/pinned maps
  // when it is really deleted — deleted ids used to sit there forever. On
  // archive nothing is pruned: archive is reversible, archived ids are
  // inert in both maps (archived rows never render in the main rail), and
  // pruning would silently destroy the user's pin/drag-order across an
  // archive→unarchive round-trip.
  const pruneSessionMeta = useCallback((id: string) => {
    setOrderOverride(prev => {
      if (!(id in prev)) return prev
      const next = { ...prev }
      delete next[id]
      persist(SESSIONS_ORDER_KEY, next)
      return next
    })
    setPinnedIds(prev => {
      if (!prev.has(id)) return prev
      const next = new Set(prev)
      next.delete(id)
      persist(SESSIONS_PINNED_KEY, [...next])
      return next
    })
  }, [])

  // B4 P2-4: bounded rendering. Each list shows its first CAP rows — the
  // active session is always kept visible even when it sorts beyond the
  // cap — unless the user expanded the list. DOM stays O(cap) per section.
  const visibleSlice = useCallback(<T extends { id: string }>(list: T[], key: string): T[] => {
    if (expandedLists.has(key) || list.length <= SESSIONS_VISIBLE_CAP) return list
    return list.filter((s, i) => i < SESSIONS_VISIBLE_CAP || s.id === currentSessionId)
  }, [expandedLists, currentSessionId])

  const expandList = useCallback((key: string) => {
    setExpandedLists(prev => {
      if (prev.has(key)) return prev
      const next = new Set(prev)
      next.add(key)
      return next
    })
  }, [])

  // P-U2: commit a project rename (double-click or ⋯ menu) through the
  // engine registry — the localStorage registry is gone; the returned record
  // updates local state immediately (the backend also persists it).
  const commitProjectRename = useCallback((dirKey: string) => {
    const next = projectNameDraft.trim()
    setEditingProject(null)
    if (!next || !dirKey) return
    callSafe(() => api.renameProject(dirKey, next))
      .then(applyProjectRecord)
      .catch(e => toastError(t('sidebar.projects.rename.failed'), e))
  }, [projectNameDraft, applyProjectRecord, t])

  // P-U2 menu action: new session rooted in this project — create, stamp the
  // working dir (the backend adopts the project into the registry), open it
  // in /chat. new_session emits sessions-updated, so the rail refills (and
  // the registry load refires with it).
  const handleNewSessionInProject = useCallback((path: string) => {
    callSafe(() => api.newSession())
      .then(id => callSafe(() => api.setSessionWorkingDir(id, path)).then(() => id))
      .then(id => switchSession(id))
      .then(() => {
        navigate('/chat')
        closeMobile?.()
      })
      .catch(e => toastError(t('sidebar.projects.newSession.failed'), e))
  }, [switchSession, navigate, closeMobile, t])

  // P-U2 menu action: reveal the project directory. The backend's
  // reveal_in_folder applies a path-scope check ($HOME/**, $TEMP/** +
  // canonicalized must-exist); a project that passes that check but still
  // fails to reveal falls back to opening the directory, and only when both
  // openers reject does the user see an error toast.
  const handleOpenProjectDir = useCallback((path: string) => {
    callSafe(() => api.revealInFolder(path))
      .catch(() => callSafe(() => api.openWithDefaultApp(path)))
      .catch(e => toastError(t('sidebar.projects.open.failed'), e))
  }, [t])

  // P-U2 menu actions: archive from the header ⋯ menu, restore from the
  // 已归档项目 section — same toast/lens language as the session 卡A.
  const handleArchiveProject = useCallback((path: string) => {
    callSafe(() => api.archiveProject(path))
      .then(rec => { applyProjectRecord(rec); toast.success(t('sidebar.projects.archive.toast')) })
      .catch(e => toastError(t('sidebar.projects.archive.failed'), e))
  }, [applyProjectRecord, t])

  const handleRestoreProject = useCallback((path: string) => {
    callSafe(() => api.unarchiveProject(path))
      .then(rec => { applyProjectRecord(rec); toast.success(t('sidebar.projects.restore.toast')) })
      .catch(e => toastError(t('sidebar.projects.restore.failed'), e))
  }, [applyProjectRecord, t])

  // P-U2 color swatch popover: write the picked color through
  // set_project_appearance (icon stays null — no icon UI by ruling); the
  // default swatch clears the color (null).
  const handleSetProjectColor = useCallback((path: string, color: string | null) => {
    setColorPickerFor(null)
    const icon = projects.find(p => normalizePathKey(p.path) === normalizePathKey(path))?.icon ?? null
    callSafe(() => api.setProjectAppearance(path, icon, color))
      .then(applyProjectRecord)
      .catch(e => toastError(t('sidebar.projects.color.failed'), e))
  }, [projects, applyProjectRecord, t])

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

  // 卡A: archive from the row's ⋯ menu. The backend clears the row from
  // the active list (sessions-updated) and the archived section refills.
  const handleArchive = useCallback((session: SessionInfo) => {
    api.archiveSession(session.id)
      .then(() => toast.success(t('sidebar.sessions.archived.toast')))
      .catch(e => toastError(t('sidebar.sessions.archived.failed'), e))
  }, [t])

  // 卡A: restore from the 已归档 section — the rail row is rebuilt by the
  // backend and `sessions` refreshes via sessions-updated.
  const handleRestore = useCallback((id: string) => {
    api.unarchiveSession(id)
      .then(() => toast.success(t('sidebar.sessions.archived.restoredToast')))
      .catch(e => toastError(t('sidebar.sessions.archived.failed'), e))
  }, [t])

  // B4 P2-6: the delete action never rejects — AppContext funnels backend
  // failures into the shared error banner — so success is detected by the
  // target actually leaving its list (active list, or the archived lens for
  // the 永久删除 variant) once the post-delete refresh lands. Until then the
  // dialog stays open; on failure the user can retry or cancel.
  useEffect(() => {
    if (!deleteTarget) return
    const gone = deleteTarget.permanent
      ? !archivedRows.some(r => r.id === deleteTarget.id)
      : !sessions.some(s => s.id === deleteTarget.id)
    if (!gone) return
    // B4 P2-7: the real delete happened — prune its stale pin/order entries.
    pruneSessionMeta(deleteTarget.id)
    setDeleteTarget(null)
  }, [deleteTarget, sessions, archivedRows, pruneSessionMeta])

  const handleDeleteConfirm = useCallback(() => {
    if (!deleteTarget || deletePending) return
    setDeletePending(true)
    // deleteSession funnels backend failures into the shared error banner
    // without rejecting; the catch is belt-and-braces so an unexpected
    // rejection can never surface as an unhandled promise error.
    void deleteSession(deleteTarget.id).catch(() => {}).finally(() => {
      // Gone → the effect above already closed the dialog. Still present →
      // the delete failed: drop the pending flag and leave the dialog open
      // (the error is visible through the shared banner path).
      setDeletePending(false)
    })
  }, [deleteTarget, deletePending, deleteSession])

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
      // 卡A — archive this session (leaves the active rail; restorable from
      // the 已归档 section below).
      { id: 'archive', label: t('chat.session.archive'), icon: 'archive', onSelect: () => handleArchive(session) },
      { id: 'delete', label: t('chat.session.delete'), icon: 'delete', destructive: true, onSelect: () => setDeleteTarget({ id: session.id, title: session.title || untitled }) },
    ]
  }, [pinnedIds, t, untitled, sessions, startRename, togglePin, navigate, handleArchive])

  // P-U2: the project header's ⋯ menu. `path` is the group key (full, trail-
  // normalized working dir) — the registry's unique key. P-U3: the
  // project-context entry points carry ?project= so the target page opens
  // already scoped to this project (the chip there removes it).
  const projectMenuItems = useCallback((group: SessionGroup): DropdownMenuItem[] => {
    const path = group.key
    const projectHref = (pathname: string) => `${pathname}?project=${encodeURIComponent(path)}`
    return [
      { id: 'new-session', label: t('sidebar.projects.newSessionHere'), icon: 'chat_bubble', onSelect: () => handleNewSessionInProject(path) },
      // I2 (review fix): 新建例行 carries a &new=routine marker so it lands
      // distinct from 查看自动化 (byte-identical URLs deduped to one menu
      // entry before) — /tasks opens the create-schedule form when present.
      { id: 'new-routine', label: t('sidebar.projects.newRoutine'), icon: 'event_repeat', onSelect: () => { navigate(`${projectHref('/tasks')}&new=routine`); closeMobile?.() } },
      { id: 'view-automations', label: t('sidebar.projects.viewAutomations'), icon: 'schedule', onSelect: () => { navigate(projectHref('/tasks')); closeMobile?.() } },
      { id: 'view-inbox', label: t('sidebar.projects.viewInbox'), icon: 'inbox', onSelect: () => { navigate(projectHref('/triage')); closeMobile?.() } },
      { id: 'open-folder', label: t('sidebar.projects.openFolder'), icon: 'folder_open', onSelect: () => handleOpenProjectDir(path) },
      { id: 'rename', label: t('sidebar.projects.rename'), icon: 'edit', onSelect: () => { setEditingProject(path); setProjectNameDraft(group.label) } },
      { id: 'color', label: t('sidebar.projects.color'), icon: 'palette', onSelect: () => setColorPickerFor(path) },
      { id: 'archive', label: t('sidebar.projects.archive'), icon: 'archive', onSelect: () => handleArchiveProject(path) },
    ]
  }, [t, navigate, closeMobile, handleNewSessionInProject, handleOpenProjectDir, handleArchiveProject])

  // P-U2 color popover dismissal: Escape or any outside pointer press closes
  // it (the popover is nested in the header wrapper, so a closest() probe is
  // enough to tell inside from outside).
  useEffect(() => {
    if (!colorPickerFor) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setColorPickerFor(null)
    }
    const onPointer = (e: MouseEvent) => {
      const el = e.target as Element | null
      if (!el?.closest?.('[data-testid="project-color-popover"]')) setColorPickerFor(null)
    }
    document.addEventListener('keydown', onKey)
    document.addEventListener('mousedown', onPointer)
    return () => {
      document.removeEventListener('keydown', onKey)
      document.removeEventListener('mousedown', onPointer)
    }
  }, [colorPickerFor])

  const renderGroupHeader = (group: SessionGroup) => {
    if (group.isProject) {
      // ZCode 项目 tree row: a folder button (chevron + name + count) that
      // folds/unfolds its conversations. The active session's project is
      // force-expanded by the effect above. P-U2: double-click (or the ⋯
      // menu) renames the project in place through the engine registry, and
      // a hover ⋯ + touch long-press opens the project actions menu.
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
      const isMenuOpen = projectMenuFor === group.key
      const itemCount = group.sessions.length + (group.routines?.length ?? 0)
      const currentColor = registryByKey.get(group.key)?.color ?? null
      return (
        <div
          role="group"
          aria-label={group.label}
          className="group relative flex items-center min-w-0"
          data-testid={`project-header-${group.key}`}
          onTouchStart={() => startLongPress(group.key, 'project')}
          onTouchEnd={clearLongPress}
          onTouchMove={clearLongPress}
          onTouchCancel={clearLongPress}
        >
          <button
            type="button"
            role="presentation"
            aria-expanded={!isFolded}
            title={group.label}
            onClick={() => {
              // U5: a completed long-press opened the menu — the click that
              // follows must not also fold the project.
              if (suppressClickRef.current) { suppressClickRef.current = false; return }
              toggleFold(group.key)
            }}
            onDoubleClick={() => {
              if (!group.key) return
              setEditingProject(group.key)
              setProjectNameDraft(group.label)
            }}
            className="flex-1 min-w-0 flex items-center gap-1.5 px-3 pt-2 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 hover:text-primary transition-colors cursor-pointer"
          >
            <span
              className="material-symbols-outlined text-[14px] shrink-0 transition-transform duration-150"
              style={{ transform: isFolded ? 'rotate(-90deg)' : 'rotate(0deg)' }}
              aria-hidden="true"
            >
              expand_more
            </span>
            <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">{group.icon}</span>
            {group.color && (
              <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: group.color }} aria-hidden="true" />
            )}
            <span className="truncate flex-1 min-w-0 text-left">{group.label}</span>
            {itemCount > 0 && (
              <span className="font-mono text-[10px] tabular-nums text-on-surface-variant/70 shrink-0">{itemCount}</span>
            )}
          </button>
          <div className="relative shrink-0">
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={t('sidebar.projects.menu.aria', { name: group.label })}
              className={cn(
                'rounded hover:bg-surface-container text-on-surface-variant hover:text-primary transition-opacity focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none',
                isMenuOpen || colorPickerFor === group.key ? 'opacity-100' : 'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
              )}
              onClick={e => {
                e.stopPropagation()
                setColorPickerFor(null)
                setProjectMenuFor(isMenuOpen ? null : group.key)
              }}
            >
              <span className="material-symbols-outlined text-[16px]">more_horiz</span>
            </Button>
            {isMenuOpen && (
              <DropdownMenu
                open
                onClose={() => setProjectMenuFor(null)}
                items={projectMenuItems(group)}
                align="end"
                className="w-44 min-w-0"
                ariaLabel={t('sidebar.projects.menu.aria', { name: group.label })}
              />
            )}
            {colorPickerFor === group.key && (
              <div
                role="menu"
                aria-label={t('sidebar.projects.color')}
                data-testid="project-color-popover"
                className="absolute right-0 top-full mt-sm z-modal flex items-center gap-1.5 px-sm py-sm rounded-xl border border-outline-variant/20 bg-surface-container-lowest/95 backdrop-blur-lg shadow-[var(--shadow-e3)]"
              >
                {PROJECT_COLORS.map((color, i) => (
                  <button
                    key={color}
                    type="button"
                    role="menuitemradio"
                    aria-checked={currentColor === color}
                    aria-label={t('sidebar.projects.colorSwatch.aria', { n: i + 1 })}
                    title={t('sidebar.projects.colorSwatch.aria', { n: i + 1 })}
                    className={cn(
                      'w-4 h-4 rounded-full cursor-pointer transition-transform hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40',
                      currentColor === color && 'ring-2 ring-on-surface/60 ring-offset-1 ring-offset-surface-container-lowest',
                    )}
                    style={{ backgroundColor: color }}
                    onClick={() => handleSetProjectColor(group.key, color)}
                  />
                ))}
                <span className="w-px h-4 bg-outline-variant/40 shrink-0" aria-hidden="true" />
                <button
                  type="button"
                  role="menuitemradio"
                  aria-checked={currentColor === null}
                  aria-label={t('sidebar.projects.colorDefault')}
                  title={t('sidebar.projects.colorDefault')}
                  className="w-4 h-4 rounded-full border border-dashed border-on-surface-variant/60 cursor-pointer transition-transform hover:scale-110 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/40"
                  onClick={() => handleSetProjectColor(group.key, null)}
                />
              </div>
            )}
          </div>
        </div>
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
        onTouchStart={() => startLongPress(session.id, 'session')}
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

  // P-U1: a nested routine row inside its project group — clock icon + name
  // + the 即将 badge when the next fire is imminent (the automations-row
  // language), otherwise a short date for the scheduled fire. Clicking goes
  // to /tasks where the routine lives.
  const renderRoutineRow = (routine: ScheduledRoutine) => {
    const soon = routine.next_fire_at != null && routine.next_fire_at - nowTick < 3600_000
    let when = ''
    if (!soon && routine.next_fire_at != null && routine.next_fire_at > 0) {
      try {
        when = new Intl.DateTimeFormat(undefined, { month: 'numeric', day: 'numeric' }).format(routine.next_fire_at)
      } catch { when = '' }
    }
    return (
      <button
        key={routine.id}
        type="button"
        role="listitem"
        data-testid={`sidebar-routine-row-${routine.id}`}
        title={routine.name}
        onClick={() => { navigate('/tasks'); closeMobile?.() }}
        className="w-full flex items-center gap-2 px-3 py-1.5 rounded-lg font-label-md text-label-md text-on-surface-variant hover:bg-surface-container-low hover:text-primary transition-colors cursor-pointer min-w-0 select-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 whitespace-nowrap"
      >
        <span
          className={cn('material-symbols-outlined text-[13px] shrink-0', soon ? 'text-warning' : 'text-on-surface-variant')}
          aria-hidden="true"
        >
          schedule
        </span>
        <span className="truncate flex-1 min-w-0 text-left">{routine.name}</span>
        {soon ? (
          <span className="font-label-xs px-1 py-[1px] rounded bg-warning/15 text-warning shrink-0" role="img" aria-label={t('sidebar.automations.soon')}>
            {t('sidebar.automations.soon')}
          </span>
        ) : when ? (
          <span className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0" aria-hidden="true">{when}</span>
        ) : null}
      </button>
    )
  }

  // P-U2: the greyed, NON-interactive placeholder under a registry-only
  // project (nothing to open yet — deliberately not a button, not focusable;
  // role=listitem keeps the parent role="list" axe-clean).
  const renderProjectEmptyRow = () => (
    <div
      role="listitem"
      data-testid="project-empty-row"
      className="px-3 py-1.5 font-label-sm text-label-sm text-on-surface-variant select-none"
    >
      {t('sidebar.projects.empty')}
    </div>
  )

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
      {/* P-U1/I3: the rail's standalone 自动化 section. In project lens it
          lists ONLY unhoused routines (the housed ones nest into their
          project groups above); in the time/smart lenses the tree is not
          rendered, so this section carries ALL enabled routines instead —
          otherwise housed automations would go dark outside the project
          lens. Hidden when nothing to show. */}
      {!query.trim() && lensRoutines.length > 0 && (
        <div className="mb-xs" data-testid="sidebar-automations">
          <div className="flex items-center gap-1.5 px-3 pt-1 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 min-w-0">
            <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">event_repeat</span>
            <span className="truncate flex-1 min-w-0">{t('sidebar.automations.title')}</span>
            <span className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0">{lensRoutines.length}</span>
          </div>
          {lensRoutines.slice(0, 3).map(r => {
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
          {lensRoutines.length > 3 && (
            <button
              type="button"
              onClick={() => navigate('/tasks')}
              className="w-full px-3 py-1 text-left font-label-xs text-on-surface-variant hover:text-primary hover:underline cursor-pointer"
            >
              {t('sidebar.automations.more', { n: lensRoutines.length - 3 })}
            </button>
          )}
        </div>
      )}
      <ScrollArea className="flex-1 min-h-0">
        {sessions.length === 0 && !query.trim() ? (
          // 卡A 收尾: light empty hint for the active area while the rail
          // stays up for the 已归档 section (everything archived).
          <div
            className="px-2 py-3 text-center font-label-sm text-label-sm text-on-surface-variant"
            data-testid="sidebar-active-empty-hint"
          >
            {t('sidebar.active.empty')}
          </div>
        ) : filtered.length === 0 ? (
          <div className="px-2 py-3 text-center font-label-sm text-label-sm text-on-surface-variant">
            {t('sidebar.sessions.noResults')}
          </div>
        ) : groups === null ? (
          <>
            <div className="space-y-0.5 pr-1" role="list" aria-label={t('sidebar.sessions.list.aria')}>
              {visibleSlice(filtered, 'flat').map(renderRow)}
            </div>
            {visibleSlice(filtered, 'flat').length < filtered.length && (
              <ExpanderRow
                label={t('sidebar.sessions.showAll', { n: filtered.length })}
                onClick={() => expandList('flat')}
              />
            )}
          </>
        ) : (
          <div className="space-y-0.5 pr-1" role="list" aria-label={t('sidebar.sessions.list.aria')}>
            {groups.map(group => {
              const visibleSessions = visibleSlice(group.sessions, group.key)
              return (
              <Fragment key={group.key}>
                {group.isProject ? (
                  // The project group is ONE list item of the rail's list:
                  // its header (fold toggle + ⋯ menu) and its nested row
                  // list all live inside it — role=listitem is the only
                  // child role role="list" accepts, and the rows keep their
                  // own nested role="list" so session-row listitem semantics
                  // stay axe-clean. Project conversations nest under their
                  // folder (ZCode 项目 tree); folded projects collapse their
                  // rows. B4 P2-4: the mixed session/routine row list is
                  // capped like every other list.
                  <div role="listitem" className="min-w-0">
                    {renderGroupHeader(group)}
                    {!foldedProjects.has(group.key) && (
                      <div
                        role="list"
                        aria-label={group.label}
                        className="pl-4"
                      >
                        {group.isEmpty ? (
                          renderProjectEmptyRow()
                        ) : (() => {
                          const rows = (group.rows ?? []).map(r =>
                            r.kind === 'session'
                              ? { kind: 'session' as const, session: r.session, id: r.session.id }
                              : { kind: 'routine' as const, routine: r.routine, id: r.routine.id },
                          )
                          const visibleRows = visibleSlice(rows, group.key)
                          return (
                            <>
                              {visibleRows.map(r =>
                                r.kind === 'session' ? renderRow(r.session) : renderRoutineRow(r.routine),
                              )}
                              {visibleRows.length < rows.length && (
                                <ExpanderRow
                                  label={t('sidebar.sessions.showAll', { n: rows.length })}
                                  onClick={() => expandList(group.key)}
                                />
                              )}
                            </>
                          )
                        })()}
                      </div>
                    )}
                  </div>
                ) : (
                  <>
                    {renderGroupHeader(group)}
                    <div>
                      {visibleSessions.map(renderRow)}
                      {visibleSessions.length < group.sessions.length && (
                        <ExpanderRow
                          label={t('sidebar.sessions.showAll', { n: group.sessions.length })}
                          onClick={() => expandList(group.key)}
                        />
                      )}
                    </div>
                  </>
                )}
              </Fragment>
              )
            })}
          </div>
        )}
        {/* 卡A — the collapsed 已归档 section at the bottom of the rail, in
            the same lens/visual language as the 项目 folders. Archived rows
            show title + last activity with a 恢复 action; opening a row
            resumes it (the backend auto-unarchives). Hidden while searching
            (active results stand alone) and when nothing is archived. */}
        {!query.trim() && archivedRows.length > 0 && (
          <div className="mt-2 border-t border-outline-variant/20 pt-1" data-testid="sidebar-archived-section">
            <button
              type="button"
              aria-expanded={archivedOpen}
              data-testid="sidebar-archived-toggle"
              onClick={() => setArchivedOpen(!archivedOpen)}
              className="w-full flex items-center gap-1.5 px-3 pt-2 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 hover:text-primary transition-colors min-w-0 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded"
            >
              <span
                className="material-symbols-outlined text-[14px] shrink-0 transition-transform duration-150"
                style={{ transform: archivedOpen ? 'rotate(0deg)' : 'rotate(-90deg)' }}
                aria-hidden="true"
              >
                expand_more
              </span>
              <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">archive</span>
              <span className="truncate flex-1 min-w-0 text-left">{t('sidebar.sessions.archived.title')}</span>
              <span className="font-mono text-[10px] tabular-nums text-on-surface-variant/70 shrink-0">{archivedRows.length}</span>
            </button>
            {archivedOpen && (
              <div className="pl-4 space-y-0.5" role="list" aria-label={t('sidebar.sessions.archived.aria')}>
                {visibleSlice(archivedRows, 'archived').map(row => {
                  const title = row.title || untitled
                  const ago = formatRelativeTime(row.updated_at ?? undefined, nowTick, t)
                  return (
                    <div key={row.id} role="listitem" className="group flex items-center gap-1" data-testid={`archived-row-${row.id}`}>
                      <button
                        type="button"
                        title={title}
                        aria-label={t('sidebar.sessions.archived.row.aria', { title })}
                        onClick={() => handleSwitch(row.id)}
                        className="flex-1 min-w-0 text-left px-3 py-1.5 rounded-lg font-label-md text-label-md text-on-surface-variant/80 hover:bg-surface-container-low hover:text-primary transition-colors cursor-pointer select-none flex items-center gap-2 whitespace-nowrap focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
                      >
                        <span className="flex-1 truncate">{title}</span>
                        {ago && (
                          <span className="font-mono text-[10px] tabular-nums text-on-surface-variant shrink-0" aria-hidden="true">{ago}</span>
                        )}
                      </button>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        data-testid={`archived-restore-${row.id}`}
                        aria-label={t('sidebar.sessions.archived.restore.aria', { title })}
                        title={t('sidebar.sessions.archived.restore')}
                        className={cn(
                          'rounded hover:bg-surface-container text-on-surface-variant hover:text-primary transition-opacity focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none shrink-0',
                          'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
                        )}
                        onClick={() => handleRestore(row.id)}
                      >
                        <span className="material-symbols-outlined text-[16px]">undo</span>
                      </Button>
                      {/* B4 P2-6: archived sessions finally have a way out —
                          delete_session removes the whole L0 directory
                          (log + sidecar, including the archived flag), so
                          no Rust change is needed. Destructive confirm via
                          the same dialog's 永久删除 variant. */}
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        data-testid={`archived-delete-${row.id}`}
                        aria-label={t('sidebar.sessions.archived.delete.aria', { title })}
                        title={t('sidebar.sessions.archived.delete.aria', { title })}
                        className={cn(
                          'rounded hover:bg-error/10 text-on-surface-variant hover:text-error transition-opacity focus-visible:ring-2 focus-visible:ring-error/30 focus-visible:outline-none shrink-0',
                          'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
                        )}
                        onClick={() => setDeleteTarget({ id: row.id, title, permanent: true })}
                      >
                        <span className="material-symbols-outlined text-[16px]">delete_forever</span>
                      </Button>
                    </div>
                  )
                })}
                {archivedOpen && visibleSlice(archivedRows, 'archived').length < archivedRows.length && (
                  <ExpanderRow
                    label={t('sidebar.sessions.showAll', { n: archivedRows.length })}
                    onClick={() => expandList('archived')}
                  />
                )}
              </div>
            )}
          </div>
        )}
        {/* P-U2 — the 已归档项目 section at the very bottom of the rail, in
            the same collapsed lens as the sessions' one. Rows carry a 恢复
            action (unarchive); the project then reappears in the tree (or
            as the live registry entry it already was). Hidden while
            searching and when nothing is archived. */}
        {!query.trim() && archivedRegistryProjects.length > 0 && (
          <div className="mt-2 border-t border-outline-variant/20 pt-1" data-testid="sidebar-archived-projects">
            <button
              type="button"
              aria-expanded={archivedProjectsOpen}
              data-testid="sidebar-archived-projects-toggle"
              onClick={() => setArchivedProjectsOpen(!archivedProjectsOpen)}
              className="w-full flex items-center gap-1.5 px-3 pt-2 pb-1 font-label-sm text-[11px] font-bold text-on-surface-variant/90 hover:text-primary transition-colors min-w-0 cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded"
            >
              <span
                className="material-symbols-outlined text-[14px] shrink-0 transition-transform duration-150"
                style={{ transform: archivedProjectsOpen ? 'rotate(0deg)' : 'rotate(-90deg)' }}
                aria-hidden="true"
              >
                expand_more
              </span>
              <span className="material-symbols-outlined text-[13px] shrink-0" aria-hidden="true">folder_off</span>
              <span className="truncate flex-1 min-w-0 text-left">{t('sidebar.projects.archived.title')}</span>
              <span className="font-mono text-[10px] tabular-nums text-on-surface-variant/70 shrink-0">{archivedRegistryProjects.length}</span>
            </button>
            {archivedProjectsOpen && (
              <div className="pl-4 space-y-0.5" role="list" aria-label={t('sidebar.projects.archived.aria')}>
                {archivedRegistryProjects.map(row => {
                  const label = row.name ?? pathTail(normalizePathKey(row.path) ?? row.path)
                  return (
                    <div key={row.path} role="listitem" className="group flex items-center gap-1" data-testid={`archived-project-row-${row.path}`}>
                      <div className="flex-1 min-w-0 px-3 py-1.5 font-label-md text-label-md text-on-surface-variant/80 flex items-center gap-2 whitespace-nowrap">
                        {row.color && (
                          <span className="w-2 h-2 rounded-full shrink-0" style={{ backgroundColor: row.color }} aria-hidden="true" />
                        )}
                        <span className="truncate">{label}</span>
                      </div>
                      <Button
                        variant="ghost"
                        size="icon-xs"
                        data-testid={`archived-project-restore-${row.path}`}
                        aria-label={t('sidebar.projects.archived.restore.aria', { name: label })}
                        title={t('sidebar.sessions.archived.restore')}
                        className={cn(
                          'rounded hover:bg-surface-container text-on-surface-variant hover:text-primary transition-opacity focus-visible:ring-2 focus-visible:ring-primary/30 focus-visible:outline-none shrink-0',
                          'opacity-0 group-hover:opacity-100 focus-visible:opacity-100',
                        )}
                        onClick={() => handleRestoreProject(row.path)}
                      >
                        <span className="material-symbols-outlined text-[16px]">undo</span>
                      </Button>
                    </div>
                  )
                })}
              </div>
            )}
          </div>
        )}
      </ScrollArea>

      <DeleteSessionModal
        t={t}
        target={deleteTarget}
        pending={deletePending}
        onCancel={() => { if (!deletePending) setDeleteTarget(null) }}
        onConfirm={handleDeleteConfirm}
      />
    </div>
  )
}

/** B4 P2-4: the 显示全部 expander under a capped list. */
function ExpanderRow({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      data-testid="sidebar-show-all"
      onClick={onClick}
      className="w-full px-3 py-1 text-left font-label-xs text-on-surface-variant hover:text-primary hover:underline cursor-pointer"
    >
      {label}
    </button>
  )
}

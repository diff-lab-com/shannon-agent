// AppContext — composition root for the three slice contexts (Chat, Session,
// Catalog). The provider owns ALL state and actions in one place (so cross-
// slice actions like sendMessage calling setError stay simple) but exposes
// them through three memoized context values, so a consumer using useChat() /
// useSessions() / useCatalog() only re-renders when its own slice changes.
// The legacy useApp() facade composes all three for backwards compatibility.
//
// Split history: this was a single god-context whose value changed on every
// streamed token, re-rendering all 19 consumers. The slice split scopes the
// high-frequency chat streaming to chat consumers only.

import { useState, useEffect, useCallback, useMemo, useRef, type ReactNode } from 'react'
import { messageFor } from '@/i18n'
import { describeBackendError } from '@/lib/backendError'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import { isEventForCurrentWindow, parseWindowSession } from '@/lib/windowSession'
import { reportRejectedAttachments } from '@/lib/attachmentFeedback'
import { basenameOf } from '@/lib/fileRefs'
import {
  beginRun as runBegin,
  endRun as runEnd,
  initialRunProcess,
  noteToolProgress as runToolProgress,
  noteToolStart as runToolStart,
  type RunProcessState,
} from '@/lib/runProcess'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'
import { toastError } from '@/lib/errorToast'
import type { CheckpointInfo, FeedbackRating } from '@/lib/tauri-api'
import {
  EVENT_NAMES,
  type ChatMessage,
  type GoalRunDto,
  type ToolCall,
  type SessionInfo,
  type SessionActivity,
  type StatusResponse,
  type DesktopConfig,
  type ProviderStatus,
  type ModelInfo,
  type PermissionRequest,
  type BackgroundTaskInfo,
  type TaskItem,
  type AgentInfo,
  type UsagePayload,
  type McpServerInfo,
  type SubAgentLive,
  type QueryNoticeEvent,
} from '@/types'
import { ChatProvider, useChat, type ChatContextValue, type PromptQueueItem, type StreamNotice } from './ChatContext'
import { SessionContext, useSessions, type SessionContextValue } from './SessionContext'
import { CatalogContext, useCatalog, type CatalogContextValue } from './CatalogContext'

export type AppContextValue = ChatContextValue & SessionContextValue & CatalogContextValue

/**
 * Backwards-compatible facade over the three slice contexts. New code should
 * call the specific hooks (useChat / useSessions / useCatalog) directly so it
 * only re-renders when its slice changes; this keeps legacy `useApp()` call
 * sites working unchanged.
 */
export function useApp(): AppContextValue {
  return { ...useCatalog(), ...useSessions(), ...useChat() }
}


/**
 * Background refreshes fail soft on purpose: several of them fire after a
 * single user action, so a toast per failure would spam during backend
 * hiccups. The app keeps its last-known state; startup failures surface
 * through `initError` instead. One helper keeps the policy grep-able.
 */
function logSoftFailure(what: string, e: unknown) {
  console.warn(`[shannon] ${what} failed:`, e)
}

// B1 P1-13: dock open state persists under the dock's own key namespace.
const DOCK_OPEN_KEY = 'shannon.dock.open'
// B1 P2-13: minimum interval between visible streaming-bucket projections.
const STREAM_FLUSH_MS = 50
// B1 §4-9: per-session prompt queue capacity (spec: 1–3, we take 3).
const PROMPT_QUEUE_CAP = 3

export function AppProvider({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [thinkingText, setThinkingText] = useState('')
  // B1 P1-5: per-session query state. The old single boolean let a background
  // session's run disable the foreground session's composer and pointed the
  // stop button at the wrong run. The record holds `true` per session id
  // currently running; `isQuerying` (below) projects the VISIBLE session's
  // entry, so gating/stop/error UI all key off the session on screen.
  const [queryingSessions, setQueryingSessions] = useState<Record<string, true>>({})
  const [activeToolCalls, setActiveToolCalls] = useState<ToolCall[]>([])
  // GB P2-3: 「过程四要素」 aggregation for the VISIBLE session's run —
  // fed by the same query events that fill activeToolCalls, reduced by
  // lib/runProcess. Cleared on new sends (beginRun), settled on
  // completed/failed/cancelled (endRun), reset on session switch — the run
  // tab's content survives between those points exactly like the brief's
  // "结束保留至下一轮开始".
  const [runProcess, setRunProcess] = useState<RunProcessState>(initialRunProcess)
  // Round-1 review (Minor-4): synchronous mirror of runProcess — a send the
  // backend REJECTS before recording the user message (budget/concurrent/
  // goal guards) must restore the pre-send snapshot, not leave a phantom
  // "Running" tab. (A setState-updater stash would still be unflushed when
  // the rejection's catch runs, so the mirror is kept during render, the
  // same pattern as visibleSessionIdRef below.)
  const runProcessRef = useRef<RunProcessState>(runProcess)
  runProcessRef.current = runProcess
  // P2-19: live progress of the VISIBLE session's currently-running tool
  // (QUERY_TOOL_PROGRESS {progress, progress_message}). The raw fields also
  // land on the matching activeToolCalls card; this dedicated slot feeds the
  // RunStatusLine pill. Single visible-session value, like the streaming
  // projections: progress for a background session is dropped (its pill is
  // not on screen), and switching sessions clears it rather than projecting
  // per-session buckets. Cleared everywhere activeToolCalls is cleared, on
  // every new tool start (stale % from the previous tool must not label the
  // next one), and on new sends.
  const [toolProgress, setToolProgress] = useState<{ progress?: number; message?: string } | null>(null)
  // R5-2: retry notices (failover / key rotation) observed per session's
  // current turn — the engine continued after the notice, so this is
  // informational, never an error banner. Bucketed per session like the
  // stream text (§P2-18): the visible projection is `streamNotices`.
  // Lifecycle: appended by QUERY_NOTICE, survives the run's completion (the
  // user can still see how the answer was served), cleared on the session's
  // next send, re-projected on session switch, dropped with the session.
  // Capped per session — a pathological provider cannot grow it unbounded.
  const streamNoticesBucketsRef = useRef<Map<string, StreamNotice[]>>(new Map())
  const streamNoticeIdRef = useRef(0)
  const STREAM_NOTICES_CAP = 20
  const [streamNotices, setStreamNotices] = useState<StreamNotice[]>([])
  const [usage, setUsage] = useState<UsagePayload | null>(null)
  // B2: live registry state of the currently running sub-agent, from the
  // subagent:start / subagent:stop bridge. Single slot — one live spawn per
  // session is the engine's practical pattern (agent_spawn blocks until the
  // run completes). Cleared on stop and on query end (crash safety).
  const [subagentLive, setSubagentLive] = useState<SubAgentLive | null>(null)
  // U2: ContextPanel visibility — owned here (not in the /chat page) so the
  // global Header can host the toggle while Chat renders the panel.
  // B1 P1-13: persisted (`shannon.dock.open`) like the dock's tab/width/
  // fullscreen keys — the toggle, Ctrl+\ and RightDock's close button all
  // funnel through the same persisted setter below.
  const [contextPanelOpen, setContextPanelOpen] = useState<boolean>(() => {
    try { return localStorage.getItem(DOCK_OPEN_KEY) === '1' } catch { return false }
  })
  // B1 §4-9: per-session FIFO of prompts typed while that session was still
  // streaming. State is the rendered projection (queue chips); the ref mirror
  // lets event handlers and the drain path read/mutate synchronously without
  // stale-closure races.
  const [promptQueues, setPromptQueues] = useState<Record<string, PromptQueueItem[]>>({})
  const promptQueuesRef = useRef<Map<string, PromptQueueItem[]>>(new Map())
  const queueItemIdRef = useRef(0)
  // B1 P2-3: transient flag while a session-switch IPC is in flight.
  const [switchingSession, setSwitchingSession] = useState(false)
  // Review P1-3 (B1-9): monotonic token for session switches. Rapid clicks
  // race the awaited `switch_session` IPC — without the token, an older
  // response can land after a newer one and clobber
  // `currentSessionId`/`messages` with the session the user LEFT.
  const switchTokenRef = useRef(0)
  // Which session's history `messages` currently holds. A switch whose
  // target is ALREADY the visible, loaded session must not re-fetch and
  // re-replace: the awaited IPC would land on top of optimistic sends made
  // in the meantime and wipe their bubbles (observed as the A-4 attachment
  // card vanishing between the send and its own echo). Boot binding (A-6)
  // makes this the common path — the first rail click after a cold start
  // targets the session the boot already loaded.
  const loadedSessionRef = useRef<string | null>(null)
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  // P0 sidebar telemetry: live per-session activity (running / elapsed /
  // active tool) for the session rail. Derived from the same query:* events
  // the chat slice consumes — the main window receives every session's
  // events (isEventForCurrentWindow passes all through), so the rail can
  // show cross-session runs; a dedicated window only ever sees its own
  // session's events, which is all its rail shows. High-frequency events
  // (text/thinking/usage) only touch the ref; state updates are limited to
  // membership/tool transitions so streaming never re-renders the sidebar.
  const [sessionActivity, setSessionActivity] = useState<Record<string, SessionActivity>>({})
  const sessionActivityRef = useRef<Map<string, SessionActivity>>(new Map())
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null)
  // office Wave 3 C4: per-session scratchpad of source refs (paths/URLs) for
  // the Context tab's "Session sources" block. In-memory draft board —
  // deliberately NOT persisted and NOT wired into the send pipeline.
  const [sessionSources, setSessionSources] = useState<Record<string, string[]>>({})
  const addSessionSource = useCallback((sessionId: string, item: string) => {
    const trimmed = item.trim()
    if (!sessionId || !trimmed) return
    setSessionSources(prev => {
      const list = prev[sessionId] ?? []
      if (list.includes(trimmed)) return prev
      return { ...prev, [sessionId]: [...list, trimmed] }
    })
  }, [])
  const removeSessionSource = useCallback((sessionId: string, item: string) => {
    setSessionSources(prev => {
      const list = (prev[sessionId] ?? []).filter(s => s !== item)
      const next = { ...prev }
      if (list.length === 0) delete next[sessionId]
      else next[sessionId] = list
      return next
    })
  }, [])
  // P1-1 window mode: this webview was opened as a dedicated session window
  // (`/?windowSession=<id>`). In-memory only — parsed from the URL once,
  // never persisted, so the main window is unaffected.
  const [windowSessionId] = useState<string | null>(() => parseWindowSession())
  const [status, setStatus] = useState<StatusResponse | null>(null)
  const [config, setConfig] = useState<DesktopConfig | null>(null)
  // 2026-09-29 provider review §3-A1: reliable activation signal —
  // `config.provider`/`config.api_key` are dead since ADR-0005, so the
  // banner / welcome / settings gates read this snapshot instead.
  const [providerStatus, setProviderStatus] = useState<ProviderStatus | null>(null)
  const [models, setModels] = useState<ModelInfo[]>([])
  const [permissionRequest, setPermissionRequest] = useState<PermissionRequest | null>(null)
  // /rewind: checkpoints for the current session (turn indices + previews).
  const [checkpoints, setCheckpoints] = useState<CheckpointInfo[]>([])
  // PM-12: persisted 👍/👎 for the current session's messages.
  const [feedback, setFeedback] = useState<Record<string, FeedbackRating>>({})
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTaskInfo[]>([])
  const [tasks, setTasks] = useState<TaskItem[]>([])
  const [agents, setAgents] = useState<AgentInfo[]>([])
  const [mcpServers, setMcpServers] = useState<McpServerInfo[]>([])
  const [error, setError] = useState<string | null>(null)
  // Review §2-3: machine-readable failure class for the chat error banner —
  // `auth` (401/403, classified Rust-side on the QUERY_FAILED payload) gets
  // the dedicated "update key" banner; `other` keeps the raw error line.
  // Kept in lockstep with `error` via setChatError below.
  const [errorKind, setErrorKind] = useState<'auth' | 'other' | null>(null)
  // P0-2: sessions currently owned by a desktop goal run (running/paused).
  // Manual sends to these are blocked — goal and manual input are mutually
  // exclusive; the backend `send_message` guard is the backstop.
  const [goalOwnedSessionIds, setGoalOwnedSessionIds] = useState<string[]>([])
  // P2-⑥: full goal-run info keyed by session — the sidebar renders a goal
  // badge + iteration progress for sessions a run owns.
  const [goalRunsBySession, setGoalRunsBySession] = useState<Record<string, GoalRunDto>>({})
  const [loading, setLoading] = useState(true)
  // First paint of the app depends on these loads succeeding; a silent
  // failure here used to leave the user on an empty UI with only a generic
  // chat-canvas error and no retry. initError is surfaced by the Layout
  // banner; retryInit re-runs the whole load.
  const [initError, setInitError] = useState<string | null>(null)

  // Review §P2-18: the main window receives every session's query:* events
  // (isEventForCurrentWindow passes all through), so streaming text must be
  // bucketed per session id — a single buffer would interleave tokens from
  // concurrent runs and commit the mixture as one assistant message.
  // `streamingText` / `thinkingText` are projections of the *visible*
  // session's bucket (windowSessionId ?? currentSessionId).
  const streamingBucketsRef = useRef<Map<string, string>>(new Map())
  const thinkingBucketsRef = useRef<Map<string, string>>(new Map())
  // W3-4: the per-turn citation snapshot each send receives on its
  // SendMessageResponse, keyed by the owning session — QUERY_COMPLETED pops
  // it onto the committed assistant message (citation chips). Live-turn
  // only: cleared on fail/cancel/new send, never persisted.
  const pendingInjectedMemoriesRef = useRef<Map<string, api.InjectedMemory[]>>(new Map())
  const visibleSessionIdRef = useRef<string | null>(windowSessionId)
  visibleSessionIdRef.current = windowSessionId ?? currentSessionId
  // B1 P1-5: the visible session's own run gates this session's composer,
  // stop button and error surface — never another session's.
  const isQuerying = !!queryingSessions[visibleSessionIdRef.current ?? '']

  // B1 P2-13: token-by-token setStreamingText re-parsed the whole markdown
  // document per token (O(n²) over a long reply). Buckets still absorb every
  // token synchronously; the visible projection is flushed at most once per
  // STREAM_FLUSH_MS. setTimeout (not rAF) is the single mechanism on purpose:
  // rAF stalls in occluded windows and needs a timeout backstop anyway, and
  // it stays deterministic under fake timers. A guaranteed synchronous flush
  // runs on query end (COMPLETED/FAILED/CANCELLED), session switch and new
  // sends so no tail is ever left in a pending timer.
  const streamFlushTimerRef = useRef<number | null>(null)
  const projectVisibleBuckets = useCallback(() => {
    const key = visibleSessionIdRef.current ?? ''
    setStreamingText(streamingBucketsRef.current.get(key) ?? '')
    setThinkingText(thinkingBucketsRef.current.get(key) ?? '')
  }, [])
  const scheduleStreamFlush = useCallback(() => {
    if (streamFlushTimerRef.current != null) return
    streamFlushTimerRef.current = window.setTimeout(() => {
      streamFlushTimerRef.current = null
      projectVisibleBuckets()
    }, STREAM_FLUSH_MS)
  }, [projectVisibleBuckets])
  const cancelStreamFlush = useCallback(() => {
    if (streamFlushTimerRef.current != null) {
      window.clearTimeout(streamFlushTimerRef.current)
      streamFlushTimerRef.current = null
    }
  }, [])

  // B1 P1-5: flip one session's query entry. No-op writes keep object
  // identity stable so memoized context values don't churn.
  const setSessionQuerying = useCallback((sessionId: string | null | undefined, on: boolean) => {
    const key = sessionId ?? ''
    setQueryingSessions(prev => {
      if (!!prev[key] === on) return prev
      const next = { ...prev }
      if (on) next[key] = true
      else delete next[key]
      return next
    })
  }, [])

  // P0 sidebar telemetry: record one query-stream observation for a session.
  // A state update publishes only when the sidebar-VISIBLE projection flips
  // (membership / running / activeTool / failed / awaitingApproval) or the
  // kind is state-bearing anyway — high-frequency `event` ticks (text/
  // thinking/usage) still touch just the ref's lastActivity, so a streaming
  // run re-renders the rail on transitions, never per chunk. S-2 fix: the
  // FIRST observation of a run is such a flip even when it is an `event`
  // (a pure-text run's only events are text/thinking/usage ticks — ref-only
  // publication left the rail without its Running dot for the whole run).
  const noteSessionActivity = useCallback((
    sessionId: string | null | undefined,
    kind: 'event' | 'tool-start' | 'tool-end' | 'end' | 'fail',
    toolName?: string,
  ) => {
    if (!sessionId) return
    const map = sessionActivityRef.current
    const now = Date.now()
    const prev = map.get(sessionId)
    const base: SessionActivity = prev ?? { running: true, startedAt: now, lastActivity: now, activeTool: null }
    let next = base
    switch (kind) {
      case 'event':
        // A live run supersedes both stale signals (B2): a failure is being
        // retried and an approval prompt belongs to the previous turn.
        next = { ...base, running: true, lastActivity: now, failed: false, awaitingApproval: false }
        break
      case 'tool-start':
        next = { ...base, running: true, lastActivity: now, activeTool: toolName ?? null, failed: false, awaitingApproval: false }
        break
      case 'tool-end':
        next = { ...base, lastActivity: now, activeTool: null }
        break
      case 'end':
        next = { ...base, running: false, lastActivity: now, activeTool: null }
        break
      case 'fail':
        // Batch B2: surface the failure on the rail until a new run starts
        // or the user opens the session (switchToSession clears the flag).
        next = { ...base, running: false, lastActivity: now, activeTool: null, failed: true }
        break
    }
    // S-2 fix: when this observation (re)starts a run on an entry left
    // behind by a settled run, restart the elapsed clock — `base` carried
    // the PREVIOUS run's startedAt, which would render a huge stale elapsed.
    if (next.running && prev && !prev.running) next = { ...next, startedAt: now }
    map.set(sessionId, next)
    // S-2 fix: publish on a visible flip too — membership (first observation
    // of a run) or any running/tool/failed/approval transition. Mid-run
    // `event` ticks still refresh the ref only, so streaming never
    // re-renders the sidebar per chunk.
    const visibleFlip = !prev
      || prev.running !== next.running
      || prev.activeTool !== next.activeTool
      || prev.failed !== next.failed
      || prev.awaitingApproval !== next.awaitingApproval
    if (kind !== 'event' || visibleFlip) setSessionActivity(Object.fromEntries(map))
  }, [])

  // Batch B2: permission prompts surface as an amber dot on the owning
  // session's rail row (cleared on resolve, or when the session runs again).
  const noteSessionApproval = useCallback((sessionId: string | null | undefined, pending: boolean) => {
    if (!sessionId) return
    const map = sessionActivityRef.current
    const prev = map.get(sessionId)
    if (!prev && !pending) return
    const next: SessionActivity = prev
      ? { ...prev, awaitingApproval: pending }
      : { running: false, startedAt: null, lastActivity: Date.now(), activeTool: null, awaitingApproval: true }
    map.set(sessionId, next)
    setSessionActivity(Object.fromEntries(map))
  }, [])

  const refreshSessions = useCallback(async () => {
    try {
      const list = await api.listSessions()
      setSessions(list)
      // P0 sidebar telemetry: reconcile the live map with backend truth —
      // seeds runs that started before this window joined the event stream
      // (goal-owned runs, cold start) and drops deleted sessions.
      const map = sessionActivityRef.current
      const alive = new Set(list.map(s => s.id))
      let changed = false
      for (const s of list) {
        if (s.running && !map.has(s.id)) {
          map.set(s.id, { running: true, startedAt: null, lastActivity: Date.now(), activeTool: null })
          changed = true
        }
      }
      for (const id of [...map.keys()]) {
        if (!alive.has(id)) { map.delete(id); changed = true }
      }
      if (changed) setSessionActivity(Object.fromEntries(map))
    } catch (e) { logSoftFailure('refresh sessions', e) }
  }, [])

  // B1 P1-13: every open/close path (Header button, Ctrl+\, RightDock's
  // close button, auto-dock) funnels through this persisted setter, so
  // `shannon.dock.open` stays single-sourced.
  const updateContextPanelOpen = useCallback((value: boolean | ((prev: boolean) => boolean)) => {
    setContextPanelOpen(prev => {
      const next = typeof value === 'function' ? value(prev) : value
      try { localStorage.setItem(DOCK_OPEN_KEY, next ? '1' : '0') } catch { /* quota / private mode */ }
      return next
    })
  }, [])

  const toggleContextPanel = useCallback(() => {
    updateContextPanelOpen(v => !v)
  }, [updateContextPanelOpen])
  // (P1-⑦ auto-dock: Chat's setContextPanelOpen(true) — the same persisted
  // setter — is the auto-dock entry point.)

  // B1 §4-9: prompt queue — enqueue/take/remove for the VISIBLE session.
  // The ref mirror is the source of truth for the synchronous decision
  // (cap check, drain head); state follows it for rendering. Returning
  // false on overflow lets the caller keep the draft; the toast is raised
  // here (messageFor works outside IntlProvider).
  const enqueuePrompt = useCallback((text: string, attachments: string[]): boolean => {
    const key = visibleSessionIdRef.current ?? ''
    const queue = promptQueuesRef.current.get(key) ?? []
    if (queue.length >= PROMPT_QUEUE_CAP) {
      toast.error(messageFor('chat.queue.full', { max: PROMPT_QUEUE_CAP }))
      return false
    }
    const item: PromptQueueItem = { id: ++queueItemIdRef.current, text, attachments }
    promptQueuesRef.current.set(key, [...queue, item])
    setPromptQueues(Object.fromEntries(promptQueuesRef.current))
    return true
  }, [])

  const dequeuePrompt = useCallback((): PromptQueueItem | null => {
    const key = visibleSessionIdRef.current ?? ''
    const queue = promptQueuesRef.current.get(key) ?? []
    if (queue.length === 0) return null
    const [head, ...rest] = queue
    if (rest.length > 0) promptQueuesRef.current.set(key, rest)
    else promptQueuesRef.current.delete(key)
    setPromptQueues(Object.fromEntries(promptQueuesRef.current))
    return head
  }, [])

  const removeQueuedPrompt = useCallback((id: number) => {
    const key = visibleSessionIdRef.current ?? ''
    const queue = promptQueuesRef.current.get(key) ?? []
    const next = queue.filter(item => item.id !== id)
    if (next.length > 0) promptQueuesRef.current.set(key, next)
    else promptQueuesRef.current.delete(key)
    setPromptQueues(Object.fromEntries(promptQueuesRef.current))
  }, [])

  // GB P2-10a: reorder one queued prompt within the visible session's FIFO
  // (queue chips' up/down). Out-of-range moves are no-ops.
  const moveQueuedPrompt = useCallback((id: number, delta: -1 | 1) => {
    const key = visibleSessionIdRef.current ?? ''
    const queue = promptQueuesRef.current.get(key) ?? []
    const from = queue.findIndex(item => item.id === id)
    const to = from + delta
    if (from < 0 || to < 0 || to >= queue.length) return
    const next = [...queue]
    const [item] = next.splice(from, 1)
    next.splice(to, 0, item)
    promptQueuesRef.current.set(key, next)
    setPromptQueues(Object.fromEntries(promptQueuesRef.current))
  }, [])

  const dropPromptQueue = useCallback((sessionId: string) => {
    if (!promptQueuesRef.current.has(sessionId)) return
    promptQueuesRef.current.delete(sessionId)
    setPromptQueues(Object.fromEntries(promptQueuesRef.current))
  }, [])

  // Single writer for the chat error surface so `errorKind` can never go
  // stale relative to `error` (a subsequent non-auth failure must clear the
  // auth classification, and a new send must clear both).
  const setChatError = useCallback((message: string | null, kind: 'auth' | 'other' = 'other') => {
    setError(message)
    setErrorKind(message == null ? null : kind)
  }, [])

  const refreshStatus = useCallback(async () => {
    try { setStatus(await api.getStatus()) } catch (e) { logSoftFailure('refresh status', e) }
  }, [])

  const refreshConfig = useCallback(async () => {
    try { setConfig(await api.getConfig()) } catch (e) { logSoftFailure('refresh config', e) }
  }, [])

  const refreshProviderStatus = useCallback(async () => {
    try { setProviderStatus(await api.getProviderStatus()) } catch (e) { logSoftFailure('refresh provider status', e) }
  }, [])

  const refreshModels = useCallback(async () => {
    try { setModels(await api.listModels()) } catch (e) { logSoftFailure('refresh models', e) }
  }, [])

  const refreshTasks = useCallback(async () => {
    try { setTasks(await api.listTasks()) } catch (e) { logSoftFailure('refresh tasks', e) }
  }, [])

  const refreshAgents = useCallback(async () => {
    try { setAgents(await api.listAgents()) } catch (e) { logSoftFailure('refresh agents', e) }
  }, [])

  const refreshMcpServers = useCallback(async () => {
    try { setMcpServers(await api.listMcpServers()) } catch (e) { logSoftFailure('refresh mcp servers', e) }
  }, [])

  const refreshBackgroundTasks = useCallback(async () => {
    try { setBackgroundTasks(await api.getBackgroundTasks()) } catch (e) { logSoftFailure('refresh background tasks', e) }
  }, [])

  const sendMessage = useCallback(async (
    message: string,
    filePaths?: string[],
    options?: { budgetBypass?: boolean },
  ): Promise<boolean> => {
    if (currentSessionId && goalOwnedSessionIds.includes(currentSessionId)) {
      setChatError(messageFor('goal.composer.blocked'))
      setSessionQuerying(windowSessionId ?? currentSessionId, false)
      return false
    }
    setChatError(null)
    // §P2-18: a new turn resets its session's stream buckets, not just the
    // visible projection (a previous turn may have failed mid-stream).
    const targetSessionId = windowSessionId ?? currentSessionId
    const targetKey = targetSessionId ?? ''
    streamingBucketsRef.current.set(targetKey, '')
    thinkingBucketsRef.current.set(targetKey, '')
    // R5-2: a new turn starts with a clean notice slate (the previous
    // turn's failover lines must not bleed into this one).
    streamNoticesBucketsRef.current.set(targetKey, [])
    setStreamNotices([])
    cancelStreamFlush()
    setStreamingText('')
    setThinkingText('')
    setActiveToolCalls([])
    // P2-19: a new turn starts with no progress chip (fresh run, fresh tool).
    setToolProgress(null)
    // GB P2-3: the new run's 四要素 slate — previous run's content is kept
    // until exactly this point (「结束保留至下一轮开始」).
    const prevRun = runProcessRef.current
    setRunProcess(runBegin({
      at: Date.now(),
      message,
      attachments: filePaths ?? [],
    }))
    // B1 P1-5: the run is tracked on ITS session — other sessions keep a
    // usable composer while this one streams.
    setSessionQuerying(targetSessionId, true)
    // A-4 fix: the optimistic user message carries its attachments in the
    // backend ChatMessage's wire shape (commands.rs: file_attachments of
    // {name, path, size}) so the just-sent bubble shows its attachment
    // previews immediately instead of only after the next reload. The size
    // is unknowable client-side pre-send — 0 renders as a placeholder until
    // a reload brings the recorded message (with real metadata) back.
    //
    // A-11 fix: the append's exact object doubles as the rollback handle.
    // The old rollback matched role+content and deleted the LAST match,
    // which can be the WRONG bubble: two identical texts in flight at once
    // (double-Enter before isQuerying flips, drain vs manual send) let the
    // first send's failure delete the second send's bubble, and a session
    // switch that reloaded backend truth between the append and the
    // rejection made it delete the recorded copy of the same text. Rolling
    // back by reference is exact on both counts (the pending-id variant
    // without the success-side promotion pass — the reference IS the unique
    // pending marker, and a settled message simply keeps it, inert: no code
    // reads it and the next switch/reload replaces it with backend truth).
    // After such a reload the optimistic object is no longer in state, so
    // the rollback is a no-op — there is nothing left to roll back.
    const optimistic: ChatMessage = {
      role: 'user',
      content: message,
      timestamp: Date.now(),
      file_attachments: filePaths?.map(p => ({ name: basenameOf(p), path: p, size: 0 })),
    }
    setMessages(prev => [...prev, optimistic])
    try {
      // P1-1 fix: explicit session routing — the window targets its own
      // session, the main window its current one; the backend never routes
      // via the shared active pointer for these calls. `null` (no session
      // yet) keeps the backend's legacy active fallback.
      const resp = await api.sendMessage(
        message,
        filePaths,
        options?.budgetBypass,
        targetSessionId ?? undefined,
      )
      // P0-3: the backend reports refused attachments per file instead of
      // dropping them silently. Partial success — the send itself stands;
      // each refusal gets its own "«file» was not sent: «reason»" toast.
      reportRejectedAttachments(resp.rejected_attachments)
      // W3-4: stash this turn's injected-memory snapshot so QUERY_COMPLETED
      // can attach it to the committed assistant message (citation chips).
      // A new send to the same session overwrites — the stale list can never
      // reach a later turn's bubble.
      pendingInjectedMemoriesRef.current.set(
        targetKey,
        Array.isArray(resp.injected_memories) ? resp.injected_memories : [],
      )
      return true
    } catch (e) {
      // P0-4 fix: the backend rejected the send BEFORE recording the user
      // message (budget-exceeded pre-turn guard, goal-owned guard,
      // concurrent-query guard — see `send_message`), so roll back the
      // optimistic append above. Without this, "Continue (ignore once)"
      // re-sends the same text and the rejected message renders twice.
      // A-11 fix: by the append's own reference — never by content (see the
      // comment on `optimistic` above).
      setMessages(prev => {
        const idx = prev.lastIndexOf(optimistic)
        if (idx < 0) return prev
        const next = [...prev]
        next.splice(idx, 1)
        return next
      })
      setChatError(describeBackendError(String(e), messageFor))
      setSessionQuerying(targetSessionId, false)
      // Round-1 review (Minor-4): the send was rejected BEFORE recording the
      // user message — no run ever started, so the pre-send snapshot (with
      // its sources/outputs) comes back instead of a phantom "Running".
      if (prevRun) setRunProcess(prevRun)
      return false
    }
  }, [currentSessionId, goalOwnedSessionIds, windowSessionId, setSessionQuerying, cancelStreamFlush, setChatError])

  // P1-1 fix: cancelQuery's targetSessionId mirrors sendMessage's — both
  // route explicitly instead of re-pointing the shared pointer.
  const cancelQuery = useCallback(async () => {
    const targetSessionId = windowSessionId ?? currentSessionId
    try {
      await api.cancelQuery(targetSessionId ?? undefined)
    } catch (e) {
      // B0 P2-11: messageFor works outside IntlProvider (the provider may
      // not wrap this context's call sites) — same helper SESSION_AUTO_
      // UNARCHIVED uses.
      toastError(messageFor('chat.error.cancelFailed'), e)
    }
  }, [windowSessionId, currentSessionId])

  const createSession = useCallback(async () => {
    try {
      const id = await api.newSession()
      setCurrentSessionId(id)
      setMessages([])
      loadedSessionRef.current = id
      setStreamingText('')
      setThinkingText('')
      setStreamNotices([])
      setActiveToolCalls([])
      setToolProgress(null)
      // GB P2-3: a fresh session starts with an empty run tab.
      setRunProcess(initialRunProcess())
      await refreshSessions()
    } catch (e) { setChatError(String(e)) }
  }, [refreshSessions, setChatError])

  const createSessionInWorktree = useCallback(async () => {
    let id: string | null = null
    try {
      id = await api.newSession()
      const title = `Session ${id.slice(0, 8)}`
      await api.createSessionWorktree(id, title)
      const msgs = await api.switchSession(id)
      setCurrentSessionId(id)
      setMessages(msgs)
      loadedSessionRef.current = id
      setStreamingText('')
      setThinkingText('')
      setStreamNotices([])
      setActiveToolCalls([])
      setToolProgress(null)
      // GB P2-3: a fresh session starts with an empty run tab.
      setRunProcess(initialRunProcess())
      await refreshSessions()
    } catch (e) {
      setChatError(String(e))
      if (id) {
        // Worktree creation failed after session was created — clear
        // current session to avoid UI showing a session whose working_dir
        // was never bound to a worktree.
        setCurrentSessionId(null)
        setMessages([])
        loadedSessionRef.current = null
      }
    }
  }, [refreshSessions, setChatError])

  // B1 P2-3: a session switch now carries a transient loading flag for the
  // message area's skeleton. Same-session calls (and Chat remounts that
  // re-run against the same id) never set it — no skeleton flash.
  //
  // Review P1-3 (B1-9): every call takes a monotonic token BEFORE the await;
  // only the latest token may land state. A slower earlier response (or its
  // error) is dropped on arrival, and its `finally` leaves the skeleton flag
  // to the newer request that superseded it.
  const switchToSession = useCallback(async (id: string) => {
    const token = ++switchTokenRef.current
    const isSwitch = id !== visibleSessionIdRef.current
    if (isSwitch) {
      setSwitchingSession(true)
      // A-5 fix: the error banner is a visible-session readout (QUERY_FAILED
      // only sets it for the session on screen), but nothing cleared it when
      // the user moved to another session — the previous session's failure
      // banner (auth included) followed them there. Clear it when a switch
      // STARTS, so the old banner never flashes over the new session; a
      // failing switch re-fills it in the catch below, and a superseded
      // switch can't (its token check drops the stale error). Same-session
      // reloads keep the banner — it belongs to the session still on screen.
      setChatError(null)
    }
    try {
      // Already on the loaded session: nothing to fetch. A re-replace here
      // could only destroy newer local state (the optimistic bubbles above).
      if (!isSwitch && loadedSessionRef.current === id) return
      const msgs = await api.switchSession(id)
      if (token !== switchTokenRef.current) return
      setCurrentSessionId(id)
      setMessages(msgs)
      loadedSessionRef.current = id
      // §P2-18: project the switched-to session's own stream buckets — a
      // background run keeps streaming into them while another session is
      // on screen, so a single shared buffer would show foreign tokens.
      cancelStreamFlush()
      setStreamingText(streamingBucketsRef.current.get(id) ?? '')
      setThinkingText(thinkingBucketsRef.current.get(id) ?? '')
      // R5-2: the notice list is the switched-to session's own bucket.
      setStreamNotices(streamNoticesBucketsRef.current.get(id) ?? [])
      setActiveToolCalls([])
      // P2-19: single visible-session value — switching drops the previous
      // session's pill (background progress was never captured anyway).
      setToolProgress(null)
      // GB P2-3: the run tab belongs to the session that ran it.
      setRunProcess(initialRunProcess())
      // Batch B2: opening the session marks a prior failure as seen.
      const prev = sessionActivityRef.current.get(id)
      if (prev?.failed) {
        sessionActivityRef.current.set(id, { ...prev, failed: false })
        setSessionActivity(Object.fromEntries(sessionActivityRef.current))
      }
    } catch (e) {
      if (token !== switchTokenRef.current) return
      setChatError(String(e))
    } finally {
      // A superseded request must not clear the newer request's skeleton.
      if (isSwitch && token === switchTokenRef.current) setSwitchingSession(false)
    }
  }, [cancelStreamFlush, setChatError])

  const deleteSessionAction = useCallback(async (id: string) => {
    try {
      await api.deleteSession(id)
      // §P2-18: drop the deleted session's stream buckets.
      streamingBucketsRef.current.delete(id)
      thinkingBucketsRef.current.delete(id)
      // R5-2: its retry notices die with it too.
      streamNoticesBucketsRef.current.delete(id)
      // B1 §4-9: its queued prompts die with the session too.
      dropPromptQueue(id)
      if (currentSessionId === id) {
        setMessages([])
        setStreamNotices([])
        setCurrentSessionId(null)
        loadedSessionRef.current = null
      }
      await refreshSessions()
    } catch (e) { setChatError(String(e)) }
  }, [currentSessionId, refreshSessions, dropPromptQueue, setChatError])

  const renameSessionAction = useCallback(async (id: string, title: string) => {
    try {
      await api.renameSession(id, title)
      await refreshSessions()
    } catch (e) { setChatError(String(e)) }
  }, [refreshSessions, setChatError])

  const respondPermissionAction = useCallback(async (
    requestId: string,
    allow: boolean,
    options?: { note?: string; scope?: 'once' | 'always_tool' },
  ) => {
    try {
      await api.respondPermission(requestId, allow, options)
      setPermissionRequest(null)
      // Batch B2: resolve the rail's amber dot for the prompt's session.
      if (permissionRequest?.session_id) noteSessionApproval(permissionRequest.session_id, false)
    } catch (e) {
      setChatError(String(e))
      // B0 P0-1: re-throw so awaiting callers (OPC task page) can toast the
      // failure instead of reporting success. Fire-and-forget callers
      // (Header) attach a no-op catch of their own.
      throw e
    }
  }, [permissionRequest, noteSessionApproval, setChatError])

  const refreshCheckpoints = useCallback(async () => {
    if (!currentSessionId) {
      setCheckpoints([])
      return
    }
    try {
      setCheckpoints(await api.listCheckpoints(currentSessionId))
    } catch (e) { logSoftFailure('refresh checkpoints', e) }
  }, [currentSessionId])

  // Checkpoints are recorded when a query completes — refresh when the
  // querying flag settles and when the session changes.
  useEffect(() => {
    if (!isQuerying) void refreshCheckpoints()
  }, [isQuerying, refreshCheckpoints])

  const refreshFeedback = useCallback(async () => {
    if (!currentSessionId) {
      setFeedback({})
      return
    }
    try {
      setFeedback(await api.listMessageFeedback(currentSessionId))
    } catch (e) { logSoftFailure('refresh feedback', e) }
  }, [currentSessionId])

  useEffect(() => {
    void refreshFeedback()
  }, [refreshFeedback])

  const recordFeedbackAction = useCallback(async (key: string, rating: FeedbackRating | null) => {
    if (!currentSessionId) return
    setFeedback(prev => {
      const next = { ...prev }
      if (rating == null) delete next[key]
      else next[key] = rating
      return next
    })
    try {
      await api.recordMessageFeedback(currentSessionId, key, rating)
    } catch (e) {
      console.warn('recordMessageFeedback failed:', e)
      void refreshFeedback()
    }
  }, [currentSessionId, refreshFeedback])

  const rewindSessionAction = useCallback(async (turnIndex: number) => {
    if (!currentSessionId) return
    try {
      const msgs = await api.rewindSession(currentSessionId, turnIndex)
      setMessages(msgs)
      streamingBucketsRef.current.set(currentSessionId, '')
      thinkingBucketsRef.current.set(currentSessionId, '')
      // R5-2: the turn history they replace carries the old notices too.
      streamNoticesBucketsRef.current.set(currentSessionId, [])
      setStreamNotices([])
      cancelStreamFlush()
      setStreamingText('')
      setThinkingText('')
      setActiveToolCalls([])
      setToolProgress(null)
      // GB P2-3: a fresh session starts with an empty run tab.
      setRunProcess(initialRunProcess())
      await refreshSessions()
      await refreshCheckpoints()
    } catch (e) {
      setChatError(String(e))
      throw e
    }
  }, [currentSessionId, refreshSessions, refreshCheckpoints, cancelStreamFlush, setChatError])
  const compactSessionAction = useCallback(async () => {
    if (!currentSessionId) throw new Error('no active session')
    try {
      const result = await api.compactSession(currentSessionId)
      setMessages(result.messages)
      streamingBucketsRef.current.set(currentSessionId, '')
      thinkingBucketsRef.current.set(currentSessionId, '')
      // R5-2: the turn history they replace carries the old notices too.
      streamNoticesBucketsRef.current.set(currentSessionId, [])
      setStreamNotices([])
      cancelStreamFlush()
      setStreamingText('')
      setThinkingText('')
      setActiveToolCalls([])
      setToolProgress(null)
      // GB P2-3: a fresh session starts with an empty run tab.
      setRunProcess(initialRunProcess())
      await refreshSessions()
      await refreshCheckpoints()
      return result
    } catch (e) {
      setChatError(String(e))
      throw e
    }
  }, [currentSessionId, refreshSessions, refreshCheckpoints, cancelStreamFlush, setChatError])

  // P0-2/P2-⑥: derive both the goal-owned id set (composer guard) and the
  // full per-session run map (sidebar badge) from one fetch.
  const applyGoalRuns = useCallback((runs: GoalRunDto[]) => {
    setGoalOwnedSessionIds(runs.filter(r => r.status === 'running' || r.status === 'paused').map(r => r.sessionId))
    setGoalRunsBySession(Object.fromEntries(runs.map(r => [r.sessionId, r])))
  }, [])

  // Register Tauri event listeners
  useEffect(() => {
    const unlisteners: UnlistenFn[] = []
    let cancelled = false

    async function register() {
      const handlers = [
        listen(EVENT_NAMES.QUERY_TEXT, (e) => {
          const p = e.payload as { content: string; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'event')
          // §P2-18: append to this session's own bucket; only the visible
          // session's bucket is projected into state, so tokens from a
          // background session never land in the on-screen stream.
          // B1 P2-13: the projection itself is throttled — tokens coalesce
          // in the bucket and the flush re-reads it at most every 50ms.
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          const next = (streamingBucketsRef.current.get(key) ?? '') + p.content
          streamingBucketsRef.current.set(key, next)
          if (key === visibleKey) scheduleStreamFlush()
        }),
        listen(EVENT_NAMES.QUERY_TOOL_START, (e) => {
          const p = e.payload as { tool_use_id: string; tool_name: string; tool_input: unknown; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'tool-start', p.tool_name)
          // B1 P1-5: tool cards belong to the session that runs them — a
          // background session's tools never bleed into the visible list
          // (same visibility rule as the text/thinking buckets).
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          if (key !== visibleKey) return
          // P2-19: a new tool starts from a clean slate — the previous
          // tool's last percentage/message must not label this one until it
          // reports its own progress.
          setToolProgress(null)
          setRunProcess(prev => runToolStart(prev, p.tool_name, p.tool_input, Date.now()))
          // A-7 fix: the backend can re-emit a tool-start for a card this
          // session already tracks (resume/replay paths) — a second start
          // for a known tool_use_id must keep the existing card instead of
          // appending a duplicate (the first start wins; the result event
          // resolves the shared id either way).
          setActiveToolCalls(prev => {
            if (prev.some(tc => tc.tool_use_id === p.tool_use_id)) return prev
            return [...prev, {
              tool_use_id: p.tool_use_id,
              tool_name: p.tool_name,
              tool_input: p.tool_input,
              status: 'running',
              started_at: Date.now(),
            }]
          })
        }),
        listen(EVENT_NAMES.QUERY_TOOL_RESULT, (e) => {
          const p = e.payload as { tool_use_id: string; result: string; is_error: boolean; meta?: unknown; tokens_used?: number; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'tool-end')
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          if (key !== visibleKey) return
          setActiveToolCalls(prev => prev.map(tc => {
            if (tc.tool_use_id !== p.tool_use_id) return tc
            // P1-⑤ telemetry: client-side wall-clock duration for the card.
            const duration_ms = tc.started_at != null ? Date.now() - tc.started_at : undefined
            return { ...tc, result: p.result, is_error: p.is_error, status: p.is_error ? 'error' : 'completed', duration_ms, meta: p.meta, tokens_used: p.tokens_used }
          }))
        }),
        listen(EVENT_NAMES.QUERY_TOOL_PROGRESS, (e) => {
          const p = e.payload as { tool_use_id: string; progress: number; message: string; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'event')
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          if (key !== visibleKey) return
          setActiveToolCalls(prev => prev.map(tc =>
            tc.tool_use_id === p.tool_use_id
              ? { ...tc, progress: p.progress, progress_message: p.message }
              : tc
          ))
          // P2-19: same visible-only slot for the RunStatusLine pill. The
          // backend sends a FRACTION (−1 indeterminate, 0..=1 determinate —
          // agent_loop.rs); normalize to the 0..=100 percent the pill
          // renders here, so the wire contract lives in exactly one place.
          const frac = p.progress
          setToolProgress({
            progress:
              typeof frac === 'number' && frac >= 0 && frac <= 1
                ? Math.round(frac * 100)
                : undefined,
            message: p.message,
          })
          // GB P2-3: the progress line doubles as the run tab's summary.
          setRunProcess(prev => runToolProgress(prev, p.message))
        }),
        listen(EVENT_NAMES.QUERY_NOTICE, (e) => {
          // R5-2: failover / key-rotation notices (engine continued —
          // informational). Per-session bucket like the stream text; only
          // the visible session's bucket is projected into state.
          const p = e.payload as QueryNoticeEvent
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          const notice: StreamNotice = {
            id: ++streamNoticeIdRef.current,
            kind: p.kind === 'key_rotation' ? 'key_rotation' : 'failover',
            message: p.message,
          }
          const bucket = [...(streamNoticesBucketsRef.current.get(key) ?? []), notice]
          if (bucket.length > STREAM_NOTICES_CAP) bucket.splice(0, bucket.length - STREAM_NOTICES_CAP)
          streamNoticesBucketsRef.current.set(key, bucket)
          if (key === visibleKey) setStreamNotices(bucket)
        }),
        listen(EVENT_NAMES.SUBAGENT_START, (e) => {
          const p = e.payload as SubAgentLive
          setSubagentLive({ agentId: p.agentId, agentName: p.agentName, team: p.team ?? null })
        }),
        listen(EVENT_NAMES.SUBAGENT_STOP, (e) => {
          const p = e.payload as { agentId: string }
          setSubagentLive(prev => (prev && prev.agentId === p.agentId ? null : prev))
        }),
        listen(EVENT_NAMES.QUERY_THINKING, (e) => {
          const p = e.payload as { content: string; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'event')
          // §P2-18: same per-session bucketing as QUERY_TEXT (B1 P2-13: and
          // the same throttled projection).
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          const next = (thinkingBucketsRef.current.get(key) ?? '') + p.content
          thinkingBucketsRef.current.set(key, next)
          if (key === visibleKey) scheduleStreamFlush()
        }),
        listen(EVENT_NAMES.QUERY_USAGE, (e) => {
          const p = e.payload as UsagePayload
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'event')
          // B1-16 (review §5 骨架): the footer's token/cost line is a
          // visible-session readout — a background session's usage must not
          // overwrite it (same visibleKey projection as QUERY_TEXT).
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          if (key !== visibleKey) return
          setUsage(p)
        }),
        listen(EVENT_NAMES.QUERY_COMPLETED, (e) => {
          const sid = (e.payload as { session_id?: string }).session_id
          if (!isEventForCurrentWindow(sid, windowSessionId)) return
          noteSessionActivity(sid, 'end')
          // §P2-18: the completed session commits ITS OWN bucket, and UI
          // mutations only fire when it is the one on screen — a background
          // session finishing must not append to (or clear) another
          // session's visible stream. The committed text is read from the
          // bucket (not a streamingText mirror), so StrictMode can't
          // double-append and concurrent sessions can't cross-commit.
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = sid ?? visibleKey
          // B1 P1-5/P2-13: the run's own session settles regardless of
          // visibility; a pending throttled flush must die BEFORE the
          // buckets are cleared so it can't resurrect stale text.
          setSessionQuerying(key, false)
          cancelStreamFlush()
          const finalText = streamingBucketsRef.current.get(key) ?? ''
          streamingBucketsRef.current.set(key, '')
          thinkingBucketsRef.current.set(key, '')
          // W3-4: the run is over — pop its citation snapshot regardless of
          // visibility so it can never leak onto a later turn's bubble.
          const citations = pendingInjectedMemoriesRef.current.get(key)
          pendingInjectedMemoriesRef.current.delete(key)
          if (key === visibleKey) {
            setSubagentLive(null)
            if (finalText) {
              setMessages(msgs => [...msgs, {
                role: 'assistant',
                content: finalText,
                timestamp: Date.now(),
                // W3-4: citation chips ride only a non-empty snapshot.
                ...(citations && citations.length > 0 ? { injected_memories: citations } : {}),
              }])
            }
            setStreamingText('')
            setThinkingText('')
            // Review P2-4 (round 2): completed tool cards must not linger
            // under the committed reply until the next send/switch.
            setActiveToolCalls([])
            // P2-19: the run ended — no progress chip may outlive it.
            setToolProgress(null)
            // GB P2-3: the run tab settles into its "done" snapshot (kept
            // until the next send).
            setRunProcess(prev => runEnd(prev, Date.now(), false))
            refreshStatus()
          }
        }),
        listen(EVENT_NAMES.QUERY_FAILED, (e) => {
          // Desktop emits `error_kind` ("auth" | "other") on the payload —
          // classified Rust-side from the engine's AuthenticationFailed
          // text (events::classify_query_error_kind), so the JS side never
          // string-matches provider errors.
          const p = e.payload as { error: string; error_kind?: string; session_id?: string }
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          noteSessionActivity(p.session_id, 'fail')
          // §P2-18: like QUERY_COMPLETED, failure state is scoped to the
          // session that owns the run — a background run failing must not
          // overwrite the on-screen session's composer/error state (its
          // failure is still surfaced by the rail's red dot via
          // noteSessionActivity). B0 P1-2: a failed run leaves no ghost
          // bubble — drop the run's buckets AND the visible projections.
          // Persisting the partial text needs a backend commit path (none
          // exists yet), so clearing is the approved behavior.
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = p.session_id ?? visibleKey
          setSessionQuerying(key, false)
          cancelStreamFlush()
          streamingBucketsRef.current.set(key, '')
          thinkingBucketsRef.current.set(key, '')
          // W3-4: a failed run commits no bubble — drop its citation
          // snapshot so it can't leak onto a later turn's.
          pendingInjectedMemoriesRef.current.delete(key)
          if (key === visibleKey) {
            setChatError(p.error, p.error_kind === 'auth' ? 'auth' : 'other')
            setStreamingText('')
            setThinkingText('')
            setActiveToolCalls([])
            // P2-19: run failed — clear the progress pill with the cards.
            setToolProgress(null)
            // GB P2-3: the run tab marks the failure (content still kept).
            setRunProcess(prev => runEnd(prev, Date.now(), true))
          }
        }),
        listen(EVENT_NAMES.QUERY_CANCELLED, (e) => {
          const sid = (e.payload as { session_id?: string }).session_id
          if (!isEventForCurrentWindow(sid, windowSessionId)) return
          noteSessionActivity(sid, 'end')
          // B0 P1-2: same ghost-bubble cleanup as QUERY_FAILED — the
          // cancelled session's buckets and, when visible, the projections.
          const visibleKey = visibleSessionIdRef.current ?? ''
          const key = sid ?? visibleKey
          setSessionQuerying(key, false)
          cancelStreamFlush()
          streamingBucketsRef.current.set(key, '')
          thinkingBucketsRef.current.set(key, '')
          // W3-4: a cancelled run commits no bubble — same snapshot drop.
          pendingInjectedMemoriesRef.current.delete(key)
          if (key === visibleKey) {
            setStreamingText('')
            setThinkingText('')
            setActiveToolCalls([])
            // P2-19: run cancelled — clear the progress pill with the cards.
            setToolProgress(null)
            // GB P2-3: settle the run tab (not a failure — the user stopped it).
            setRunProcess(prev => runEnd(prev, Date.now(), false))
          }
        }),
        listen(EVENT_NAMES.PERMISSION_REQUEST, (e) => {
          const p = e.payload as PermissionRequest
          // Window mode: only prompt for this window's own session — a
          // foreign session's approval dialog must not pop up here.
          if (!isEventForCurrentWindow(p.session_id, windowSessionId)) return
          // Batch B2: amber dot on the owning session's rail row.
          noteSessionApproval(p.session_id, true)
          setPermissionRequest(p)
        }),
        listen(EVENT_NAMES.SESSIONS_UPDATED, () => { refreshSessions() }),
        // 卡A resume-unarchive: opening an archived session silently
        // unarchives it — toast so the user knows why it left the 已归档
        // section (messageFor works outside IntlProvider).
        listen(EVENT_NAMES.SESSION_AUTO_UNARCHIVED, (e) => {
          const p = e.payload as { session_id: string; title?: string }
          const title = p.title?.trim()
          toast.success(
            title
              ? messageFor('sidebar.sessions.archived.autoUnarchived', { title })
              : messageFor('sidebar.sessions.archived.autoUnarchived.untitled'),
          )
        }),
        listen(EVENT_NAMES.CONFIG_UPDATED, () => {
          refreshConfig()
          // Provider saves/activations emit CONFIG_UPDATED — keep the
          // gating snapshot (active provider / has_api_key) in lockstep.
          void refreshProviderStatus()
          // P1-12: provider edits from outside this window (CLI / a session
          // window) can change the chat model catalog — refresh it so the
          // model picker reflects the new providers.toml without a restart.
          // refreshModels is the CatalogContext-exposed refresh; the event
          // is low-frequency so no debounce is needed.
          void refreshModels()
        }),
        // B3 P1-25: a background-task change can also mean a new/finished
        // agent run — refresh the agents inventory alongside the tasks so
        // the OPC load/workflow views don't need an app restart to catch up.
        listen(EVENT_NAMES.BACKGROUND_TASKS_UPDATED, () => {
          refreshBackgroundTasks()
          void refreshAgents()
        }),
        // P0-2: track which sessions a goal run owns, so the composer can
        // block manual sends while a run is driving the conversation.
        listen(EVENT_NAMES.GOAL_UPDATED, () => {
          void api.listGoalRuns()
            .then(runs => {
              if (cancelled) return
              applyGoalRuns(runs)
            })
            .catch((e) => { logSoftFailure('refresh goal runs', e) })
        }),
      ]

      const results = await Promise.all(handlers)
      if (cancelled) {
        results.forEach(fn => fn())
        return
      }
      unlisteners.push(...results)
    }

    register()
    return () => {
      cancelled = true
      unlisteners.forEach(fn => fn())
      // B1 P2-13: a pending streaming flush must not fire post-unmount.
      cancelStreamFlush()
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Initial data load. Each surface's failure is recorded (the refresh*
  // wrappers still console.warn for the log) and the first failure is raised
  // as initError so the UI can offer a retry instead of rendering empty.
  const loadInitialData = useCallback(async () => {
    setLoading(true)
    setInitError(null)
    const failures: string[] = []
    const record = (label: string, p: Promise<unknown>) =>
      p.catch((e: unknown) => {
        console.warn(`${label} failed:`, e)
        failures.push(`${label}: ${String(e)}`)
      })
    await Promise.all([
      record('refreshStatus', refreshStatus()),
      record('refreshConfig', refreshConfig()),
      record('refreshProviderStatus', refreshProviderStatus()),
      record('refreshSessions', refreshSessions()),
      record('refreshModels', refreshModels()),
      record('refreshTasks', refreshTasks()),
      record('refreshAgents', refreshAgents()),
      record('refreshMcpServers', refreshMcpServers()),
      record('refreshBackgroundTasks', refreshBackgroundTasks()),
      // P1-1 window mode: auto-switch to the window's own session (instead
      // of the backend's global active session) and load its messages.
      //
      // A-6 fix: the main window's cold start filled `messages` from
      // get_conversation but left `currentSessionId` null — RunStatusLine
      // had no session to time against and every currentSessionId-gated
      // action (checkpoints, feedback, explicit send routing, composer
      // guards) stayed half-bound until the first manual switch. The
      // conversation comes from the backend's ACTIVE session
      // (get_or_create_active materializes it), so bind that id — read
      // AFTER the conversation lands (get_active_session_id is a read-only
      // peek at the same pointer, not a re-materialization).
      record('getConversation', windowSessionId != null
        ? switchToSession(windowSessionId)
        : api.getConversation()
          .then(messages => {
            setMessages(messages)
            return api.getActiveSessionId()
          })
          .then(id => {
            if (id) {
              setCurrentSessionId(id)
              loadedSessionRef.current = id
            }
          })),
      record('goalOwnedSessions', api.listGoalRuns().then(runs => applyGoalRuns(runs))),
    ])
    if (failures.length > 0) setInitError(failures[0])
    setLoading(false)
  }, [refreshStatus, refreshConfig, refreshProviderStatus, refreshSessions, refreshModels, refreshTasks,
    refreshAgents, refreshMcpServers, refreshBackgroundTasks, windowSessionId, switchToSession, applyGoalRuns])

  useEffect(() => {
    void loadInitialData()
  }, [loadInitialData])

  const visibleKey = windowSessionId ?? currentSessionId ?? ''
  const chatValue = useMemo<ChatContextValue>(() => ({
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, toolProgress, streamNotices, usage, runProcess,
    sendMessage, cancelQuery,
    promptQueue: promptQueues[visibleKey] ?? [],
    enqueuePrompt, dequeuePrompt, removeQueuedPrompt, moveQueuedPrompt,
    contextPanelOpen, toggleContextPanel, setContextPanelOpen: updateContextPanelOpen,
    checkpoints, rewindSession: rewindSessionAction, compactSession: compactSessionAction,
    feedback, recordFeedback: recordFeedbackAction,
  }), [messages, streamingText, thinkingText, isQuerying, activeToolCalls, toolProgress, streamNotices, usage, runProcess, sendMessage, cancelQuery,
    promptQueues, visibleKey, enqueuePrompt, dequeuePrompt, removeQueuedPrompt, moveQueuedPrompt,
    contextPanelOpen, toggleContextPanel, updateContextPanelOpen, checkpoints, rewindSessionAction, compactSessionAction, feedback, recordFeedbackAction])

  const sessionValue = useMemo<SessionContextValue>(() => ({
    sessions, sessionActivity, goalRunsBySession, subagentLive, currentSessionId, windowSessionId, switchingSession, createSession, createSessionInWorktree, switchSession: switchToSession,
    deleteSession: deleteSessionAction, renameSession: renameSessionAction, refreshSessions,
    sessionSources, addSessionSource, removeSessionSource,
  }), [sessions, sessionActivity, goalRunsBySession, subagentLive, currentSessionId, windowSessionId, switchingSession, createSession, createSessionInWorktree, switchToSession,
    deleteSessionAction, renameSessionAction, refreshSessions, sessionSources, addSessionSource, removeSessionSource])

  const catalogValue = useMemo<CatalogContextValue>(() => ({
    status, config, providerStatus, models, agents, tasks, mcpServers, backgroundTasks, permissionRequest,
    error, errorKind, loading, initError, retryInit: loadInitialData, refreshStatus, refreshConfig, refreshModels, refreshTasks, refreshAgents,
    refreshMcpServers, refreshBackgroundTasks, respondPermission: respondPermissionAction,
  }), [status, config, providerStatus, models, agents, tasks, mcpServers, backgroundTasks, permissionRequest,
    error, errorKind, loading, initError, loadInitialData, refreshStatus, refreshConfig, refreshModels, refreshTasks, refreshAgents,
    refreshMcpServers, refreshBackgroundTasks, respondPermissionAction])

  return (
    <CatalogContext.Provider value={catalogValue}>
      <SessionContext.Provider value={sessionValue}>
        <ChatProvider value={chatValue}>
          {children}
        </ChatProvider>
      </SessionContext.Provider>
    </CatalogContext.Provider>
  )
}

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
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import * as api from '@/lib/tauri-api'
import {
  EVENT_NAMES,
  type ChatMessage,
  type ToolCall,
  type SessionInfo,
  type StatusResponse,
  type DesktopConfig,
  type ModelInfo,
  type PermissionRequest,
  type BackgroundTaskInfo,
  type TaskItem,
  type AgentInfo,
  type UsagePayload,
  type McpServerInfo,
} from '@/types'
import { ChatContext, useChat, type ChatContextValue } from './ChatContext'
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

export function AppProvider({ children }: { children: ReactNode }) {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [streamingText, setStreamingText] = useState('')
  const [thinkingText, setThinkingText] = useState('')
  const [isQuerying, setIsQuerying] = useState(false)
  const [activeToolCalls, setActiveToolCalls] = useState<ToolCall[]>([])
  const [usage, setUsage] = useState<UsagePayload | null>(null)
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null)
  const [status, setStatus] = useState<StatusResponse | null>(null)
  const [config, setConfig] = useState<DesktopConfig | null>(null)
  const [models, setModels] = useState<ModelInfo[]>([])
  const [permissionRequest, setPermissionRequest] = useState<PermissionRequest | null>(null)
  const [backgroundTasks, setBackgroundTasks] = useState<BackgroundTaskInfo[]>([])
  const [tasks, setTasks] = useState<TaskItem[]>([])
  const [agents, setAgents] = useState<AgentInfo[]>([])
  const [mcpServers, setMcpServers] = useState<McpServerInfo[]>([])
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [_currentQueryId, setCurrentQueryId] = useState<string | null>(null)

  // Mirror streamingText into a ref so the QUERY_COMPLETED handler can read
  // the final streamed text synchronously. This replaces the prior pattern of
  // calling setMessages from inside the setStreamingText updater, which
  // double-fires under React StrictMode and could append the assistant
  // message twice.
  const streamingTextRef = useRef('')
  streamingTextRef.current = streamingText

  const refreshSessions = useCallback(async () => {
    try { setSessions(await api.listSessions()) } catch (e) { console.warn('refreshSessions failed:', e) }
  }, [])

  const refreshStatus = useCallback(async () => {
    try { setStatus(await api.getStatus()) } catch (e) { console.warn('refreshStatus failed:', e) }
  }, [])

  const refreshConfig = useCallback(async () => {
    try { setConfig(await api.getConfig()) } catch (e) { console.warn('refreshConfig failed:', e) }
  }, [])

  const refreshModels = useCallback(async () => {
    try { setModels(await api.listModels()) } catch (e) { console.warn('refreshModels failed:', e) }
  }, [])

  const refreshTasks = useCallback(async () => {
    try { setTasks(await api.listTasks()) } catch (e) { console.warn('refreshTasks failed:', e) }
  }, [])

  const refreshAgents = useCallback(async () => {
    try { setAgents(await api.listAgents()) } catch (e) { console.warn('refreshAgents failed:', e) }
  }, [])

  const refreshMcpServers = useCallback(async () => {
    try { setMcpServers(await api.listMcpServers()) } catch (e) { console.warn('refreshMcpServers failed:', e) }
  }, [])

  const refreshBackgroundTasks = useCallback(async () => {
    try { setBackgroundTasks(await api.getBackgroundTasks()) } catch (e) { console.warn('refreshBackgroundTasks failed:', e) }
  }, [])

  const sendMessage = useCallback(async (message: string, filePaths?: string[]) => {
    setError(null)
    setStreamingText('')
    setThinkingText('')
    setActiveToolCalls([])
    setIsQuerying(true)
    setMessages(prev => [...prev, { role: 'user', content: message, timestamp: Date.now() }])
    try {
      const resp = await api.sendMessage(message, filePaths)
      setCurrentQueryId(resp.query_id)
    } catch (e) {
      setError(String(e))
      setIsQuerying(false)
    }
  }, [])

  const cancelQuery = useCallback(async () => {
    try { await api.cancelQuery() } catch (e) { console.warn("AppContext error:", e) }
  }, [])

  const createSession = useCallback(async () => {
    try {
      const id = await api.newSession()
      setCurrentSessionId(id)
      setMessages([])
      setStreamingText('')
      setThinkingText('')
      setActiveToolCalls([])
      await refreshSessions()
    } catch (e) { setError(String(e)) }
  }, [refreshSessions])

  const createSessionInWorktree = useCallback(async () => {
    let id: string | null = null
    try {
      id = await api.newSession()
      const title = `Session ${id.slice(0, 8)}`
      await api.createSessionWorktree(id, title)
      const msgs = await api.switchSession(id)
      setCurrentSessionId(id)
      setMessages(msgs)
      setStreamingText('')
      setThinkingText('')
      setActiveToolCalls([])
      await refreshSessions()
    } catch (e) {
      setError(String(e))
      if (id) {
        // Worktree creation failed after session was created — clear
        // current session to avoid UI showing a session whose working_dir
        // was never bound to a worktree.
        setCurrentSessionId(null)
        setMessages([])
      }
    }
  }, [refreshSessions])

  const switchToSession = useCallback(async (id: string) => {
    try {
      const msgs = await api.switchSession(id)
      setCurrentSessionId(id)
      setMessages(msgs)
      setStreamingText('')
      setThinkingText('')
      setActiveToolCalls([])
    } catch (e) { setError(String(e)) }
  }, [])

  const deleteSessionAction = useCallback(async (id: string) => {
    try {
      await api.deleteSession(id)
      if (currentSessionId === id) {
        setMessages([])
        setCurrentSessionId(null)
      }
      await refreshSessions()
    } catch (e) { setError(String(e)) }
  }, [currentSessionId, refreshSessions])

  const renameSessionAction = useCallback(async (id: string, title: string) => {
    try {
      await api.renameSession(id, title)
      await refreshSessions()
    } catch (e) { setError(String(e)) }
  }, [refreshSessions])

  const respondPermissionAction = useCallback(async (requestId: string, allow: boolean, note?: string) => {
    try {
      await api.respondPermission(requestId, allow, note)
      setPermissionRequest(null)
    } catch (e) { setError(String(e)) }
  }, [])

  // Register Tauri event listeners
  useEffect(() => {
    const unlisteners: UnlistenFn[] = []
    let cancelled = false

    async function register() {
      const handlers = [
        listen(EVENT_NAMES.QUERY_TEXT, (e) => {
          const p = e.payload as { content: string }
          setStreamingText(prev => prev + p.content)
        }),
        listen(EVENT_NAMES.QUERY_TOOL_START, (e) => {
          const p = e.payload as { tool_use_id: string; tool_name: string; tool_input: unknown }
          setActiveToolCalls(prev => [...prev, {
            tool_use_id: p.tool_use_id,
            tool_name: p.tool_name,
            tool_input: p.tool_input,
            status: 'running',
          }])
        }),
        listen(EVENT_NAMES.QUERY_TOOL_RESULT, (e) => {
          const p = e.payload as { tool_use_id: string; result: string; is_error: boolean }
          setActiveToolCalls(prev => prev.map(tc =>
            tc.tool_use_id === p.tool_use_id
              ? { ...tc, result: p.result, is_error: p.is_error, status: p.is_error ? 'error' : 'completed' }
              : tc
          ))
        }),
        listen(EVENT_NAMES.QUERY_TOOL_PROGRESS, (e) => {
          const p = e.payload as { tool_use_id: string; progress: number; message: string }
          setActiveToolCalls(prev => prev.map(tc =>
            tc.tool_use_id === p.tool_use_id
              ? { ...tc, progress: p.progress, progress_message: p.message }
              : tc
          ))
        }),
        listen(EVENT_NAMES.QUERY_THINKING, (e) => {
          const p = e.payload as { content: string }
          setThinkingText(prev => prev + p.content)
        }),
        listen(EVENT_NAMES.QUERY_USAGE, (e) => {
          setUsage(e.payload as UsagePayload)
        }),
        listen(EVENT_NAMES.QUERY_COMPLETED, () => {
          setIsQuerying(false)
          // Commit the streamed text as a finished assistant message. Read
          // via the ref (kept in sync on every render) instead of nesting
          // setMessages inside the setStreamingText updater.
          const finalText = streamingTextRef.current
          if (finalText) {
            setMessages(msgs => [...msgs, { role: 'assistant', content: finalText, timestamp: Date.now() }])
          }
          setStreamingText('')
          setThinkingText('')
          setCurrentQueryId(null)
          refreshStatus()
        }),
        listen(EVENT_NAMES.QUERY_FAILED, (e) => {
          const p = e.payload as { error: string }
          setError(p.error)
          setIsQuerying(false)
          setCurrentQueryId(null)
        }),
        listen(EVENT_NAMES.QUERY_CANCELLED, () => {
          setIsQuerying(false)
          setCurrentQueryId(null)
        }),
        listen(EVENT_NAMES.PERMISSION_REQUEST, (e) => {
          setPermissionRequest(e.payload as PermissionRequest)
        }),
        listen(EVENT_NAMES.SESSIONS_UPDATED, () => { refreshSessions() }),
        listen(EVENT_NAMES.CONFIG_UPDATED, () => { refreshConfig() }),
        listen(EVENT_NAMES.BACKGROUND_TASKS_UPDATED, () => { refreshBackgroundTasks() }),
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
    }
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  // Initial data load
  useEffect(() => {
    Promise.all([
      refreshStatus(),
      refreshConfig(),
      refreshSessions(),
      refreshModels(),
      refreshTasks(),
      refreshAgents(),
      refreshMcpServers(),
      refreshBackgroundTasks(),
      api.getConversation().then(setMessages).catch(e => console.warn('Failed to load conversation:', e)),
    ]).finally(() => setLoading(false))
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  const chatValue = useMemo<ChatContextValue>(() => ({
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage,
    sendMessage, cancelQuery,
  }), [messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage, sendMessage, cancelQuery])

  const sessionValue = useMemo<SessionContextValue>(() => ({
    sessions, currentSessionId, createSession, createSessionInWorktree, switchSession: switchToSession,
    deleteSession: deleteSessionAction, renameSession: renameSessionAction, refreshSessions,
  }), [sessions, currentSessionId, createSession, createSessionInWorktree, switchToSession,
    deleteSessionAction, renameSessionAction, refreshSessions])

  const catalogValue = useMemo<CatalogContextValue>(() => ({
    status, config, models, agents, tasks, mcpServers, backgroundTasks, permissionRequest,
    error, loading, refreshStatus, refreshConfig, refreshModels, refreshTasks, refreshAgents,
    refreshMcpServers, refreshBackgroundTasks, respondPermission: respondPermissionAction,
  }), [status, config, models, agents, tasks, mcpServers, backgroundTasks, permissionRequest,
    error, loading, refreshStatus, refreshConfig, refreshModels, refreshTasks, refreshAgents,
    refreshMcpServers, refreshBackgroundTasks, respondPermissionAction])

  return (
    <CatalogContext.Provider value={catalogValue}>
      <SessionContext.Provider value={sessionValue}>
        <ChatContext.Provider value={chatValue}>
          {children}
        </ChatContext.Provider>
      </SessionContext.Provider>
    </CatalogContext.Provider>
  )
}

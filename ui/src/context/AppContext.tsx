import { createContext, useContext, useState, useEffect, useCallback, type ReactNode } from 'react'
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

interface AppState {
  messages: ChatMessage[]
  streamingText: string
  thinkingText: string
  isQuerying: boolean
  activeToolCalls: ToolCall[]
  usage: UsagePayload | null
  sessions: SessionInfo[]
  currentSessionId: string | null
  status: StatusResponse | null
  config: DesktopConfig | null
  models: ModelInfo[]
  permissionRequest: PermissionRequest | null
  backgroundTasks: BackgroundTaskInfo[]
  tasks: TaskItem[]
  agents: AgentInfo[]
  mcpServers: McpServerInfo[]
  error: string | null
  loading: boolean
}

interface AppActions {
  sendMessage: (message: string, filePaths?: string[]) => Promise<void>
  cancelQuery: () => Promise<void>
  createSession: () => Promise<void>
  createSessionInWorktree: () => Promise<void>
  switchSession: (id: string) => Promise<void>
  deleteSession: (id: string) => Promise<void>
  renameSession: (id: string, title: string) => Promise<void>
  respondPermission: (requestId: string, allow: boolean, note?: string) => Promise<void>
  refreshSessions: () => Promise<void>
  refreshStatus: () => Promise<void>
  refreshConfig: () => Promise<void>
  refreshModels: () => Promise<void>
  refreshTasks: () => Promise<void>
  refreshAgents: () => Promise<void>
  refreshMcpServers: () => Promise<void>
  refreshBackgroundTasks: () => Promise<void>
}

const AppContext = createContext<(AppState & AppActions) | null>(null)

export function useApp(): AppState & AppActions {
  const ctx = useContext(AppContext)
  if (!ctx) throw new Error('useApp must be used within AppProvider')
  return ctx
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
          setStreamingText(prev => {
            if (prev) {
              setMessages(msgs => [...msgs, { role: 'assistant', content: prev, timestamp: Date.now() }])
            }
            return ''
          })
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

  const value: AppState & AppActions = {
    messages, streamingText, thinkingText, isQuerying, activeToolCalls, usage,
    sessions, currentSessionId, status, config, models, permissionRequest,
    backgroundTasks, tasks, agents, mcpServers, error, loading,
    sendMessage, cancelQuery, createSession, createSessionInWorktree, switchSession: switchToSession,
    deleteSession: deleteSessionAction, renameSession: renameSessionAction,
    respondPermission: respondPermissionAction, refreshSessions, refreshStatus,
    refreshConfig, refreshModels, refreshTasks, refreshAgents, refreshMcpServers,
    refreshBackgroundTasks,
  }

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>
}

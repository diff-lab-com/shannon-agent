// CatalogContext — low-frequency, broadly-read catalog slice of the former
// AppContext: status/config/models/agents/tasks/mcp/background/permissions,
// plus the shared `error`/`loading` flags (see AppContext doc). Provided by
// AppProvider, which owns the state and actions; this file only declares the
// slice type, the context, and the useCatalog hook.

import { createContext, useContext } from 'react'
import type {
  StatusResponse,
  DesktopConfig,
  ProviderStatus,
  ModelInfo,
  TaskItem,
  AgentInfo,
  McpServerInfo,
  BackgroundTaskInfo,
  PermissionRequest,
} from '@/types'

export interface CatalogContextValue {
  status: StatusResponse | null
  config: DesktopConfig | null
  /** Reliable provider-activation snapshot (`get_provider_status`) — the
   *  ADR-0005-safe replacement for the dead `config.provider`/`api_key`
   *  gates (2026-09-29 provider review §3-A1). */
  providerStatus: ProviderStatus | null
  models: ModelInfo[]
  agents: AgentInfo[]
  tasks: TaskItem[]
  mcpServers: McpServerInfo[]
  backgroundTasks: BackgroundTaskInfo[]
  permissionRequest: PermissionRequest | null
  error: string | null
  /** Failure class for `error`: `auth` (key rejected — dedicated
   *  "update key" banner) vs `other` (raw error line). null when no error
   *  or when the error came from a non-query path. */
  errorKind: 'auth' | 'other' | null
  loading: boolean
  /** Set when the initial data load fails for any surface; cleared by retryInit. */
  initError: string | null
  retryInit: () => Promise<void>
  refreshStatus: () => Promise<void>
  refreshConfig: () => Promise<void>
  refreshModels: () => Promise<void>
  refreshTasks: () => Promise<void>
  refreshAgents: () => Promise<void>
  refreshMcpServers: () => Promise<void>
  refreshBackgroundTasks: () => Promise<void>
  respondPermission: (requestId: string, allow: boolean, options?: { note?: string; scope?: 'once' | 'always_tool' }) => Promise<void>
}

export const CatalogContext = createContext<CatalogContextValue | null>(null)

export function useCatalog(): CatalogContextValue {
  const ctx = useContext(CatalogContext)
  if (!ctx) throw new Error('useCatalog must be used within AppProvider')
  return ctx
}

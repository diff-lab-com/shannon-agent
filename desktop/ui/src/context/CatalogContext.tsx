// CatalogContext — low-frequency, broadly-read catalog slice of the former
// AppContext: status/config/models/agents/tasks/mcp/background/permissions,
// plus the shared `error`/`loading` flags (see AppContext doc). Provided by
// AppProvider, which owns the state and actions; this file only declares the
// slice type, the context, and the useCatalog hook.

import { createContext, useContext } from 'react'
import type {
  StatusResponse,
  DesktopConfig,
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
  models: ModelInfo[]
  agents: AgentInfo[]
  tasks: TaskItem[]
  mcpServers: McpServerInfo[]
  backgroundTasks: BackgroundTaskInfo[]
  permissionRequest: PermissionRequest | null
  error: string | null
  loading: boolean
  refreshStatus: () => Promise<void>
  refreshConfig: () => Promise<void>
  refreshModels: () => Promise<void>
  refreshTasks: () => Promise<void>
  refreshAgents: () => Promise<void>
  refreshMcpServers: () => Promise<void>
  refreshBackgroundTasks: () => Promise<void>
  respondPermission: (requestId: string, allow: boolean, note?: string) => Promise<void>
}

export const CatalogContext = createContext<CatalogContextValue | null>(null)

export function useCatalog(): CatalogContextValue {
  const ctx = useContext(CatalogContext)
  if (!ctx) throw new Error('useCatalog must be used within AppProvider')
  return ctx
}

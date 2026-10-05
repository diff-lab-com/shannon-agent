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

// S1-1 (review 2026-10-05 §3 P-N1): machine-readable failure classes carried
// on the desktop `query:failed` payload (`error_kind`, classified Rust-side
// from the typed error — engine is the single source). Each kind routes to
// its own recovery banner in MessageArea:
//   auth → "update key" (401) · quota → "quota exhausted" (402) ·
//   rate_limit → "rate limited" (429) · authz → "access denied" (403) ·
//   other → plain error line + Retry.
export const CHAT_ERROR_KINDS = ['auth', 'quota', 'rate_limit', 'authz', 'other'] as const
export type ChatErrorKind = (typeof CHAT_ERROR_KINDS)[number]

/** Narrow an untrusted payload field to a known kind. Unknown/missing
 *  values fall back to `other` (plain error banner) — never a thrown
 *  match, so older payloads and future engine kinds stay renderable. */
export function normalizeChatErrorKind(kind: unknown): ChatErrorKind {
  return typeof kind === 'string' && (CHAT_ERROR_KINDS as readonly string[]).includes(kind)
    ? (kind as ChatErrorKind)
    : 'other'
}

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
  /** Failure class for `error` (S1-1): `auth` (401 — "update key" banner),
   *  `quota` (402 — "quota exhausted" + update key / view usage),
   *  `rate_limit` (429 — wait hint + Retry), `authz` (403 — access
   *  denied + Settings pointer), `other` (raw error line + Retry). null
   *  when no error or when the error came from a non-query path. */
  errorKind: ChatErrorKind | null
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

// Scripted-backend seed override — mutable layer consulted by the mock
// handlers when a ChatScript is loaded (`loadScript` → `setScriptSeed`).
//
// This module is a deliberate LEAF: handlers.ts imports it, and the player
// writes into it, so it must not import either (avoids an ESM cycle). When
// no seed is armed every accessor returns undefined and handlers behave
// byte-identically to the pre-scripted defaults.

import type { ChatMessage, SessionInfo } from '@/types'
import type { ScriptSeed } from './schema'
import { realmSingleton } from '../realmState'

// Realm-global (see realmState.ts): the seed must be shared across the mock
// layer's two module-graph instances, or one instance arms a script while
// the other answers the fetches.
interface SeedStateBox {
  seed: ScriptSeed | null
}

function seedState(): SeedStateBox {
  return realmSingleton<SeedStateBox>('__shannonMockSeed', () => ({ seed: null }))
}

/** Arm the override (loadScript). Pass null to clear (reset). */
export function setScriptSeed(seed: ScriptSeed | null): void {
  seedState().seed = seed
}

/** Raw override, or null when no script is loaded. */
export function getScriptSeed(): ScriptSeed | null {
  return seedState().seed
}

/** SessionInfo rows for `list_sessions` while a seed is armed. */
export function seededSessions(): SessionInfo[] | null {
  const seed = seedState().seed
  const sessions = seed?.sessions
  if (!sessions) return null
  const now = Date.now()
  return sessions.map((s, i) => ({
    id: s.id,
    title: s.title,
    created_at: now - (sessions.length - i) * 60_000,
    message_count: s.messages.length,
    updated_at: now - i * 60_000,
  }))
}

/**
 * ChatMessage rows for `get_conversation` while a seed is armed.
 * `sessionId` selects a seeded session; with no match (or no id — the
 * main window boots without one) the FIRST seeded session answers, which
 * is the "current conversation" in the scripted world.
 */
export function seededMessages(sessionId?: string | null): ChatMessage[] | null {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  const session = seed.sessions.find(s => s.id === sessionId) ?? seed.sessions[0]
  const base = Date.now() - session.messages.length * 60_000
  return session.messages.map((m, i) => ({
    role: m.role,
    content: m.content,
    timestamp: base + i * 60_000,
    ...(m.attachments?.length
      ? {
          file_attachments: m.attachments.map(path => ({
            path,
            name: path.split('/').pop() ?? path,
            size: 0,
          })),
        }
      : {}),
  }))
}

/** `budgetUsd` override for `get_session_budget`; undefined = not seeded.
 *  Applies when no session id is given (active-session fallback) or when the
 *  id matches a seeded session; unknown sessions fall back to demo state. */
export function seededBudget(sessionId?: string | null): number | null | undefined {
  const seed = seedState().seed
  if (!seed?.config || seed.config.budgetUsd == null) return undefined
  if (sessionId == null) return seed.config.budgetUsd
  return seed.sessions?.some(s => s.id === sessionId) ? seed.config.budgetUsd : undefined
}

/**
 * Zero ledger for a seeded session (`get_session_usage`): a scripted session
 * starts pristine — spending arrives via budget:* events, not history. Null
 * when the seed is unarmed or the id is not a seeded one (strict match,
 * unlike seededMessages' first-session fallback).
 */
export function seededUsage(sessionId?: string | null): {
  input_tokens: number
  output_tokens: number
  cache_creation_tokens: number
  cache_read_tokens: number
  cost_usd: number
  events: number
} | null {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  const session = sessionId == null
    ? seed.sessions[0]
    : seed.sessions.find(s => s.id === sessionId)
  if (!session) return null
  return {
    input_tokens: 0,
    output_tokens: 0,
    cache_creation_tokens: 0,
    cache_read_tokens: 0,
    cost_usd: 0,
    events: session.messages.length,
  }
}

/**
 * Config patch for `get_config`: `provider` swap and the hasKey=false shape
 * (`api_key: null`). Null while unarmed — handlers keep their defaults.
 */
export function seededConfigPatch(): { provider?: string; api_key?: string | null } | null {
  const config = seedState().seed?.config
  if (!config) return null
  const patch: { provider?: string; api_key?: string | null } = {}
  if (config.provider != null) patch.provider = config.provider
  if (config.hasKey === false) patch.api_key = null
  return Object.keys(patch).length ? patch : null
}

/**
 * get_provider_status patch while armed. `hasKey: false` keeps the active
 * provider but flips `has_api_key` (banner variant "no-key"); `provider`
 * retargets the active provider id (no roster row → null display/kind,
 * mirroring the real command's unknown-provider shape).
 */
export function seededProviderStatusPatch(): {
  active_provider_id?: string | null
  display_name?: string | null
  kind?: string | null
  has_api_key?: boolean
} | null {
  const config = seedState().seed?.config
  if (!config) return null
  const patch: {
    active_provider_id?: string | null
    display_name?: string | null
    kind?: string | null
    has_api_key?: boolean
  } = {}
  if (config.provider != null) {
    patch.active_provider_id = config.provider
    patch.display_name = null
    patch.kind = null
  }
  if (config.hasKey != null) patch.has_api_key = config.hasKey
  return Object.keys(patch).length ? patch : null
}

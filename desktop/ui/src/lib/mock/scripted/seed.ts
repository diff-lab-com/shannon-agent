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
 *
 * R3: seeded `toolCalls` map onto the wire's snake_case `tool_calls` with
 * the derived `status` the UI's card renderer gates on (completed/error),
 * so preloaded history renders tool cards / FileChangesCard / FileCard.
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
    ...(m.toolCalls?.length
      ? {
          tool_calls: m.toolCalls.map(tc => ({
            tool_use_id: tc.toolUseId,
            tool_name: tc.toolName,
            tool_input: tc.toolInput,
            ...(tc.result != null ? { result: tc.result } : {}),
            ...(tc.isError != null ? { is_error: tc.isError } : {}),
            ...(tc.meta != null ? { meta: tc.meta } : {}),
            status: tc.isError ? ('error' as const) : ('completed' as const),
          })),
        }
      : {}),
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

// ── S-4 fix (R4 group 3): sent-message durability ──────────────────────────
//
// The real backend records a turn's user message into the session's L0 log
// BEFORE the model sees anything (`agent_loop.rs` — record_user_message + 
// record_turn_start precede the first request), so `switch_session`/`load_session`
// reload from the log always include it — even mid-run or after a cancel.
// The scripted backend never modeled this: a send left no trace, so
// switching away and back dropped the just-sent bubble (cancel-matrix
// scenario 9's "bubble does not survive"). This overlay closes that
// fidelity gap: each scripted/fall-through send appends the user message to
// its session's runtime tail, and the seeded conversation readers project
// seed + recorded tail.

interface RecordedSendsBox {
  bySession: Map<string, ChatMessage[]>
}

function recordedSends(): RecordedSendsBox {
  return realmSingleton<RecordedSendsBox>('__shannonMockRecordedSends', () => ({
    bySession: new Map(),
  }))
}

/** Resolve the seeded session a send with this explicit id targets.
 *  `null` keeps the scripted "current conversation" = first seeded session
 *  (the same fallback seededMessages applies to get_conversation). An id
 *  that matches no seeded session records nothing — the demo rail is
 *  seeded-only while a script is armed, so this cannot diverge in practice. */
function targetSession(sessionId: string | null | undefined) {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  return sessionId == null ? seed.sessions[0] : seed.sessions.find(s => s.id === sessionId)
}

/** Record one accepted send's user message into its session's tail. A
 *  REJECTED send must NOT reach this (the UI rolls its bubble back; the
 *  real backend's guards reject before recording either). */
export function recordSeedUserSend(sessionId: string | null | undefined, message: string | null): void {
  if (message == null || message === '') return
  const session = targetSession(sessionId)
  if (!session) return
  const box = recordedSends()
  const tail = box.bySession.get(session.id) ?? []
  tail.push({ role: 'user', content: message, timestamp: Date.now() })
  box.bySession.set(session.id, tail)
}

/** Clear the recorded tails (player load/reset — a fresh script lifecycle). */
export function resetRecordedSends(): void {
  recordedSends().bySession.clear()
}

/** Drop ONE session's recorded tail — /rewind rewrites the session history
 *  (the mock truncates to the checkpoint boundary), so the pre-rewind tail
 *  must not resurrect on the next reload. */
export function clearRecordedSends(sessionId: string | null | undefined): void {
  const session = targetSession(sessionId ?? null)
  if (session) recordedSends().bySession.delete(session.id)
}

/**
 * seededMessages + the session's recorded send tail — the durability-aware
 * read behind get_conversation / load_session / switch_session. Null when
 * no seed is armed (callers fall back to demo state, byte-identical).
 */
export function seededMessagesWithRecorded(sessionId?: string | null): ChatMessage[] | null {
  const base = seededMessages(sessionId)
  if (base == null) return null
  const session = targetSession(sessionId ?? null)
  const tail = session ? recordedSends().bySession.get(session.id) : undefined
  return tail?.length ? [...base, ...tail] : base
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
 * Usage ledger for a seeded session (`get_session_usage`). R3: when the
 * seed carries `config.spentUsd` the ledger reports it as `cost_usd` —
 * the "already over budget" shape the budget banners re-derive on
 * mount/switch (useBudgetGuard B4 P2-8). Otherwise a scripted session
 * starts pristine (spend arrives via budget:* events, not history).
 * Null when the seed is unarmed or the id is not a seeded one (strict
 * match, unlike seededMessages' first-session fallback).
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
    cost_usd: seed.config?.spentUsd ?? 0,
    events: session.messages.length,
  }
}

/**
 * R3 (journey #10): checkpoints for a seeded session (`list_checkpoints`).
 * The demo mock has none (record_turn runs in the desktop Rust process),
 * which hid the edit/rewind affordances from every scripted journey. While
 * a seed is armed we derive one checkpoint per user turn — the pre-turn
 * snapshot semantics the edit-commit flow assumes (rewind to
 * `turnIndex`, then resend). Null when unarmed or the id is not seeded.
 */
export function seededCheckpoints(sessionId?: string | null): Array<{
  turn_index: number
  timestamp: number
  description: string
  files_changed: string[]
  prompt_preview: string | null
}> | null {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  const session = sessionId == null
    ? seed.sessions[0]
    : seed.sessions.find(s => s.id === sessionId)
  if (!session) return null
  const base = Date.now() - session.messages.length * 60_000
  const checkpoints: Array<{
    turn_index: number
    timestamp: number
    description: string
    files_changed: string[]
    prompt_preview: string | null
  }> = []
  let turnIndex = 0
  for (const m of session.messages) {
    if (m.role !== 'user') continue
    checkpoints.push({
      turn_index: turnIndex,
      timestamp: base + turnIndex * 60_000,
      description: `Turn ${turnIndex + 1}`,
      files_changed: [],
      prompt_preview: m.content,
    })
    turnIndex += 1
  }
  return checkpoints
}

/**
 * R3 (journey #10): the post-rewind conversation (`rewind_session`) —
 * every seeded message BEFORE the `turnIndex`-th user turn, i.e. the
 * conversation truncated to the checkpoint boundary. Null when unarmed or
 * the id is not seeded (unknown sessions keep the demo handler).
 */
export function seededRewoundMessages(sessionId: string | null | undefined, turnIndex: number): ChatMessage[] | null {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  const session = sessionId == null
    ? seed.sessions[0]
    : seed.sessions.find(s => s.id === sessionId)
  if (!session) return null
  const all = seededMessages(sessionId) ?? []
  const out: ChatMessage[] = []
  let turn = 0
  for (let i = 0; i < session.messages.length; i++) {
    const m = session.messages[i]!
    if (m.role === 'user' && turn >= turnIndex) break
    if (m.role === 'user') turn += 1
    const wire = all[i]
    if (wire) out.push(wire)
  }
  return out
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

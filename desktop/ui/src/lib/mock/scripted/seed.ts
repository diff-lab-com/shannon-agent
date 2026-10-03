// Scripted-backend seed override — mutable layer consulted by the mock
// handlers when a ChatScript is loaded (`loadScript` → `setScriptSeed`).
//
// This module is a deliberate LEAF: handlers.ts imports it, and the player
// writes into it, so it must not import either (avoids an ESM cycle). When
// no seed is armed every accessor returns undefined and handlers behave
// byte-identically to the pre-scripted defaults.

import type { ArchivedSessionRow, ChatMessage, SessionInfo } from '@/types'
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
  // W2 journeys: the session-runtime registry below is derived from the
  // seed, so it follows the exact same lifecycle (load re-arms, reset clears).
  resetSeedSessionRuntime()
}

// ── W2 journeys: seeded session-lifecycle + model-override registry ────────
//
// Mutable state for SEEDED sessions — renames / deletes / archives issued
// through the sidebar while a script is armed, plus the session-model
// override the composer chip reads and writes (backend
// `SessionState.model_override` twin). Realm-global for the same
// two-module-graph reason as the seed itself; handlers consult it ONLY
// while a seed is armed, so the un-scripted demo path stays untouched.

interface SeedSessionRuntime {
  deleted: Set<string>
  archived: Set<string>
  renamed: Map<string, string>
  modelOverrides: Map<string, { provider: string; model: string }>
}

function seedSessionRuntime(): SeedSessionRuntime {
  return realmSingleton<SeedSessionRuntime>('__shannonMockSeedSessionRuntime', () => ({
    deleted: new Set(),
    archived: new Set(),
    renamed: new Map(),
    modelOverrides: new Map(),
  }))
}

/** Clear + re-arm the registry (script load/reset — a fresh lifecycle).
 *  Also clears the save_text_file store: a new script is a fresh world. */
export function resetSeedSessionRuntime(): void {
  const rt = seedSessionRuntime()
  rt.deleted.clear()
  rt.archived.clear()
  rt.renamed.clear()
  rt.modelOverrides.clear()
  savedFiles().files.clear()
  const seed = seedState().seed
  // Re-arm the seeded overrides: the chip reads them via get_session_model
  // on mount, before any UI action could have written the registry.
  for (const s of seed?.sessions ?? []) {
    if (s.modelOverride) rt.modelOverrides.set(s.id, { ...s.modelOverride })
  }
}

function findSeedSession(sessionId: string | null | undefined) {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  return seed.sessions.find(s => s.id === sessionId) ?? null
}

/** Record a seeded session's rename (rename_session while armed). */
export function recordSeedSessionRenamed(sessionId: string, title: string): void {
  if (!findSeedSession(sessionId)) return
  seedSessionRuntime().renamed.set(sessionId, title)
}

/** Record a seeded session's delete (delete_session while armed). A delete
 *  also leaves the archive registry — the archived lens' 永久删除 removes
 *  the whole session, exactly like the backend's L0 directory wipe. */
export function recordSeedSessionDeleted(sessionId: string): void {
  if (!findSeedSession(sessionId)) return
  const rt = seedSessionRuntime()
  rt.deleted.add(sessionId)
  rt.archived.delete(sessionId)
}

/** Record a seeded session's archive / restore (archive/unarchive_session
 *  while armed). Archived rows leave list_sessions and surface through
 *  list_archived_sessions until restored. */
export function recordSeedSessionArchived(sessionId: string): void {
  if (!findSeedSession(sessionId)) return
  seedSessionRuntime().archived.add(sessionId)
}

export function recordSeedSessionUnarchived(sessionId: string): void {
  if (!findSeedSession(sessionId)) return
  seedSessionRuntime().archived.delete(sessionId)
}

/** True when the seeded session is marked deleteFails (the deterministic
 *  refused-delete fixture behind DeleteSessionModal's failure branch). */
export function seedSessionDeleteFails(sessionId: string): boolean {
  return findSeedSession(sessionId)?.deleteFails === true
}

/**
 * The session-scoped model override for `get_session_model` and the
 * player's send-time `model` stamp. `null` sessionId resolves to the FIRST
 * seeded session (the scripted "current conversation" rule, same as
 * seededMessages); an id matching no seeded session resolves to null — the
 * demo map answers instead. Chip switches update the registry via
 * setSeedSessionModel/clearSeedSessionModel, so the NEXT send observes the
 * switch (the "takes effect next turn" anchor).
 */
export function seededSessionModel(sessionId?: string | null): { provider: string; model: string } | null {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return null
  const id = sessionId == null ? seed.sessions[0]!.id : sessionId
  return seedSessionRuntime().modelOverrides.get(id) ?? null
}

/** Chip switch (set_session_model while armed): update the registry. */
export function setSeedSessionModel(sessionId: string | null | undefined, override: { provider: string; model: string }): void {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return
  const id = sessionId == null ? seed.sessions[0]!.id : sessionId
  if (!seed.sessions.some(s => s.id === id)) return
  seedSessionRuntime().modelOverrides.set(id, { ...override })
}

/** Chip reset (clear_session_model while armed): back to inheriting the
 *  global default. */
export function clearSeedSessionModel(sessionId: string | null | undefined): void {
  const seed = seedState().seed
  if (!seed?.sessions?.length) return
  const id = sessionId == null ? seed.sessions[0]!.id : sessionId
  seedSessionRuntime().modelOverrides.delete(id)
}

// ── W2 journey #20: save_text_file store (plan write-back + failure) ───────

interface SavedFilesBox {
  files: Map<string, string>
}

function savedFiles(): SavedFilesBox {
  return realmSingleton<SavedFilesBox>('__shannonMockSavedFiles', () => ({ files: new Map() }))
}

/**
 * `save_text_file` while armed: record the write (in-memory demo twin of
 * the disk write). `get_session_plan` serves the written content back so a
 * PlanPanel checkbox tick survives its own refresh. With
 * `config.saveTextFileFails` the handler rejects BEFORE recording — the
 * rollback-to-engine-truth half of the journey.
 */
export function seedSaveTextFileShouldFail(): boolean {
  return seedState().seed?.config?.saveTextFileFails === true
}

export function recordSavedTextFile(path: string, content: string): void {
  savedFiles().files.set(path, content)
}

/** The written plan content under `<workingDir>/.shannon/plans/*.md`, or
 *  null when nothing was written there (or no script armed — the store is
 *  global but only written through the armed save handler). */
export function savedPlanForWorkingDir(workingDir: string): { id: string; title: string; status: string; created_at: string; content: string } | null {
  const prefix = `${workingDir}/.shannon/plans/`
  for (const [path, content] of savedFiles().files) {
    if (!path.startsWith(prefix)) continue
    // The exact header layout PlanPanel writes (PlanManager::save_plan_to_file
    // format): `# Plan: <title>\nCreated: <created>\nStatus: <status>\n\n<body>`.
    const title = /^# Plan: (.*)$/m.exec(content)?.[1] ?? 'Plan'
    const created = /^Created: (.*)$/m.exec(content)?.[1] ?? new Date().toISOString()
    const status = /^Status: (.*)$/m.exec(content)?.[1] ?? 'pending'
    const body = content.replace(/^# Plan: .*\nCreated: .*\nStatus: .*\n\n/, '')
    return {
      id: path.slice(prefix.length).replace(/\.md$/, ''),
      title,
      status,
      created_at: created,
      content: body,
    }
  }
  return null
}

/** Raw override, or null when no script is loaded. */
export function getScriptSeed(): ScriptSeed | null {
  return seedState().seed
}

/** SessionInfo rows for `list_sessions` while a seed is armed. Renames,
 *  deletes and archives issued through the sidebar while armed are
 *  projected here (the scripted twin of the backend session registry). */
export function seededSessions(): SessionInfo[] | null {
  const seed = seedState().seed
  const sessions = seed?.sessions
  if (!sessions) return null
  const rt = seedSessionRuntime()
  const now = Date.now()
  return sessions
    .filter(s => !rt.deleted.has(s.id) && !rt.archived.has(s.id))
    .map((s, i) => ({
      id: s.id,
      title: rt.renamed.get(s.id) ?? s.title,
      created_at: now - (sessions.length - i) * 60_000,
      message_count: s.messages.length,
      updated_at: now - i * 60_000,
      ...(s.workingDir ? { working_dir: s.workingDir } : {}),
    }))
}

/**
 * ArchivedSessionRow list for `list_archived_sessions` while a seed is
 * armed (the session-lifecycle journey's 已归档 section). Null when
 * unarmed — the demo handler keeps its (empty) roster.
 */
export function seededArchivedSessions(): ArchivedSessionRow[] | null {
  const seed = seedState().seed
  if (!seed?.sessions) return null
  const rt = seedSessionRuntime()
  const now = Date.now()
  return seed.sessions
    .filter(s => rt.archived.has(s.id))
    .map(s => ({
      id: s.id,
      title: rt.renamed.get(s.id) ?? s.title,
      updated_at: now,
    }))
}

/**
 * Title search over the VISIBLE seeded sessions for `search_sessions` while
 * a seed is armed (the backend's title-first contract). Null when unarmed —
 * the demo handler keeps its roster.
 */
export function seededSearchSessions(query: string): SessionInfo[] | null {
  if (!seedState().seed?.sessions) return null
  const q = query.trim().toLowerCase()
  const visible = seededSessions() ?? []
  if (!q) return visible
  return visible.filter(s => s.title.toLowerCase().includes(q))
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

/**
 * D6 (keep the partial output): record a CANCELLED turn's partial assistant
 * text into its session's tail, flagged `interrupted` — the scripted
 * counterpart of the real backend's interrupted-turn finalize (the engine
 * tee writes `assistant/message(interrupted: true)` on the cancel path, so a
 * log-backed reload brings the marked partial bubble back). OBS1: a FAILED
 * turn's partial is recorded the same way with `interrupted_reason:
 * 'failed'` (the real backend's failed close keeps the prefix identically).
 * An empty partial (stop/fail before the first token) records nothing,
 * matching the no-bubble commit. Completed replies stay unrecorded (the S-4
 * tail models accepted sends; assistant completion persistence remains the
 * pre-existing gap).
 */
export function recordSeedPartialAssistant(
  sessionId: string | null | undefined,
  text: string | null,
  reason?: 'cancelled' | 'failed',
): void {
  if (text == null || text === '') return
  const session = targetSession(sessionId)
  if (!session) return
  const box = recordedSends()
  const tail = box.bySession.get(session.id) ?? []
  tail.push({
    role: 'assistant',
    content: text,
    timestamp: Date.now(),
    interrupted: true,
    ...(reason != null ? { interrupted_reason: reason } : {}),
  })
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
  // D6: recorded tail user sends are turns too — each gets its checkpoint
  // exactly like the desktop's record_turn (the cancel path records it as
  // well), so a cancelled turn's partial bubble carries the same
  // rewind/regenerate affordances a completed turn has.
  const tail = recordedSends().bySession.get(session.id) ?? []
  const tailUserSends = tail.filter(m => m.role === 'user')
  const base = Date.now() - (session.messages.length + tailUserSends.length) * 60_000
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
  for (const m of tailUserSends) {
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
  // D6: the rewind boundary may name a RECORDED tail turn (a scripted turn
  // after the seeded history), so the truncation walks seed + tail — the
  // same conversation the reload readers project.
  const all = seededMessagesWithRecorded(sessionId) ?? []
  const out: ChatMessage[] = []
  let turn = 0
  for (let i = 0; i < all.length; i++) {
    const m = all[i]!
    if (m.role === 'user' && turn >= turnIndex) break
    if (m.role === 'user') turn += 1
    out.push(m)
  }
  return out
}

/**
 * Config patch for `get_config`: `provider` swap, the hasKey=false shape
 * (`api_key: null`) and, W2 journey #17, the boot `approval_mode` (any
 * engine value — the composer pill echoes unknown ones verbatim). Null
 * while unarmed — handlers keep their defaults.
 */
export function seededConfigPatch(): { provider?: string; api_key?: string | null; approval_mode?: string } | null {
  const config = seedState().seed?.config
  if (!config) return null
  const patch: { provider?: string; api_key?: string | null; approval_mode?: string } = {}
  if (config.provider != null) patch.provider = config.provider
  if (config.hasKey === false) patch.api_key = null
  if (config.approvalMode != null) patch.approval_mode = config.approvalMode
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

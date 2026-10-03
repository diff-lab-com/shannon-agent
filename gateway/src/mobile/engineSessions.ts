/**
 * §J session data plane (cross-repo adaptation spec) — the gateway's local
 * interface to the engine WS `sessions.list` / `session.history` messages plus
 * the mapping onto the mobile wire (`shannon/session.list` / `.history`).
 *
 * IMPORTANT (integration note): `gateway/src/engine/types.gen.ts` is
 * code-generated from the Rust protocol and is being regenerated in parallel
 * (MF-5 adds `SessionsSnapshot` / `SessionTranscript` there). To avoid touching
 * the generated file, the engine-frame shapes live HERE as hand-written local
 * interfaces — this module is the single place to reconcile when the generated
 * types land. Response matchers tolerate the two plausible serde tag spellings
 * (dotted `sessions.snapshot` / underscore `sessions_snapshot`) so the wire
 * wiring survives either rename; field reads accept snake_case (the engine WS
 * convention) with a camelCase fallback.
 *
 * Semantics per spec §J:
 *  - `session.list` maps to `{sessions: [{id, agentId?, title?, updatedAt?,
 *    totalInputTokens?, totalOutputTokens?}]}` (token totals additive, C8:
 *    present only when the engine supplies them).
 *  - `session.history` paginates with `before` (ISO-8601 anchor = the first
 *    occurrence of that ts; same-ts groups are not split) + `limit`, and
 *    answers `hasMore`. An unknown sessionId is NOT an error — the engine (or
 *    an error frame) degrades to `{sessionId, messages: [], hasMore: false}`.
 */

// ── engine-frame shapes (local; reconcile with types.gen.ts at integration) ─

/** One session in the engine's `SessionsSnapshot`. */
export interface EngineSessionSummary {
  session_id: string;
  /** Not exposed by the engine yet — mapped through when it lands. */
  agent_id?: string | null;
  title?: string | null;
  updated_at?: string | number | null;
  /** C8 r2-w2: lifetime token totals (engine protocol additive fields). */
  total_input_tokens?: number | null;
  total_output_tokens?: number | null;
}

/** Engine response to `sessions.list`. */
export interface EngineSessionsSnapshot {
  sessions: EngineSessionSummary[];
}

/** One transcript entry in the engine's `SessionTranscript`. */
export interface EngineTranscriptMessage {
  role: string;
  content: string;
  ts?: string | number | null;
}

/** Engine response to `session.history`. */
export interface EngineSessionTranscript {
  session_id: string;
  messages: EngineTranscriptMessage[];
  has_more?: boolean;
}

/** Engine wire frames (serde `tag = "type"`, like `query` / `cancel`). */
export const ENGINE_SESSIONS_LIST_TYPE = "sessions.list";
export const ENGINE_SESSION_HISTORY_TYPE = "session.history";

export interface EngineSessionHistoryRequestFrame {
  type: typeof ENGINE_SESSION_HISTORY_TYPE;
  session_id: string;
  /** ISO-8601 anchor: first occurrence of this ts; returns strictly older. */
  before?: string;
  /** Page size (engine default 50; <1 clamps to 1). */
  limit?: number;
}

export const ENGINE_SESSIONS_LIST_REQUEST: { type: typeof ENGINE_SESSIONS_LIST_TYPE } = {
  type: ENGINE_SESSIONS_LIST_TYPE,
};

/**
 * The one-shot RPC surface `EngineWsClient#call` exposes (bound). Declared here
 * so the bridge can consume it without importing the concrete socket class.
 */
export interface EngineSessionCaller {
  connect(): Promise<void>;
  call<T>(
    message: unknown,
    match: (frame: unknown) => T | null,
    opts?: { timeoutMs?: number },
  ): Promise<T>;
  close(): Promise<void>;
}

/** A bound `EngineWsClient#call` — the only piece the fetch helpers need. */
export type EngineCall = <T>(
  message: unknown,
  match: (frame: unknown) => T | null,
  opts?: { timeoutMs?: number },
) => Promise<T>;

// ── frame matching ───────────────────────────────────────────────────────────

/** Both serde spellings the engine side might land on for the snapshot. */
const SESSIONS_SNAPSHOT_FRAME_TYPES = new Set(["sessions.snapshot", "sessions_snapshot"]);
/** Same tolerance for the transcript response. */
const SESSION_TRANSCRIPT_FRAME_TYPES = new Set(["session.transcript", "session_transcript"]);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === "object" && v !== null;
}

function asString(v: unknown): string | null {
  return typeof v === "string" && v.length > 0 ? v : null;
}

/** The engine's protocol-error frame (`{type: "error", message}`). */
export function engineErrorMessage(frame: unknown): string | null {
  if (!isObject(frame) || frame.type !== "error") return null;
  return asString(frame.message);
}

export function matchSessionsSnapshot(frame: unknown): EngineSessionsSnapshot | null {
  if (!isObject(frame) || !SESSIONS_SNAPSHOT_FRAME_TYPES.has(String(frame.type))) return null;
  if (!Array.isArray(frame.sessions)) return null;
  const sessions: EngineSessionSummary[] = [];
  for (const entry of frame.sessions) {
    if (isObject(entry)) sessions.push(entry as unknown as EngineSessionSummary);
  }
  return { sessions };
}

export function matchSessionTranscript(frame: unknown): EngineSessionTranscript | null {
  if (!isObject(frame) || !SESSION_TRANSCRIPT_FRAME_TYPES.has(String(frame.type))) return null;
  if (!Array.isArray(frame.messages)) return null;
  const messages: EngineTranscriptMessage[] = [];
  for (const entry of frame.messages) {
    if (isObject(entry)) messages.push(entry as unknown as EngineTranscriptMessage);
  }
  return {
    session_id: asString(frame.session_id) ?? "",
    messages,
    has_more: frame.has_more === true,
  };
}

// ── engine calls ─────────────────────────────────────────────────────────────

/**
 * Fetch the session roster. Rejects on transport failure or an engine error
 * frame (the caller maps that to ENGINE_ERROR — a dead engine must be
 * distinguishable from an empty roster).
 */
export async function fetchEngineSessions(call: EngineCall): Promise<EngineSessionsSnapshot> {
  const boxed = await call(ENGINE_SESSIONS_LIST_REQUEST, (frame: unknown) => {
    const snapshot = matchSessionsSnapshot(frame);
    if (snapshot) return { ok: true as const, value: snapshot };
    const err = engineErrorMessage(frame);
    if (err !== null) return { ok: false as const, error: err };
    return null;
  });
  if (!boxed.ok) throw new Error(`engine rejected sessions.list: ${boxed.error}`);
  return boxed.value;
}

/**
 * Fetch one session's transcript page. Per §J2 an unknown sessionId (or an
 * engine error frame) degrades to an EMPTY transcript — never an error — while
 * transport failures still reject (the gateway maps those to ENGINE_ERROR).
 */
export async function fetchEngineSessionHistory(
  call: EngineCall,
  sessionId: string,
  opts: { before?: string; limit?: number } = {},
): Promise<EngineSessionTranscript> {
  const frame: EngineSessionHistoryRequestFrame = {
    type: ENGINE_SESSION_HISTORY_TYPE,
    session_id: sessionId,
  };
  if (opts.before !== undefined) frame.before = opts.before;
  if (opts.limit !== undefined) frame.limit = opts.limit;
  const boxed = await call(frame, (raw: unknown) => {
    const transcript = matchSessionTranscript(raw);
    if (transcript) return { ok: true as const, value: transcript };
    if (engineErrorMessage(raw) !== null) {
      // Unknown session (or a history-side refusal) → honest empty, per §J2.
      return {
        ok: true as const,
        value: { session_id: sessionId, messages: [], has_more: false },
      };
    }
    return null;
  });
  return boxed.value;
}

// ── mobile-wire mapping (the §J shapes ARE the phone contract) ──────────────

export interface MobileSessionSummaryWire {
  id: string;
  agentId?: string;
  title?: string;
  updatedAt?: string;
  /** Lifetime token totals — present only when the engine supplied a number. */
  totalInputTokens?: number;
  totalOutputTokens?: number;
}

/**
 * Map one engine summary to the §J1 wire entry. Entries without a usable id
 * are skipped (the phone would skip them anyway); `agentId`/`title`/
 * `updatedAt`/token totals are omitted when the engine omits them — never
 * invented (the phone renders the spend row only "有数据才渲染").
 */
export function mapSessionSummary(summary: EngineSessionSummary): MobileSessionSummaryWire | null {
  const id = asString(summary.session_id ?? (summary as { id?: unknown }).id);
  if (!id) return null;
  const wire: MobileSessionSummaryWire = { id };
  const agentId = asString(summary.agent_id ?? (summary as { agentId?: unknown }).agentId);
  if (agentId) wire.agentId = agentId;
  const title = asString(summary.title);
  if (title) wire.title = title;
  const updatedAt = toIsoTimestamp(summary.updated_at ?? (summary as { updatedAt?: unknown }).updatedAt);
  if (updatedAt) wire.updatedAt = updatedAt;
  const totalInputTokens = asTokenCount(
    summary.total_input_tokens ?? (summary as { totalInputTokens?: unknown }).totalInputTokens,
  );
  if (totalInputTokens !== null) wire.totalInputTokens = totalInputTokens;
  const totalOutputTokens = asTokenCount(
    summary.total_output_tokens ?? (summary as { totalOutputTokens?: unknown }).totalOutputTokens,
  );
  if (totalOutputTokens !== null) wire.totalOutputTokens = totalOutputTokens;
  return wire;
}

export interface MobileTranscriptWire {
  sessionId: string;
  messages: { role: string; content: string; ts?: string }[];
  hasMore: boolean;
}

/**
 * Map the engine transcript to the §J2 wire shape. Messages keep the engine's
 * role verbatim (the phone only distinguishes user vs. non-user); entries
 * without string content are dropped rather than rendered blank. `ts` is
 * normalized to ISO-8601 when the engine supplies epoch ms.
 */
export function mapSessionTranscript(
  transcript: EngineSessionTranscript,
  requestedSessionId: string,
): MobileTranscriptWire {
  const messages: MobileTranscriptWire["messages"] = [];
  for (const m of transcript.messages ?? []) {
    const content = typeof m.content === "string" ? m.content : null;
    if (content === null) continue;
    const message: { role: string; content: string; ts?: string } = {
      role: typeof m.role === "string" && m.role.length > 0 ? m.role : "assistant",
      content,
    };
    const ts = toIsoTimestamp(m.ts);
    if (ts) message.ts = ts;
    messages.push(message);
  }
  return {
    sessionId: asString(transcript.session_id) || requestedSessionId,
    messages,
    hasMore: transcript.has_more === true,
  };
}

/** ISO-8601 UTC from an ISO string (verbatim) or epoch-ms number. */
function toIsoTimestamp(v: unknown): string | null {
  if (typeof v === "string" && v.length > 0) return v;
  if (typeof v === "number" && Number.isFinite(v)) return new Date(v).toISOString();
  return null;
}

/** A usable token count: a finite, non-negative number. Anything else → null. */
function asTokenCount(v: unknown): number | null {
  if (typeof v !== "number" || !Number.isFinite(v) || v < 0) return null;
  return v;
}

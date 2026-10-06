/**
 * shannon/* JSON-RPC 2.0 wire protocol — the mobile↔gateway contract.
 *
 * NDJSON over WebSocket: one JSON object per line. The phone is a first-class
 * streaming client (like the desktop UI), not a throttled chat channel, so the
 * surface is the full engine event stream — this protocol is the engine WS
 * protocol's "device-friendly + E2E + rich-UI" superset (architecture doc §5.2).
 *
 * Frames:
 *  - Request       (phone→gateway):   `{ jsonrpc, id, method, params? }`
 *  - Response      (gateway→phone):   `{ jsonrpc, id, result } | { jsonrpc, id, error }`
 *  - Notification  (gateway→phone, streaming): `{ jsonrpc, method:"shannon/event", params: ShannonEvent }`
 *
 * Method names use a slashed namespace (`shannon/query`, `shannon/approval/decide`,
 * `shannon/device.resume` …) to match `shannon-mobile`'s `methods.dart`. Field
 * names are snake_case on the wire, consistent with the engine WS types.
 *
 * Source of truth: `shannon-desktop/claudedocs/mobile-host-architecture.md` §5.2.
 */

export const JSONRPC_VERSION = "2.0" as const;

/**
 * Every shannon/* method the gateway recognizes. Phase-1 minimal set (R5) plus
 * the P2-1 task-dispatch pair: `shannon/task.dispatch` routes a text through
 * the same inbound pipeline the IM adapters use (trigger-free: the dispatch
 * action itself is the trigger), and `shannon/task.list` returns the recent
 * tasks the gateway dispatched for this device (minimal read-only surface).
 */
export type ShannonMethod =
  | "shannon/pair"
  | "shannon/device.resume"
  | "shannon/query"
  | "shannon/cancel"
  | "shannon/approval/decide"
  | "shannon/agent.list"
  | "shannon/agent.detail"
  | "shannon/model.list"
  | "shannon/model.switch"
  | "shannon/health"
  | "shannon/task.dispatch"
  | "shannon/task.list"
  | "shannon/snapshot"
  | "shannon/resume"
  | "shannon/device.list"
  | "shannon/device.revoke"
  | "shannon/approval.list"
  | "shannon/approval.state"
  | "shannon/approval.set"
  | "shannon/session.list"
  | "shannon/session.history"
  | "shannon/push.register";

/**
 * Runtime mirror of [ShannonMethod] — the SINGLE SOURCE both the type above
 * (members must match exactly) and the protocol schema
 * (`docs/protocol/shannon-mobile-protocol.schema.json`, pinned by
 * `__tests__/protocolSchema.test.ts`) derive from. Adding a method: add it
 * here, to the type, and to the schema enum — the tests force all three.
 */
export const SHANNON_METHODS = [
  "shannon/pair",
  "shannon/device.resume",
  "shannon/query",
  "shannon/cancel",
  "shannon/approval/decide",
  "shannon/agent.list",
  "shannon/agent.detail",
  "shannon/model.list",
  "shannon/model.switch",
  "shannon/health",
  "shannon/task.dispatch",
  "shannon/task.list",
  "shannon/snapshot",
  "shannon/resume",
  "shannon/device.list",
  "shannon/device.revoke",
  "shannon/approval.list",
  "shannon/approval.state",
  "shannon/approval.set",
  "shannon/session.list",
  "shannon/session.history",
  "shannon/push.register",
] as const satisfies readonly ShannonMethod[];

// Compile-time guard: every member of the union is present in the runtime
// list (the `satisfies` above covers the reverse direction).
type _UncoveredMethod = Exclude<ShannonMethod, (typeof SHANNON_METHODS)[number]>;
type _AllCovered = _UncoveredMethod extends never ? true : never;
const _allCovered: _AllCovered = true;
void _allCovered;

// ── Requests (phone → gateway) ───────────────────────────────────────────

export interface JsonRpcRequest<P = unknown> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: string | number;
  method: ShannonMethod;
  params?: P;
}

export interface QueryParams {
  prompt: string;
  model?: string | null;
  session_id?: string | null;
}

export interface CancelParams {
  session_id?: string | null;
}

export interface ApprovalDecideParams {
  request_id: string;
  /** `"allow" | "deny"` (maps to the engine `approval/respond` choice). */
  choice: "allow" | "deny";
  /**
   * P3-3: with `choice: "allow"` — how far the grant reaches. `"once"`
   * (default) maps to the engine `allow_once`; `"session"` maps to
   * `always_allow_session` (in-session always-allow, never persisted). A
   * `session` scope is bound into the decision signature; `deny` rejects any
   * scope.
   */
  scope?: "once" | "session";
  /**
   * Ed25519 signature over the decision message — v2 also binds `timestamp`
   * (see below). Required at runtime whenever the gateway runs with
   * `requireSession` on (the live pairing-gated mode, WP-15 P2-7); the type is
   * required to match that contract instead of hinting that unsigned decisions
   * are acceptable. Open-mode dev gateways (`requireSession: false`) tolerate
   * absence and only log a warning.
   */
  signature: string;
  /**
   * v2 anti-replay (docs/approval-decide-signing.md): epoch-ms timestamp bound
   * into the signed message (`approvalMessageV2`). Its presence switches the
   * gateway to v2-only verification within ±approvalDecideTimestampWindowMs;
   * absent means the legacy v1 shape, verified exactly as before.
   */
  timestamp?: number;
  note?: string | null;
}

export interface AgentDetailParams {
  session_id: string;
  /** Subscribe to the session's task.progress stream. */
  subscribe?: boolean;
}

/**
 * `shannon/task.dispatch` (cross-repo spec §K1) — send a prompt through the
 * IM-style inbound pipeline (per-device lane → approval loop → structured
 * task stream). `agent_id` is accepted on the wire but this host has NO agent
 * roster (`shannon/agent.list` is an empty stub), so ANY non-empty value is
 * rejected with INVALID_PARAMS rather than silently routed elsewhere.
 */
export interface TaskDispatchParams {
  prompt: string;
  agent_id?: string | null;
}

/**
 * `shannon/task.list` — recent tasks dispatched from this device with their
 * gateway-side status (running/completed/failed). In-memory journal, newest
 * first.
 */
export interface TaskListParams {
  limit?: number;
}

/**
 * `shannon/session.history` (cross-repo spec §J2) — fetch one session's
 * transcript. `sessionId` is required (the gateway rejects absence with
 * INVALID_PARAMS, unlike the mock's active-session fallback). Optional
 * pagination: `before` is the ISO-8601 ts of the oldest message the client
 * already holds, `limit` the page size (engine default 50).
 */
export interface SessionHistoryParams {
  sessionId: string;
  before?: string;
  limit?: number;
}

/**
 * `shannon/pair` — register a device key by consuming a one-time pairToken.
 * The phone generates its own Ed25519 keypair, then proves possession of the
 * private key with `pop_signature` over `${pair_token}:${device_public_key}`
 * (see mobile/crypto.ts). On success the gateway registers the device and binds
 * the connection's session.
 */
export interface PairParams {
  pair_token: string;
  /** JWK `x` — base64url of the 32-byte Ed25519 public key. */
  device_public_key: string;
  /** Ed25519 signature over `${pair_token}:${device_public_key}`. */
  pop_signature: string;
  /** Optional human-friendly label (device name) for the paired-devices UI. */
  device_label?: string | null;
}

/**
 * `shannon/device.resume` — a previously-paired device reconnects and proves
 * identity by signing `${device_id}:${timestamp}`. The timestamp bounds replay
 * (rejected outside ±clock-skew, default 60s). On success the connection's
 * session is rebound to the device (Z1 continuity).
 */
export interface DeviceResumeParams {
  device_id: string;
  /** Epoch milliseconds; must be within the gateway's clock-skew window. */
  timestamp: number;
  /** Ed25519 signature over `${device_id}:${timestamp}` — or, when `nonce` is
   *  sent, over `${device_id}:${timestamp}:${nonce}`. */
  signature: string;
  /**
   * Client-generated single-use value (random ≤128 chars). When present, the
   * gateway enforces one resume per nonce per device inside the skew window —
   * a captured resume can't be replayed. Absent on legacy clients, which fall
   * back to the monotonic-timestamp watermark only.
   */
  nonce?: string;
}

// ── Responses (gateway → phone) ──────────────────────────────────────────

export interface JsonRpcSuccess<R> {
  jsonrpc: typeof JSONRPC_VERSION;
  id: string | number;
  result: R;
}

export interface JsonRpcError {
  jsonrpc: typeof JSONRPC_VERSION;
  id: string | number | null;
  error: { code: number; message: string; data?: unknown };
}

export type JsonRpcResponse<R> = JsonRpcSuccess<R> | JsonRpcError;

// ── Notifications (gateway → phone, streaming) ───────────────────────────

/**
 * Server→phone notification carrying one event in a streaming turn. The phone
 * correlates a turn by the `query.started`/`turn_id` it sees first; subsequent
 * `task.progress` events belong to the in-flight turn on that socket.
 *
 * 0.7.0: `task.progress` now also carries `turn_id` directly (WP-15 P2-8), so
 * clients can correlate without relying on the one-in-flight-turn-per-socket
 * convention. Optional + additive — clients that ignore it keep working.
 */
export interface ShannonEventNotification {
  jsonrpc: typeof JSONRPC_VERSION;
  method: "shannon/event";
  params: ShannonEvent & {
    /**
     * WP-15 T4: push cursor stamped by the gateway on every notification
     * (mobile live_sync consumes it; absence is tolerated — legacy gateways).
     */
    seq?: number;
  };
}

export type ShannonEvent =
  | {
      type: "query.started";
      turn_id: string;
      /**
       * §K3: the routing key for a DISPATCHED task's stream — the task's own
       * id (the phone keys its local thread by it). Absent on plain query
       * turns (the direct `shannon/query` path), which route by turn_id.
       */
      session_id?: string;
    }
  | {
      type: "task.progress";
      content?: string;
      tool?: ToolFrame;
      usage?: UsageFrame;
      /** Turn this progress belongs to (WP-15 P2-8). Absent on legacy gateways. */
      turn_id?: string;
      /**
       * §K3: the task thread key on a dispatched task's stream (= the task's
       * own id). Absent on plain query turns.
       */
      session_id?: string;
      /** Push cursor (WP-15 T4). */
      seq?: number;
    }
  | { type: "query.completed"; model: string }
  | {
      type: "query.failed";
      error: string;
      /** §K3: a dispatched task's failure carries its thread key. */
      session_id?: string;
    }
  | { type: "query.cancelled" }
  | {
      type: "approval.request";
      request_id: string;
      tool_name: string;
      tool_input: unknown;
      description: string;
      is_destructive: boolean;
      diff_preview: string | null;
      /**
       * §L1 (additive): engine-side epoch-ms timestamp. Absent/omitted on
       * engines that don't supply it (the phone falls back to arrival time).
       */
      ts?: number;
      /** §L1 (additive): the requesting agent, when the engine supplies it. */
      agent?: { id: string | null; name: string | null };
      /** §L1 (additive): the engine's three-dimensional risk, when present. */
      risk?: {
        destructive?: boolean;
        scope: "local" | "repo" | "system";
        reversible: boolean;
      };
    }
  | {
      /**
       * §K3: a dispatched task's terminal reply — the final complete text. The
       * `session_id` IS the task's own id (the phone's thread key), and its
       * presence makes this the task stream's closing event (the P2-1 IM
       * bubble/lifecycle-stamp semantics only apply when `session_id` is
       * absent — the pre-§K fallback for plain chat channels).
       */
      type: "task.message";
      text: string;
      session_id?: string;
    }
  | {
      /**
       * §M2 (cross-repo spec): a paired device was revoked. Broadcast to every
       * OTHER online device so their settings screens drop the row. The phone
       * tolerates unknown event types, so older builds just ignore this.
       */
      type: "device.revoked";
      device_id: string;
    };

export interface ToolFrame {
  kind: "use" | "result";
  name: string;
  input?: unknown;
  output?: string;
}

export interface UsageFrame {
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

// ── Result shapes ──────────────────────────────────────────────────────────

export interface HealthResult {
  gateway: "ok";
  engine: "ok" | "down";
  version: string;
}

export interface ModelListResult {
  models: { id: string; label?: string | null }[];
  current: string | null;
}

export interface AgentListResult {
  agents: { session_id: string; platform: string; active: boolean }[];
}

/** `shannon/pair` / `shannon/device.resume` success — the connection is now bound. */
export interface DeviceSessionResult {
  /** The gateway's device id for this key (deterministic from the public key). */
  device_id: string;
  /** Bound per-connection session id; pass to subsequent methods as session_id. */
  session_id: string;
  /** Human-friendly label echoed back (pair only). */
  device_label?: string | null;
  /** Push cursor at resume time (WP-15 T4) — seeds the phone's live-sync. */
  lastSeq?: number;
}

export interface OkResult {
  ok: true;
}

// ── §K task dispatch shapes (cross-repo spec, mock-server aligned) ──────────

/**
 * One dispatched task on the wire (spec §K1/K2). `id` is the primary key AND
 * the task thread's conversation key (§K3: the `session_id` the task's
 * `task.progress` / `task.message` events carry). `agent_id` is always null
 * on this host (no agent roster); `created_at` is ISO-8601 UTC.
 */
export interface MobileTaskRecord {
  id: string;
  prompt: string;
  status: "running" | "completed" | "failed";
  agent_id: string | null;
  created_at: string;
}

/** `shannon/task.dispatch` success — the task object, synchronously, before any event. */
export interface TaskDispatchResult {
  task: MobileTaskRecord;
}

/** `shannon/task.list` success — newest first. */
export interface TaskListResult {
  tasks: MobileTaskRecord[];
}

// ── §L2 approval-restore + §J session shapes (cross-repo spec) ─────────────

/**
 * One pending approval as served by `shannon/approval.list` and carried in the
 * `shannon/snapshot` `pendingApprovals` array. Key names are camelCase and
 * map 1:1 onto the phone's `approvalFromMap` (shannon-mobile
 * `lib/src/live/protocol_mapper.dart`) — these keys ARE the contract.
 */
export interface MobileApprovalItem {
  /** The engine's request id (`request_id` on `approval.request` events). */
  approvalId: string;
  /** The tool that wants to run (`tool_name`). */
  kind: string;
  /** Human headline (`description`). */
  headline: string;
  /** `'high' | 'medium' | 'low'` — synthesized per the §L2 mapping. */
  risk: "high" | "medium" | "low";
  /** ISO-8601 UTC instant the request was recorded. */
  timestamp: string;
  /** The raw tool input, verbatim — feeds the phone's operation mono block. */
  toolInput: unknown;
  /** Initiating agent, when the engine supplies it (absent otherwise). */
  agentId?: string;
  agentName?: string;
  /** Risk scopes from the engine's three-dimensional risk, when present. */
  scope?: string[];
  /** `tool_input.path` when it is a string (diff header for the phone). */
  diffTitle?: string;
}

/** `shannon/approval.list` success — verbatim envelope key per the contract. */
/** P3-3: result of `shannon/approval.state` — the approval token currently
 *  in effect for the device's attached session. */
export interface ApprovalStateResult {
  mode: string;
}

/** P3-3: params for `shannon/approval.set` — the mobile TIGHTEN route; the
 *  gateway only forwards `readonly` (a phone may clamp a session, never
 *  loosen or escalate it). */
export interface ApprovalSetParams {
  mode: "readonly";
}

export interface ApprovalListResult {
  pendingApprovals: MobileApprovalItem[];
}

/** One session summary as served by `shannon/session.list` (spec §J1). */
export interface MobileSessionSummary {
  /** Global session primary key — required; entries without it are useless. */
  id: string;
  /** Owning agent id — required by the phone's parser; omitted until the
   *  engine exposes it (the phone then honestly skips the entry). */
  agentId?: string;
  /** UTF-8 title; the phone falls back to the agent name when absent. */
  title?: string;
  /** ISO-8601 UTC last-activity instant (list ordering). */
  updatedAt?: string;
  /** Lifetime token totals (C8, additive): present only when the engine
   *  supplied a usable number — the phone renders spend "有数据才渲染". */
  totalInputTokens?: number;
  totalOutputTokens?: number;
}

/** `shannon/session.list` success. */
export interface SessionListResult {
  sessions: MobileSessionSummary[];
}

/** One transcript message as served by `shannon/session.history` (§J2). */
export interface MobileSessionMessage {
  role: string;
  content: string;
  /** ISO-8601 UTC; the phone stamps arrival time when absent. */
  ts?: string;
}

/** `shannon/session.history` success. */
export interface SessionHistoryResult {
  sessionId: string;
  messages: MobileSessionMessage[];
  /** True when older messages exist beyond this page (spec §J2 pagination). */
  hasMore: boolean;
}

// ── T9: desktop pairing-approval shapes ────────────────────────────────────

/**
 * `shannon/pairing.pending` — list the IM pairing challenges awaiting approval.
 * Caller trust: a paired device session, or a valid (unconsumed) pair token —
 * the desktop host's credential (see mobile/accessRpc.ts).
 */
export interface PairingPendingParams {
  /**
   * One-time pair token minted by the desktop (its Design-D control channel).
   * Verified, not consumed, for this read-only call. Optional for callers that
   * already hold a bound device session (a paired phone).
   */
  token?: string;
}

/** One pending pairing request, mirroring the IM guard's PairingRecord. */
export interface PairingRequestRecord {
  /** The 6-digit code shown in the IM challenge (also what approval consumes). */
  code: string;
  /** Chat platform the requester came from (slack/telegram/…). */
  platform: string;
  /** Platform sender id the allowlist entry will carry. */
  senderId: string;
  /** Epoch ms when the challenge was issued. */
  requestedAt: number;
  /** Epoch ms after which the code dies (issue + 5 min). */
  expiresAt: number;
}

/** `shannon/pairing.pending` success — oldest first, expired codes pruned. */
export interface PairingPendingResult {
  pending: PairingRequestRecord[];
}

/**
 * `shannon/pairing.approve` — approve (allowlist) a pending IM pairing. Same
 * store the IM `approve <code>` reply consumes; unknown/expired codes are
 * rejected. Caller trust: paired device session, or a pair token which IS
 * consumed (this call mutates access control).
 */
export interface PairingApproveParams {
  /** The challenged sender's 6-digit code. */
  code: string;
  /** One-time pair token (consumed on success or failure to authorize). */
  token?: string;
}

/** `shannon/pairing.approve` success — the approved request (now allowlisted). */
export interface PairingApproveResult {
  ok: true;
  record: PairingRequestRecord;
}

// ── Error codes ────────────────────────────────────────────────────────────

/**
 * Stable error codes. JSON-RPC reserves -32xxx for protocol errors; app errors
 * use -32xxx too where they map cleanly (method-not-found, not-implemented) and
 * the -32000 custom band for shannon-specific conditions.
 */
/**
 * Error-code registry (shannon band -32000..-32099). Assign new codes from the
 * lowest unused slot and mirror them in shannon-mobile `json_rpc.dart` RpcError;
 * -32010..-32013 are reserved for the mock-only rich surface. Assigned:
 *   -32000 PAIRING_REQUIRED   -32001 BAD_PARAMS      -32002 ENGINE_ERROR
 *   -32003 CLOCK_SKEW          -32014 GAP_TOO_LARGE
 */
export const ShannonError = {
  PARSE_ERROR: -32700,
  INVALID_REQUEST: -32600,
  METHOD_NOT_FOUND: -32601,
  /** Method recognized but not implemented yet (e.g. pairing before P1.2). */
  NOT_IMPLEMENTED: -32603,
  /** Device must complete pairing first — the P1.2 auth gate. */
  PAIRING_REQUIRED: -32000,
  BAD_PARAMS: -32001,
  ENGINE_ERROR: -32002,
  /**
   * `shannon/device.resume` timestamp outside the skew window (or replayed).
   * Dedicated code so clients don't classify skew by matching message text.
   */
  CLOCK_SKEW: -32003,
  /** Resume cursor beyond the gateway's retained event window (WP-15 T4). */
  GAP_TOO_LARGE: -32014,
} as const;

// ── NDJSON codec ───────────────────────────────────────────────────────────

/**
 * Parse one WebSocket text frame. The mobile protocol sends one JSON object per
 * frame, but the codec tolerates newline-delimited multiples and blank lines.
 * Unparseable records yield `null` (the dispatcher reports a parse error for them).
 */
export function parseNdjson(frame: string): unknown[] {
  return frame
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      try {
        return JSON.parse(line);
      } catch {
        return null;
      }
    });
}

export function serializeFrame(value: unknown): string {
  return JSON.stringify(value);
}

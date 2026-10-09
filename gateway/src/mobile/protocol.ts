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

import type { RosterAgent } from "./agentRoster.js";

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
  | "shannon/trust.list"
  | "shannon/trust.revoke"
  | "shannon/session.list"
  | "shannon/session.history"
  | "shannon/push.register"
  | "shannon/usage.budget"
  | "shannon/group.list"
  | "shannon/group.get"
  | "shannon/group.create"
  | "shannon/group.message"
  | "shannon/group.archive";

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
  "shannon/trust.list",
  "shannon/trust.revoke",
  "shannon/session.list",
  "shannon/session.history",
  "shannon/push.register",
  "shannon/usage.budget",
  "shannon/group.list",
  "shannon/group.get",
  "shannon/group.create",
  "shannon/group.message",
  "shannon/group.archive",
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
   * `always_allow_session` (in-session always-allow, never persisted). N3:
   * `"kind"` maps to `always_allow_kind` — a PERSISTED per-kind trust grant
   * (`params.kind` is required, exact-match against the approval's tool
   * name; there is no global always-allow). A scope is bound into the
   * decision signature; `deny` rejects any scope.
   */
  scope?: "once" | "session" | "kind";
  /**
   * N3: with `scope: "kind"` — the category to trust. Must equal the
   * approval's own kind (the engine tool name) verbatim; the engine
   * degrades a mismatch to a one-shot allow so a buggy scope can neither
   * widen trust nor veto the operation the human approved.
   */
  kind?: string;
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
 * task stream). B0: `agent_id` names a CONFIGURED agent — validated against
 * the host roster (`shannon/agent.list`, `~/.shannon/agents/*.toml`); a hit
 * is accepted and recorded as the task's attribution, anything else is
 * rejected with INVALID_PARAMS rather than silently routed elsewhere.
 *
 * Boundary: the engine has NO per-agent routing face — the recorded agent_id
 * never changes what executes (the turn still runs as the default engine);
 * it only says "this configured agent owns the task" (wire task object +
 * `shannon/session.list` enrichment).
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
      /**
       * §S (additive, B6.0): group-approval attribution — absent on plain
       * approvals, whose rendering stays byte-identical. `ruleTrigger` is
       * emitted only when the orchestrator knows it deterministically (v1:
       * `handoff-first` only); payment/pool triggers have no producer yet.
       */
      group?: ApprovalGroupInfo;
    }
  | {
      /**
       * §S (B6.0-3, additive): a handoff card — deterministic orchestrator
       * step (v1), `session_id` = groupId. `note` is the machine-composed
       * context transfer (truncated prior member output), not authored prose.
       */
      type: "group.handoff";
      session_id: string;
      handoff: {
        id: string;
        from: string;
        to: string;
        ts: string;
        note: string;
      };
    }
  | {
      /** §S (B6.0-3, additive): member status progression on the group. */
      type: "group.member";
      session_id: string;
      member: {
        memberId: string;
        status: "idle" | "queued" | "working" | "waiting-approval" | "done" | "failed" | "archived";
        statusNote: string | null;
      };
    }
  | {
      /**
       * §S (B6.0-3, additive): the system status card (quote-expired /
       * member-failed / reassigned / member-archived / group-completed…).
       * `text` is host-composed human prose; `kind` selects the card style.
       * Unknown kinds render the generic system card (mobile degrade).
       */
      type: "group.system";
      session_id: string;
      system: { kind: string; text: string; ts: string };
    }
  | {
      /**
       * §S (B6.1, additive): one settled pool spend — an approval carrying a
       * parseable amount was ALLOWED and the group ledger took the entry.
       * `poolSpend` mirrors that ledger line verbatim (see `PoolSpendInfo`);
       * denials and expiry never emit this (拒绝不记账). The phone renders
       * it as the group thread's spend receipt and reconciles its pool
       * numbers from `poolAfterCny`.
       */
      type: "group.pool-spend";
      session_id: string;
      poolSpend: PoolSpendInfo;
    }
  | {
      /**
       * §S (B6.2, additive): the host-composed group daily report (R11) —
       * aggregated deterministically from the group store (transcript +
       * group.json pool/member fields) at the group's configured local
       * `rules.dailyReportAt`. `text` is the human summary; the structured
       * keys are the render fallback when text is missing. Host offline at
       * the report minute → the day's report is skipped (never backfilled);
       * `todaySpentCny` stays an honest 0 — the per-day ledger face is not
       * built yet (B6.1 supplies the pool CUMULATIVE, see
       * `review.pool.spentCny`), never a fabricated number. Deliberately
       * NOT in the §O3 wake whitelist — a daily report is non-interactive
       * (no ring, notification-tier rendering only).
       */
      type: "group.report";
      session_id: string;
      report: {
        /** Local-calendar day key, `YYYY-MM-DD`. */
        date: string;
        kind: "daily";
        /** Host-aggregated human prose (never fabricated numbers). */
        text: string;
        goalProgress: { done: number; total: number };
        /** Honest 0 — per-day ledger face not built; pool cumulative lives
         *  in review.pool.spentCny (B6.1). */
        todaySpentCny: number;
        ts: string;
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
  /**
   * §R additive keys (tool-result cards): recovered engine fields the WS
   * face used to drop. `tool_use_id` pairs a use→result frame into one
   * card, `is_error` drives the failure ribbon, `meta` is §4.12
   * tool-private metadata passed through uninterpreted, `ts` is the
   * engine's epoch-ms forward stamp (§L1 precedent). Every key is optional:
   * old engines never send them and the phone hides the missing pieces
   * per-field (no duration exists on the live face by ruling — L0 history
   * artifacts are the only duration source).
   */
  tool_use_id?: string;
  is_error?: boolean;
  meta?: unknown;
  ts?: number;
}

export interface UsageFrame {
  input_tokens: number;
  output_tokens: number;
  cost_usd: number;
}

// ── P2-9: agent.state push (gateway → phone, roster live status) ────────────

/**
 * One agent map inside a `shannon/agent.state` notification (P2-9). This is
 * NOT a `shannon/event` type — the phone consumes a dedicated notification
 * METHOD (`shannon/agent.state`, shannon-mobile `Notifications.agentState`)
 * whose params are `{agent: {...}}`, parsed by `agentFromMap`
 * (lib/src/live/protocol_mapper.dart). The mock server's broadcast
 * (`{'agent': {...agent, 'status': ...}}`) is the wire's reference shape.
 *
 * Field names follow THAT mapper (the only consumer), not the gateway-wide
 * snake_case rule: `currentTask` is camelCase on this face, `id`/`name`/
 * `status`/`activity` carry the same keys `shannon/agent.list` serves. The
 * map must be the FULL roster shape — the phone's consumer REPLACES the
 * matching roster entry wholesale (unknown ids are ADDED), so a minimal
 * `{id, status}` push would visibly degrade `name`/`role`/`model` on the
 * Fleet screen. `role`/`model` stay optional exactly like `agent.list`
 * (omitted when the definition doesn't declare them).
 */
export interface MobileAgentState {
  /** The roster agent id (the `~/.shannon/agents/*.toml` `name`). */
  id: string;
  /** Same string as `id` (roster convention — `agent.list` serves it so). */
  name: string;
  /** The definition's description — omitted when absent/empty. */
  role?: string;
  /** The definition's model — omitted when absent/empty. */
  model?: string;
  /** Live status: `"running"` from task acceptance, `"idle"` at a terminal. */
  status: "running" | "idle";
  /** Kept for shape parity with `agent.list` (always empty today). */
  activity: string[];
  /** The task text while running; explicitly `null` once idle. */
  currentTask: string | null;
}

/**
 * The P2-9 notification frame. Deliberately seq-free and outside the §O4
 * replay ring: the phone's `agent.state` consumer reads no cursor, and
 * ephemeral roster status must not replay through the `shannon/event`
 * resume path — the Fleet screen re-converges via `shannon/agent.list` on
 * its next (re)bind.
 */
export interface AgentStateNotification {
  jsonrpc: typeof JSONRPC_VERSION;
  method: "shannon/agent.state";
  params: { agent: MobileAgentState };
}

// ── trust.changed push (gateway → phone, per-kind trust ledger live-sync) ────

/**
 * The `shannon/trust.changed` notification — pushed when a per-kind trust
 * grant is REVOKED through this gateway (`shannon/trust.revoke` → engine
 * `/api/trust/revoke` answering `revoked: true`). Grants push nothing: the
 * deciding device already learns of a grant from its signed decide response
 * (§Q2), and other devices pick the grant up on their next `shannon/trust.list`
 * probe — the §Q5 v1 degrade, unchanged.
 *
 * Like `shannon/agent.state`, this is a dedicated notification METHOD (not a
 * `shannon/event` type) and deliberately seq-free / outside the §O4 replay
 * ring: the engine's kind-trust store is the truth, the push is only a
 * live-sync hint, and a device that misses it (offline, gateway restart)
 * falls back to exactly the pre-push behavior — re-probe `shannon/trust.list`
 * when the trust ledger is next opened. Nothing is lost, so nothing needs
 * replaying. Broadcast to EVERY connected device: the store
 * (`~/.shannon/trust/kinds.toml`) is a host-level entity, not device-private.
 *
 * `kind` is the revoked tool name (exact-match semantics, §Q1). `revokedAt`
 * is ISO-8601 UTC stamped at the gateway at the revoke moment — the engine's
 * revoke response carries no timestamp, and the gateway invents nothing
 * beyond that forward wall-clock moment (the §L1 `ts` stamping posture).
 */
export interface TrustChangedNotification {
  jsonrpc: typeof JSONRPC_VERSION;
  method: "shannon/trust.changed";
  params: { kind: string; revokedAt: string };
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

/**
 * `shannon/agent.list` — the host's configured agent roster (B0, v2.3). One
 * entry per parseable `~/.shannon/agents/*.toml` definition; `status` is the
 * fixed "idle" and `activity` the always-empty placeholder (configured
 * agents, not live processes). The previous `{session_id, platform, active}`
 * element shape was a mock-era leftover that no mobile mapper ever consumed —
 * replaced outright in v2.3 (intentional contract correction, not additive:
 * the old keys were never produced by the real gateway).
 */
export interface AgentListResult {
  agents: RosterAgent[];
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
 * `task.progress` / `task.message` events carry). B0: `agent_id` is the
 * roster agent the task was dispatched under (validated against
 * `shannon/agent.list` at accept time), or null when the dispatch carried
 * none — attribution only, not engine routing (see `TaskDispatchParams`);
 * `created_at` is ISO-8601 UTC.
 *
 * B1a (v2.3 additive): three optional keys return — `title` rides whenever
 * the internal task title is non-empty, `finished_at` (ISO-8601 UTC) only
 * once the record is terminal, `error` only on `status: "failed"` with a
 * non-null error. All three may be absent; consumers of the legacy five-key
 * shape keep working.
 */
export interface MobileTaskRecord {
  id: string;
  prompt: string;
  status: "running" | "completed" | "failed";
  agent_id: string | null;
  created_at: string;
  /** B1a (additive): the task title — present whenever non-empty. */
  title?: string;
  /** B1a (additive): ISO-8601 UTC terminal instant — only once finished. */
  finished_at?: string;
  /** B1a (additive): the failure reason — only on failed tasks with an error. */
  error?: string;
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
// ── §S group orchestration (B6.0; cross-repo spec) ──────────────────────────

/**
 * One settled pool spend (§S B6.1, `group.pool-spend` events) — the wire
 * mirror of the host-side `ledger.jsonl` entry (groupStore): 元 with two
 * decimals, `poolAfterCny` the group pool's `spentCny` AFTER this entry
 * landed. Broadcast to every connected device; it is a projection of the
 * ledger, never a second authority.
 */
export interface PoolSpendInfo {
  id: string;
  memberId: string;
  amountCny: number;
  kind: string;
  approvalId: string;
  decidedBy: string;
  poolAfterCny: number;
  ts: string;
}

/** Group member attribution on approvals / group events (camelCase wire). */
export interface ApprovalGroupMemberInfo {
  memberId: string;
  label: string;
  title: string;
  source: "ephemeral" | "roster";
}

/**
 * The `group` key on `approval.request` events and `approval.list` /
 * `snapshot.pendingApprovals` items. `ruleTrigger` is emitted only when the
 * orchestrator knows it deterministically, one key per ask: v1 has the R5
 * one-shot `handoff-first`, and B6.1 adds the pool-ledger escalations
 * `over-pool` / `over-share` (computed only when the ask carries a parseable
 * amount; `handoff-first` wins the key when both hit — the over-* verdict is
 * still enforced, just not double-labeled). `payments-ask-first` stays
 * unproduced (no payment-class tool classifier — 宁缺勿造).
 *
 * `poolAfter` rides B6.1 asks whose amount was resolvable from the tool
 * input: `poolCny` is the group pool's total (元), `remainingAfterCny` is
 * `total − spent − pending − amount` if this ask were allowed — honest even
 * when negative (an over-pool ask shows the hole it would dig).
 *
 * `quoteWindow` (B6.0-4, R3/R4) rides the SAME amount-bearing asks: the
 * 锁价/拍板 window the phone renders as 「HH:MM 前有效」+ countdown. Its
 * presence is also the TTL contract: the ask parks at the gateway (and the
 * engine's approval resolver waits) until `expiresAt`, not the legacy 300s —
 * the engine clamps any per-request window at a hard 60 minutes, and v1
 * produces only the 30-minute design window (`windowMinutes: 30`).
 * `onExpire` is constant `"requote-next"` in v1: the window dying at the
 * gateway auto-abandons the ask (deny + pending refund + the
 * `group.system {kind: "quote-expired"}` card); when no second candidate
 * exists the turn then fails into the member-failed reassignment叙事 — no
 * fabricated requote action. Plain (groupless) asks and no-amount group asks
 * never carry the key: they keep the exact legacy 300s window everywhere.
 */
export interface ApprovalQuoteWindow {
  /** ISO-8601 UTC — the instant the ask auto-abandons (R4 「14:41 前有效」). */
  expiresAt: string;
  /** Whole minutes; v1 producer emits only 30. */
  windowMinutes: number;
  /** Constant `"requote-next"` in v1 (提案 §B6.0-4). */
  onExpire: "requote-next";
}

export interface ApprovalGroupInfo {
  groupId: string;
  member: ApprovalGroupMemberInfo;
  ruleTrigger?: "handoff-first" | "over-pool" | "over-share";
  poolAfter?: { poolCny: number; remainingAfterCny: number };
  quoteWindow?: ApprovalQuoteWindow;
}

/** `shannon/group.list` item (B6.0-1 projection; optional keys degrade). */
export interface GroupListItem {
  groupId: string;
  title: string;
  goalSummary?: string;
  status: "active" | "completed" | "archived";
  memberCount?: number;
  pool?: { totalCny: number; spentCny: number; pendingCny: number };
  lastActivityAt?: string;
}

export interface GroupListResult {
  groups: GroupListItem[];
}

/** One member in a `group` object (create response / transcript entries). */
export interface GroupMemberInfo extends ApprovalGroupMemberInfo {
  slot: string;
  agentId: string | null;
  shareCny: number;
  spentCny: number;
  status: "idle" | "queued" | "working" | "waiting-approval" | "done" | "failed" | "archived";
  statusNote: string | null;
  permissions: string[];
}

export interface GroupRulesInfo {
  /** Locked: `false` is rejected at create (the "payments always ask" red line). */
  paymentsAskFirst: true;
  infoBoundary: string;
  handoffFree: boolean;
  /** Local-timezone HH:mm, or null = off (B6.2 stores it today, enacts later). */
  dailyReportAt: string | null;
}

export interface GroupObject {
  groupId: string;
  title: string;
  goal: string;
  status: "active" | "completed" | "archived";
  createdAt: string;
  members: GroupMemberInfo[];
  pool: { totalCny: number; spentCny: number; pendingCny: number; reserveCny: number };
  rules: GroupRulesInfo;
  /**
   * §S (B6.2, additive): the 16-screen review aggregate — present ONLY on
   * archived/completed groups that were closed with
   * `group.archive {reason: "completed"}` on a B6.2-capable gateway (it is
   * computed once at archive time and persisted in group.json; it never
   * appears on group.list/group.create). Groups archived before B6.2 — or
   * disbanded — honestly omit the key.
   */
  review?: GroupReview;
}

// ── §S B6.2: the review aggregate (16 复盘面) + the daily report ────────────

/** One human key-value row of a deliverable (rendered verbatim by the phone). */
export interface GroupReviewLine {
  k: string;
  v: string;
}

/**
 * One done member's deliverable row. v1 posture (宁缺勿造): `title` is the
 * member's own 分工 title, `status` is the fixed 「已完成」, `lines` is a
 * first-line summary of the member's last produced transcript entry (empty
 * when none exists), and `artifact` is ALWAYS null — the R14 desktop deep
 * link has no protocol yet (the phone renders a 「桌面深链 · 待协议」 note).
 */
export interface GroupReviewDeliverable {
  memberId: string;
  title: string;
  status: string;
  lines: GroupReviewLine[];
  artifact: null;
}

/** One member's pool-spend row (16 「钱花在哪」) — spentCny > 0 members only. */
export interface GroupReviewPoolMember {
  memberId: string;
  label: string;
  title: string;
  amountCny: number;
}

/**
 * The deterministic, host-side review aggregate for a closed group (16).
 * Every number is read from the group store's single sources (group.json
 * member/pool fields + the transcript) — nothing is invented:
 *  - `goals.done/total` = members whose status was "done" at archive time /
 *    total members;
 *  - `durationMinutes` = createdAt → last transcript entry ts (floored);
 *  - `handoffCount` = handoff system-card entries in the transcript
 *    (pre-B6.2 transcripts carry no marker → an honest 0 lower bound);
 *  - `decisionCount` = member-approval 拍板 count — v1 has NO approval
 *    attribution counting face, so this is HONESTLY 0 (never estimated);
 *  - `pool` comes verbatim from group.json (B6.1's settle chain feeds
 *    spentCny/perMember from the ledger).
 */
export interface GroupReview {
  goals: { done: number; total: number };
  durationMinutes: number;
  handoffCount: number;
  decisionCount: number;
  pool: {
    totalCny: number;
    spentCny: number;
    perMember: GroupReviewPoolMember[];
  };
  deliverables: GroupReviewDeliverable[];
}

export interface GroupCreateParams {
  goal: string;
  /** "ephemeral" (default) | "roster". */
  path?: "ephemeral" | "roster";
  /** Roster path: required, every member needs a roster agentId. Ephemeral:
   *  optional — omitted = orchestrator plans (v1: the generic 3-slot template). */
  members?: Array<{
    slot: string;
    label: string;
    title: string;
    agentId?: string | null;
    shareCny?: number;
    permissions?: string[];
  }>;
  pool?: { totalCny?: number; reserveCny?: number };
  rules?: Partial<GroupRulesInfo>;
}

export interface GroupCreateResult {
  group: GroupObject;
}

export interface GroupMessageParams {
  groupId: string;
  text: string;
  mentionMemberId?: string;
}

export interface GroupMessageResult {
  messageId: string;
  ts: string;
}

/**
 * `shannon/group.get` (§S B6.2 收尾面) — one group entity by id. Unknown
 * ids are INVALID_PARAMS (the §J2-for-groups honest miss), never an empty
 * object. Archived/completed groups additionally carry `review` when one
 * was persisted at archive time (see [GroupObject.review]).
 */
export interface GroupGetParams {
  groupId: string;
}

export interface GroupGetResult {
  group: GroupObject;
}

export interface GroupArchiveParams {
  groupId: string;
  reason: "completed" | "disbanded";
}

/**
 * B6.2 (additive): `reason: "completed"` also aggregates and returns the
 * 16-screen `review` (and persists it into group.json for `group.get`);
 * `reason: "disbanded"` honestly omits it (可缺席).
 */
export interface GroupArchiveResult {
  ok: true;
  review?: GroupReview;
}

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
  /** §S (additive): group attribution — absent on plain approvals. */
  group?: ApprovalGroupInfo;
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

// ── N3 per-kind trust shapes (cross-repo spec §Q) ──────────────────────────

/**
 * One active per-kind trust grant as served by `shannon/trust.list`. The
 * kind is the engine tool name, matched EXACTLY on later approval requests —
 * there is no global always-allow and no cross-kind semantics. `grantedAt`
 * is ISO-8601 UTC (the phone renders it or hides it — never invents one).
 */
export interface TrustedKindItem {
  kind: string;
  grantedAt: string;
}

/**
 * `shannon/trust.list` success — the active grants, sorted by kind. An old
 * gateway answers METHOD_NOT_FOUND: that absence IS the capability signal
 * (the `shannon/usage.budget` degrade precedent — the capability name is
 * `trust.kind`), and the phone's trust switch hides/disables accordingly.
 */
export interface TrustListResult {
  kinds: TrustedKindItem[];
}

/**
 * `shannon/trust.revoke` params — the kind to revoke. Idempotent: an unknown
 * kind still answers ok (the grant is gone either way), so a retried revoke
 * (the phone's offline queue) cannot fail.
 */
export interface TrustRevokeParams {
  kind: string;
}

/** `shannon/trust.revoke` success — revocation takes effect immediately. */
export interface TrustRevokeResult {
  ok: true;
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

/**
 * `shannon/usage.budget` (B2, v2.3) — read-only month-to-date budget
 * snapshot, mirroring the desktop's Usage-governance numbers (camelCase keys,
 * same `usage_governance.rs` window convention).
 *
 * Boundary (deliberate): the ledger only records DESKTOP engine sessions —
 * gateway/mobile-side task spend is not written there yet, so `monthCostUsd`
 * is a LOWER bound of real spend, never an overstatement.
 */
export interface UsageBudgetResult {
  /** Local-calendar month key, `"%Y-%m"`. */
  month: string;
  /** Month-to-date USD spend summed from `~/.shannon/usage.jsonl` (lower bound — see above). */
  monthCostUsd: number;
  /** User-set monthly budget from `~/.shannon/desktop/config.json`; null when unset/not a number. */
  budgetUsd: number | null;
  /** v1: always null — per-session caps live in session sidecars (not read). Forward-compat placeholder. */
  sessionCapUsd: null;
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

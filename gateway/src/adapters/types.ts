/**
 * Transport-adapter contract for the gateway (layer 1 of the four-layer
 * model). One adapter per chat platform; produces NormalizedInbound and
 * consumes ReplyTarget.
 *
 * These are the gateway-internal types and use idiomatic camelCase. They are
 * distinct from the engine *wire* types (src/engine/types.ts), which stay
 * snake_case to match the Rust serde shapes 1:1.
 *
 * Source of truth: §4.3 of shannon-desktop/claudedocs/social-connection-architecture.md.
 * The doc defers finalizing ApprovalReq / ApprovalDecision / MessageReceipt /
 * AdapterContext to "Phase 1 TS impl" — that finalization lives here.
 */

/** Platforms with a first-class adapter. Subprocess transports (Signal/iMessage) join later. */
export type Platform =
  | "slack"
  | "telegram"
  | "discord"
  | "whatsapp"
  | "matrix"
  | "wecom"
  | "feishu"
  | "dingtalk"
  /** P2-1: the paired-phone channel. Not a chat platform — the adapter lifts
   *  connected mobile devices into the same router/lifecycle pipeline the IM
   *  adapters use (dispatch → lane → approval → lifecycle push back to the phone). */
  | "mobile";

export const PLATFORMS: readonly Platform[] = [
  "slack",
  "telegram",
  "discord",
  "whatsapp",
  "matrix",
  "wecom",
  "feishu",
  "dingtalk",
  "mobile",
];

/**
 * Declarative capability sheet (OpenClaw-style). Adding a platform = filling
 * this table + passing the contract test, not editing a dispatch switch.
 */
export interface AdapterCapabilities {
  /** Thread semantics (Slack thread / TG reply / Discord thread). */
  threading: boolean;
  /** Supports DM pairing (F14 allowlist flow). */
  pairing: boolean;
  /** Can render in-channel approval buttons. */
  approvalButtons: boolean;
  /** Streaming render capability. */
  streaming: "none" | "partial" | "block";
  /** Inbound MIME types the adapter can ingest. */
  mediaIn?: string[];
  /** Outbound MIME types the adapter can emit. */
  mediaOut?: string[];
  /** Message edit window in ms (TG 48h, Slack ∞, Discord 1h). */
  editWindowMs?: number;
  /** Min interval between progress edits, to dodge TG flood control. */
  progressThrottleMs?: number;
}

/** Minimal structural logger; adapters must not depend on a specific lib. */
export interface Logger {
  debug(msg: string, ...args: unknown[]): void;
  info(msg: string, ...args: unknown[]): void;
  warn(msg: string, ...args: unknown[]): void;
  error(msg: string, ...args: unknown[]): void;
}

/**
 * What the host hands an adapter at start(). Credentials are accessed by name
 * through `getSecret` (OS keyring, F14) — raw tokens never cross this boundary
 * and never live in the repo or process argv.
 */
export interface AdapterContext {
  logger: Logger;
  /** Read a named secret from the OS keyring. Resolves null if absent. */
  getSecret(key: string): Promise<string | null>;
}

/** A media payload attached to an inbound or outbound message. */
export interface MediaAttachment {
  kind: "image" | "audio" | "video" | "file";
  mimeType: string;
  /** Remote URL when the platform hosts it. */
  url?: string;
  /** Inline bytes when the platform delivers content directly. */
  data?: Uint8Array;
  caption?: string;
}

/**
 * Unified inbound envelope. F4 fix: senderId/senderName are always populated
 * (the legacy desktop inbound path left sender empty).
 */
export interface NormalizedInbound {
  platform: Platform;
  chatId: string;
  /** Thread / reply / forum-post id when the platform threads. */
  threadId?: string;
  senderId: string;
  senderName: string;
  text: string;
  media?: MediaAttachment[];
  /** Epoch ms. */
  timestamp: number;
  /** True for a 1:1 direct message (eligible for DM pairing); false/absent for group channels. */
  isDirect?: boolean;
  /**
   * Mobile §K only: the engine session id this turn must run under — the
   * dispatched task's own UUID (the §K3 conversation key). The engine's WS
   * gate rejects every non-UUID session_id query frame, and the per-device
   * lane default (`mobile:<deviceId>`) is exactly that, so the dispatch hub
   * pins the task id here and the §K3 task turn handler passes it to
   * `runQuery`. Absent on every other platform — IM turns keep the lane's
   * stable session key (behavior unchanged).
   */
  engineSessionId?: string;
  /** The platform-native event, for adapter-specific fallback. */
  raw?: unknown;
}

/** Where to send an outbound message. Thread continuity (F4) lives here. */
export interface ReplyTarget {
  platform: Platform;
  chatId: string;
  threadId?: string;
}

/** Optional outbound controls. */
export interface SendOpts {
  /** Force a thread / reply target. */
  threadId?: string;
  /** Message id being replied to (platform-specific). */
  replyTo?: string;
  /** Edit an existing message in place (progress updates within editWindowMs). */
  editMessageId?: string;
  attachments?: MediaAttachment[];
  /** Send visibly only to one user (slash-command responses, approvals). */
  ephemeralTo?: string;
}

/** Confirmation that a send succeeded; carries the id for later edits. */
export interface MessageReceipt {
  messageId: string;
  threadId?: string;
  /** Epoch ms of the edit if this was an edit, else undefined. */
  editedAt?: number;
}

/**
 * A tool-use approval to render in-channel. Mirrors the engine's
 * ApprovalRequest (engine wire: snake_case) in gateway-internal camelCase.
 *
 * B1b (v2.3 additive): the engine's rich fields ride through when supplied —
 * older engines (and older call sites) keep the legacy six-key shape, and no
 * consumer may invent values for absent keys.
 */
export interface ApprovalReq {
  requestId: string;
  toolName: string;
  toolInput: unknown;
  description: string;
  isDestructive: boolean;
  diffPreview: string | null;
  /** §L1 (additive): engine-side epoch-ms timestamp of the ask. */
  ts?: number;
  /** §L1 (additive): the requesting agent, when the engine supplies it. */
  agent?: { id: string | null; name: string | null };
  /**
   * §L1 (additive): the engine's three-dimensional risk, when present — same
   * normalized shape `mapEngineEvent` emits on the mobile wire.
   */
  risk?: {
    destructive?: boolean;
    scope: "local" | "repo" | "system";
    reversible: boolean;
  };
  /**
   * §S (additive, B6.0): group attribution set by the group orchestrator for
   * member-turn asks. Plain (non-group) turns never set it, so their wire
   * shape stays byte-identical. `ruleTrigger` carries AT MOST ONE trigger per
   * ask (`handoff-first` wins when a pool escalation also hits); `poolAfter`
   * rides asks whose amount was parseable (B6.1); `quoteWindow` (B6.0-4, R4)
   * rides the same amount-bearing asks — the 锁价 countdown the phone
   * renders, mirroring this ask's `deadlineMs` on the wire.
   */
  group?: {
    groupId: string;
    member: { memberId: string; label: string; title: string; source: "ephemeral" | "roster" };
    ruleTrigger?: "handoff-first" | "over-pool" | "over-share";
    poolAfter?: { poolCny: number; remainingAfterCny: number };
    quoteWindow?: {
      expiresAt: string;
      windowMinutes: number;
      onExpire: "requote-next";
    };
  };
  /**
   * §S B6.0-4 (additive): per-ask deadline override in ms — how long THIS ask
   * parks before the hub denies it (and the registry drops it), instead of
   * the gateway's default 300s. The group orchestrator sets it for
   * amount-bearing asks (the R4 quote window) so the ask outlives the
   * default window; without it the ask keeps the exact legacy behavior. The
   * engine side of the same window rides the query frame's
   * `approval_ttl_ms` (shannon-api-protocol, hard-capped at 60 minutes).
   */
  deadlineMs?: number;
}

/** User's decision on a rendered approval. Maps to the engine's PermissionChoice. */
export interface ApprovalDecision {
  requestId: string;
  choice: "allow" | "deny";
  reason?: string;
}

/**
 * Decomposed routing key for a raw platform conversation id. Used by the
 * session router (P1-c) to recover {baseChatId, threadId} so a thread and its
 * parent channel can share or split sessions as the platform demands.
 */
export interface SessionConversation {
  baseChatId: string;
  threadId?: string;
  /** Ancestry for platforms whose "thread" is a chain of message ids. */
  parentCandidates?: string[];
}

/**
 * The adapter contract. Implementations are platform-specific; the registry
 * and router depend only on this interface.
 */
export interface ChannelAdapter {
  readonly platform: Platform;
  readonly capabilities: AdapterCapabilities;

  /** Connect and begin consuming platform events. Push inbound via onMessage. */
  start(ctx: AdapterContext): Promise<void>;
  /** Disconnect and release resources. Idempotent. */
  stop(): Promise<void>;

  /** Register the inbound handler. Called once between start/stop. */
  onMessage(handler: (m: NormalizedInbound) => void): void;

  /** Send a text (with optional media/edits) to a target. */
  send(target: ReplyTarget, text: string, opts?: SendOpts): Promise<MessageReceipt>;

  /**
   * Render an approval request in-channel and resolve with the user's choice.
   * Blocks until decided or timed out. Implemented per-platform in P1-f.
   */
  requestApproval(target: ReplyTarget, req: ApprovalReq): Promise<ApprovalDecision>;

  /** Split a raw conversation id into routing components. */
  resolveSessionConversation(rawId: string): SessionConversation;
}

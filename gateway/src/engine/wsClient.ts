import { type RawData, WebSocket } from "ws";

import { PushQueue, type CloseReason } from "../lib/pushQueue.js";
import {
  type EngineEvent,
  type EngineEventType,
  isTerminalEvent,
} from "./runtime.js";
import {
  type MessageAttachment,
  PROTOCOL_VERSION,
  type WsClientMessageQuery,
  type WsServerMessageSessionInfo,
} from "./types.gen.js";

/**
 * Typed WebSocket client for the Shannon engine's `/api/ws`.
 *
 * One socket, one query at a time. The engine processes turns sequentially on
 * a connection, and the gateway's session router (P1-c) gives each session its
 * own client — so per-socket serialization maps cleanly onto per-session
 * serialization. Mid-stream `cancel()` and out-of-band `approval_request`
 * frames are delivered through the same event stream.
 *
 * Lifecycle: `connect()` → `runQuery()` (zero or more, sequential) → `close()`.
 *
 * Uses the `ws` Node EventEmitter API (`.on`), not the browser `addEventListener`
 * shape — the error callback receives an `Error`, not an `ErrorEvent`.
 */

export interface EngineWsClientOptions {
  /** Full WebSocket URL, e.g. `ws://127.0.0.1:33420/api/ws`. */
  url: string;
  /** Default model for queries that don't override it. */
  model?: string | null;
  /** Default session id (UUID string) for conversation continuity. */
  sessionId?: string | null;
  /** Extra handshake headers (e.g. `authorization` for the engine bearer). */
  headers?: Record<string, string>;
  /**
   * review §P2-23: how long to wait for the WS handshake (TCP+upgrade)
   * before giving up, in ms. A hung engine accept must not wedge `connect()`
   * forever — the rejection surfaces as a normal error so the caller's
   * reconnect/backoff logic takes over. Default 10s.
   */
  handshakeTimeoutMs?: number;
}

/** Default WS handshake budget (review §P2-23). */
const DEFAULT_HANDSHAKE_TIMEOUT_MS = 10_000;

const KNOWN_EVENT_TYPES: ReadonlySet<EngineEventType> = new Set([
  "text",
  "thinking",
  "tool_use",
  "tool_result",
  "usage",
  "completed",
  "failed",
  "cancelled",
  "approval_request",
  "session_info",
  "error",
]);

/**
 * Parse one wire frame. Returns `null` for anything that isn't a recognized
 * engine event. The engine is the only sender and its types are fixed, so a
 * shallow `type` check is sufficient; add a hardening layer only if an
 * untrusted sender ever shares the socket.
 */
function parseEngineEvent(raw: unknown): EngineEvent | null {
  if (typeof raw !== "object" || raw === null) return null;
  const type = (raw as { type?: unknown }).type;
  if (
    typeof type !== "string" ||
    !KNOWN_EVENT_TYPES.has(type as EngineEventType)
  ) {
    return null;
  }
  return raw as EngineEvent;
}

/** Decode a `ws` RawData payload to parsed JSON. Returns null if unrecognized. */
function parseFrame(data: RawData): unknown {
  let text: string;
  if (typeof data === "string") {
    text = data;
  } else if (Buffer.isBuffer(data)) {
    text = data.toString("utf8");
  } else if (data instanceof ArrayBuffer) {
    text = new TextDecoder().decode(data);
  } else if (Array.isArray(data)) {
    // Buffer[] — fragmented assembly.
    text = Buffer.concat(data).toString("utf8");
  } else if (ArrayBuffer.isView(data)) {
    text = new TextDecoder().decode(data as Uint8Array);
  } else {
    return null;
  }
  return JSON.parse(text);
}

/** Leading major component of a semver-ish string (`"0.8.0"` → `0`). */
function majorVersionOf(version: string): number {
  const major = Number.parseInt(version.split(".", 1)[0] ?? "", 10);
  return Number.isNaN(major) ? -1 : major;
}

export class EngineWsClient {
  private socket: WebSocket | null = null;
  private activeQueue: PushQueue<EngineEvent> | null = null;
  private engineProtocolVersion: string | null = null;
  private versionObserved = false;
  private readonly url: string;
  private readonly defaultModel: string | null;
  private readonly defaultSessionId: string | null;
  private readonly headers: Record<string, string>;
  private readonly handshakeTimeoutMs: number;

  constructor(options: EngineWsClientOptions) {
    this.url = options.url;
    this.defaultModel = options.model ?? null;
    this.defaultSessionId = options.sessionId ?? null;
    this.headers = options.headers ?? {};
    this.handshakeTimeoutMs =
      options.handshakeTimeoutMs ?? DEFAULT_HANDSHAKE_TIMEOUT_MS;
  }

  get isConnected(): boolean {
    return this.socket !== null && this.socket.readyState === WebSocket.OPEN;
  }

  /**
   * review §P2-24: the wire protocol version reported by the engine's
   * greeting frame (`WsServerMessage::SessionInfo.protocol_version`), or
   * `null` until a greeting arrived / when talking to a pre-versioning
   * engine that omits the field.
   */
  get protocolVersion(): string | null {
    return this.engineProtocolVersion;
  }

  /** Open the socket and wait for it to be ready. Idempotent. */
  async connect(): Promise<void> {
    if (this.socket) return;
    const socket =
      Object.keys(this.headers).length > 0
        ? new WebSocket(this.url, { headers: this.headers })
        : new WebSocket(this.url);
    // Attach frame routing BEFORE the handshake wait resolves: the engine
    // sends its greeting as soon as it accepts the connection, so the frame
    // can land between the upgrade completing and the `open` event's
    // continuation running — an EventEmitter silently drops it (review
    // §P2-24). Until the socket is stored below, the handlers are harmless
    // no-ops (`activeQueue` is null, `socket` stays null).
    socket.on("message", (data) => this.onMessage(data));
    socket.on("close", () => this.onSocketClosed());
    socket.on("error", (err) => this.onSocketError(err));
    try {
      await waitForOpen(socket, this.handshakeTimeoutMs);
    } catch (err) {
      // Don't leak a half-open socket; the caller's reconnect path retries
      // with a fresh one. ws emits "error" if the socket is torn down while
      // CONNECTING — swallow it (the original failure is what propagates).
      socket.on("error", () => {});
      socket.terminate();
      throw err;
    }
    this.socket = socket;
  }

  /**
   * Send a query and yield the resulting event stream until a terminal frame
   * (completed / failed / cancelled / error). Throws if the socket isn't open
   * or another query is already in flight on this client.
   */
  async *runQuery(
    prompt: string,
    opts: {
      model?: string | null;
      sessionId?: string | null;
      /** B4: multimodal attachments (engine validates MIME/size). */
      attachments?: MessageAttachment[];
    } = {},
  ): AsyncGenerator<EngineEvent> {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) {
      throw new Error("EngineWsClient not connected; call connect() first");
    }
    if (this.activeQueue) {
      throw new Error("a query is already in flight on this client");
    }

    const req: WsClientMessageQuery = {
      type: "query",
      prompt,
      model: opts.model ?? this.defaultModel,
      session_id: opts.sessionId ?? this.defaultSessionId,
      // Omit on the wire when the turn is text-only (server treats absent
      // and empty identically via serde(default)).
      ...(opts.attachments && opts.attachments.length > 0
        ? { attachments: opts.attachments }
        : {}),
    };

    const queue = new PushQueue<EngineEvent>();
    this.activeQueue = queue;
    socket.send(JSON.stringify(req));

    try {
      for await (const ev of queue) {
        yield ev;
        if (isTerminalEvent(ev)) break;
      }
    } finally {
      this.activeQueue = null;
    }
  }

  /** Interrupt the in-progress query. No-op if the socket isn't open. */
  cancel(): void {
    const socket = this.socket;
    if (!socket || socket.readyState !== WebSocket.OPEN) return;
    socket.send(JSON.stringify({ type: "cancel" }));
  }

  /** Close the socket. Resolves once the underlying socket has closed. */
  async close(): Promise<void> {
    const socket = this.socket;
    if (!socket) return;
    // End any in-flight consumer cleanly so its `for await` stops.
    this.activeQueue?.close({ kind: "done" } satisfies CloseReason);
    this.activeQueue = null;
    await closeSocket(socket);
    this.socket = null;
  }

  // ── frame routing ──────────────────────────────────────────────────

  private onMessage(data: RawData): void {
    const parsed = parseEngineEvent(parseFrame(data));
    if (!parsed) return; // unknown / malformed — ignore for now
    // review §P2-24: the greeting (unsolicited `session_info`) used to be
    // dropped here because no query consumer is active yet. Consume its
    // protocol version first; a `session_info` that arrives mid-query (a
    // response to an `info` frame) is still routed to the consumer below.
    if (parsed.type === "session_info") {
      this.observeProtocolVersion(parsed);
    }
    const queue = this.activeQueue;
    if (!queue) return; // no active consumer (e.g. unsolicited session_info)
    queue.push(parsed);
    if (isTerminalEvent(parsed)) {
      queue.close({ kind: "done" } satisfies CloseReason);
    }
  }

  /**
   * Capture the engine's protocol version from a `session_info` frame, log
   * it once, and warn on a *major*-version mismatch. Policy (§P2-24): the
   * mismatch is a warning, never a hard failure — minor-cycle differences
   * are additive by contract.
   */
  private observeProtocolVersion(frame: WsServerMessageSessionInfo): void {
    const version = frame.protocol_version ?? null;
    // Announce each distinct observation once — a mid-query `session_info`
    // echoing the same version must not re-log.
    if (this.versionObserved && version === this.engineProtocolVersion) return;
    this.versionObserved = true;
    this.engineProtocolVersion = version;
    if (!version) {
      console.warn(
        `[engine-ws] engine greeting carries no protocol_version (pre-versioning engine, gateway speaks ${PROTOCOL_VERSION}); continuing`,
      );
      return;
    }
    console.info(
      `[engine-ws] engine protocol version ${version} (gateway ${PROTOCOL_VERSION})`,
    );
    if (majorVersionOf(version) !== majorVersionOf(PROTOCOL_VERSION)) {
      console.warn(
        `[engine-ws] engine protocol major version mismatch: engine ${version} vs gateway ${PROTOCOL_VERSION}; continuing, wire compatibility is not guaranteed`,
      );
    }
  }

  private onSocketError(err: Error): void {
    this.activeQueue?.close({
      kind: "error",
      error: new Error(`engine socket error: ${err.message}`),
    } satisfies CloseReason);
  }

  private onSocketClosed(): void {
    // An unexpected close mid-stream surfaces as an error so a truncated turn
    // isn't silently swallowed. After a normal terminal frame the queue is
    // already cleared, so this is a no-op.
    this.activeQueue?.close({
      kind: "error",
      error: new Error("engine socket closed before terminal event"),
    } satisfies CloseReason);
    this.socket = null;
  }
}

function waitForOpen(socket: WebSocket, timeoutMs: number): Promise<void> {
  if (socket.readyState === WebSocket.OPEN) return Promise.resolve();
  return new Promise<void>((resolve, reject) => {
    // review §P2-23: bound the handshake so a wedged engine accept can't
    // hang `connect()` forever. On timeout the socket is terminated (by the
    // caller) and the rejection flows into the normal reconnect path.
    const timer = setTimeout(() => {
      cleanup();
      reject(
        new Error(`engine WS handshake timed out after ${timeoutMs}ms`),
      );
    }, timeoutMs);
    timer.unref?.();
    const onOpen = (): void => {
      cleanup();
      resolve();
    };
    const onError = (err: Error): void => {
      cleanup();
      reject(err);
    };
    const cleanup = (): void => {
      clearTimeout(timer);
      socket.off("open", onOpen);
      socket.off("error", onError);
    };
    socket.on("open", onOpen);
    socket.on("error", onError);
  });
}

function closeSocket(socket: WebSocket): Promise<void> {
  if (socket.readyState === WebSocket.CLOSED) return Promise.resolve();
  return new Promise<void>((resolve) => {
    const onClose = (): void => {
      socket.off("close", onClose);
      resolve();
    };
    socket.on("close", onClose);
    socket.close();
  });
}

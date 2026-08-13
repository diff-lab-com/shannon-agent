/**
 * Relay host transport — connects the gateway OUTBOUND to shannon-relay so the
 * phone can pair through the relay broker instead of requiring LAN access.
 *
 * Flow:
 *  1. Gateway connects WS to the relay as a "host" (registers with a session ID)
 *  2. Phone connects to the same relay and "joins" the same session ID
 *  3. The relay pairs them and forwards opaque binary frames between them
 *  4. Binary frames are E2E-encrypted (AES-256-GCM via E2eChannel)
 *  5. Inside the E2E channel, the same Shannon JSON-RPC protocol flows
 *
 * The dispatch is shared with MobileServer via `dispatchNdjson` — the relay host
 * provides a `send` callback that seals the plaintext with E2E before sending as
 * binary, and feeds incoming E2E-decrypted binary through `dispatchNdjson`.
 */

import { EventEmitter } from "node:events";
import WebSocket from "ws";

import type { Logger } from "../../adapters/types.js";
import { dispatchNdjson } from "../dispatch.js";
import type { MethodContext, MethodHandlers } from "../server.js";
import { E2eChannel } from "./e2e.js";

export interface RelayHostOptions {
  /** Relay WebSocket URL (e.g. "wss://relay.shannon.example"). */
  relayUrl: string;
  /** Session ID to register with the relay. */
  sid: string;
  /** 32-byte E2E session key (derived from the pair token via HKDF). */
  sessionKey: Buffer;
  /** Shannon JSON-RPC method handlers (shared with MobileServer). */
  handlers: MethodHandlers;
  logger: Logger;
  /** ms to wait for phone to join (default 75_000). */
  pairTimeout?: number;
}

export interface RelayHostHandle {
  /** Resolves when the phone joins and the session is active. */
  readonly paired: Promise<void>;
  /** Stop the relay host connection. */
  stop(): Promise<void>;
}

/**
 * A virtual socket that satisfies the `MethodContext.socket` contract but routes
 * through the E2E channel. Handlers (e.g. the query stream loop) check
 * `readyState === WebSocket.OPEN` so we emulate that constant here.
 */
class VirtualSocket extends EventEmitter {
  /** Mirror ws.WebSocket.OPEN so handler code using `ctx.socket.readyState` works. */
  static readonly OPEN = WebSocket.OPEN;
  readonly readyState = WebSocket.OPEN;

  constructor(private readonly sendFn: (data: Buffer) => void) {
    super();
  }

  send(data: string): void {
    this.sendFn(Buffer.from(data, "utf8"));
  }

  close(): void {
    // The relay host manages the underlying WS lifecycle; this is a no-op stub
    // so handler code that calls ctx.socket.close() during streaming doesn't crash.
  }
}

const PAIR_TIMEOUT_MS = 75_000;

/**
 * Start a relay host: connect to the relay, register, and wait for a phone to
 * join. Once paired, the host E2E-decrypts incoming binary frames, dispatches
 * them as NDJSON, and E2E-encrypts outgoing responses.
 */
export function startRelayHost(opts: RelayHostOptions): RelayHostHandle {
  const pairTimeout = opts.pairTimeout ?? PAIR_TIMEOUT_MS;
  const logger = opts.logger;

  let stopped = false;
  let ws: WebSocket | null = null;

  // Separate E2E channels for send/recv with independent counters.
  // The send channel seals our outgoing messages; the recv channel opens
  // the phone's incoming messages. Both share the same key.
  let sendChannel: E2eChannel | null = null;
  let recvChannel: E2eChannel | null = null;
  let virtualSocket: VirtualSocket | null = null;
  // One context per relay session — mutated by shannon/pair (sets sessionId)
  // and reused for all subsequent binary frames on that session, mirroring
  // MobileServer's per-connection context.
  let sessionCtx: MethodContext | null = null;

  let pairResolve: (() => void) | null = null;
  let pairReject: ((err: Error) => void) | null = null;
  const paired: Promise<void> = new Promise((resolve, reject) => {
    pairResolve = resolve;
    pairReject = reject;
  });

  // Pair timeout — if no phone joins within the window, reject.
  const pairTimer = setTimeout(() => {
    if (!stopped && pairReject) {
      pairReject(new Error(`relay host: pair timeout after ${pairTimeout}ms`));
    }
  }, pairTimeout);

  function clearPairTimer(): void {
    clearTimeout(pairTimer);
  }

  function connect(): void {
    ws = new WebSocket(opts.relayUrl);

    ws.on("open", () => {
      logger.info(`relay host: connected to ${opts.relayUrl}, registering sid=${opts.sid}`);
      ws!.send(JSON.stringify({ t: "register", role: "host", sid: opts.sid }));
    });

    ws.on("message", (data, isBinary) => {
      if (stopped) return;

      if (isBinary) {
        handleBinaryFrame(data);
        return;
      }

      // Text = control frame from the relay
      let ctrl: Record<string, unknown>;
      try {
        ctrl = JSON.parse(String(data));
      } catch {
        logger.warn("relay host: received unparseable control frame");
        return;
      }
      handleControlFrame(ctrl);
    });

    ws.on("error", (err) => {
      logger.warn(`relay host: WS error: ${(err as Error).message}`);
      if (pairReject && !ws) {
        pairReject(err as Error);
      }
    });

    ws.on("close", () => {
      logger.info("relay host: WS closed");
      clearPairTimer();
    });
  }

  function handleControlFrame(ctrl: Record<string, unknown>): void {
    const type = ctrl["t"] as string | undefined;
    switch (type) {
      case "host_ready":
        logger.info("relay host: registered, waiting for phone to join");
        break;

      case "paired": {
        clearPairTimer();
        logger.info("relay host: phone joined — E2E session active");
        // Create fresh E2E channels (new phone or reconnect — counters restart).
        sendChannel = new E2eChannel(opts.sessionKey);
        recvChannel = new E2eChannel(opts.sessionKey);
        // The virtual socket routes sends through the send channel.
        virtualSocket = new VirtualSocket((plaintext) => {
          if (!sendChannel || !ws || ws.readyState !== WebSocket.OPEN) return;
          const frame = sendChannel.seal(plaintext);
          ws.send(frame);
        });
        // Fresh context — shannon/pair will set sessionId, and it persists
        // for all subsequent messages on this relay session.
        sessionCtx = {
          socket: virtualSocket as unknown as WebSocket,
          sessionId: null,
          logger,
        };
        if (pairResolve) pairResolve();
        break;
      }

      case "peer_gone":
        logger.info("relay host: phone disconnected, waiting for re-pair");
        // Reset recv counter so a reconnecting phone (counter starting at 1)
        // isn't rejected by replay protection. The send channel is also reset
        // and will be recreated on the next "paired" event.
        recvChannel = null;
        virtualSocket = null;
        sessionCtx = null;
        break;

      case "error": {
        const msg = (ctrl["message"] as string) ?? "relay error";
        logger.error(`relay host: relay error: ${msg}`);
        if (pairReject) pairReject(new Error(`relay error: ${msg}`));
        break;
      }

      default:
        logger.debug(`relay host: unknown control frame type: ${type}`);
    }
  }

  function handleBinaryFrame(data: unknown): void {
    if (!recvChannel || !sessionCtx) {
      logger.warn("relay host: binary frame before paired, ignoring");
      return;
    }
    const buf = toBuffer(data);
    let plaintext: Buffer;
    try {
      plaintext = recvChannel.open(buf);
    } catch (err) {
      logger.warn(`relay host: E2E open failed: ${(err as Error).message}`);
      return;
    }

    void dispatchNdjson(
      plaintext.toString("utf8"),
      sessionCtx,
      opts.handlers,
      (text) => {
        if (!sendChannel || !ws || ws.readyState !== WebSocket.OPEN) return;
        const frame = sendChannel.seal(Buffer.from(text, "utf8"));
        ws.send(frame);
      },
      logger,
    );
  }

  async function stop(): Promise<void> {
    if (stopped) return;
    stopped = true;
    clearPairTimer();
    if (ws) {
      ws.close();
      ws = null;
    }
  }

  connect();

  return {
    get paired(): Promise<void> {
      return paired;
    },
    stop,
  };
}

// ── helpers ──────────────────────────────────────────────────────────────────

function toBuffer(data: unknown): Buffer {
  if (Buffer.isBuffer(data)) return data;
  if (data instanceof ArrayBuffer) return Buffer.from(data);
  if (Array.isArray(data)) return Buffer.concat(data as Buffer[]);
  if (ArrayBuffer.isView(data)) return Buffer.from(data as Uint8Array);
  return Buffer.from(data as Uint8Array);
}

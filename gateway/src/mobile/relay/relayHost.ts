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
import type { KeyObject } from "node:crypto";

import type { Logger } from "../../adapters/types.js";
import { dispatchNdjson } from "../dispatch.js";
import type { MethodContext, MethodHandlers } from "../server.js";
import {
  E2eChannel,
  deriveSessionKeyV2,
  hostSharedSecret,
  isE2eHello,
} from "./e2e.js";

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
  /**
   * Auto-reconnect when the relay WS drops (default true). Re-registers with
   * the same sid after an exponential backoff (1s doubling to a 30s cap), so
   * a relay restart doesn't leave the gateway remote-unreachable until the
   * next gateway restart. Phones re-join and re-`device.resume` as usual.
   */
  reconnect?: boolean;
  /**
   * Relay handshake auth tag (wire-protocol.md) — sent with `register` so the
   * relay can pin it and reject sid-only squatters. Derived from the pair
   * token (`deriveRelayAuthTag`); the phone presents the same value on join.
   */
  relayAuthTag?: string;
  /**
   * v0.3 E2E handshake (forward secrecy): the host's per-session ephemeral
   * X25519 private key + the pair token. When set, the phone's first binary
   * frame must be a plaintext `e2e_hello` carrying its ephemeral pubkey; both
   * sides derive the v2 session key (token-mixed ECDH). A first frame that is
   * NOT a hello falls back to the legacy token-derived `sessionKey` so older
   * phones keep working.
   */
  hostE2E?: { privateKey: KeyObject; pairToken: string };
  /**
   * Invoked when the phone pairs and the session's MethodContext is created
   * (parity with MobileServer's `onContext`). The dispatch hub uses it to
   * register the connection so pushes can reach the phone over the relay.
   */
  onContext?: (ctx: MethodContext) => void;
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
  const reconnectEnabled = opts.reconnect !== false;
  const logger = opts.logger;

  let stopped = false;
  let ws: WebSocket | null = null;
  let reconnectAttempts = 0;
  let reconnectTimer: NodeJS.Timeout | null = null;
  // v0.3 ECDH mode: channels stay null until the phone's `e2e_hello` arrives
  // (or a legacy sealed frame triggers the fallback). Once established, both
  // channels follow the §G-rev2/§G-rev3 re-pair rules (recv reset, send kept).
  const ecdh = opts.hostE2E ?? null;
  let e2eEstablished = false;
  /** The established session key (v2 when ECDH, legacy on fallback). */
  let e2eKey: Buffer | null = null;

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

  /** Exponential backoff (1s doubling, 30s cap) until stopped. */
  function scheduleReconnect(): void {
    if (stopped || !reconnectEnabled || reconnectTimer) return;
    const delay = Math.min(1000 * 2 ** reconnectAttempts, 30_000);
    reconnectAttempts += 1;
    logger.info(`relay host: reconnecting in ${delay}ms (attempt ${reconnectAttempts})`);
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      if (!stopped) connect();
    }, delay);
  }

  function connect(): void {
    ws = new WebSocket(opts.relayUrl);

    ws.on("open", () => {
      logger.info(`relay host: connected to ${opts.relayUrl}, registering sid=${opts.sid}`);
      // The auth tag lets the relay pin the session to the pairing secret —
      // a sid-only knower can't squat the host slot once this is pinned.
      ws!.send(
        JSON.stringify({
          t: "register",
          role: "host",
          sid: opts.sid,
          ...(opts.relayAuthTag ? { tag: opts.relayAuthTag } : {}),
        }),
      );
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
      if (!stopped) {
        // Relay restart / network drop: re-register the same sid so the phone
        // can re-join without a gateway restart (OPERATIONS.md §5 says every
        // client reconnects — that must hold for the host leg too).
        scheduleReconnect();
      }
    });
  }

  function handleControlFrame(ctrl: Record<string, unknown>): void {
    const type = ctrl["t"] as string | undefined;
    switch (type) {
      case "host_ready":
        logger.info("relay host: registered, waiting for phone to join");
        // Registration (re)succeeded — the backoff did its job.
        reconnectAttempts = 0;
        break;

      case "paired": {
        clearPairTimer();
        logger.info("relay host: phone joined — E2E session active");
        if (ecdh && !e2eEstablished) {
          // v2 handshake: wait for the phone's plaintext `e2e_hello` first
          // frame — both channels are created from the derived v2 key then.
          recvChannel = null;
          sendChannel = null;
          virtualSocket = null;
          sessionCtx = null;
          break;
        }
        // Re-pair with an established session (§G-rev2/§G-rev3): recv fresh
        // (a reconnecting phone restarts at counter 1), send PRESERVED (its
        // counter must stay monotonic — the phone's recv is parked at k).
        recvChannel = new E2eChannel(e2eKey ?? opts.sessionKey);
        if (!sendChannel) sendChannel = new E2eChannel(e2eKey ?? opts.sessionKey);
        attachVirtualSocket();
        if (pairResolve) {
          pairResolve();
          pairResolve = null;
        }
        break;
      }

      case "peer_gone":
        logger.info("relay host: phone disconnected, waiting for re-pair");
        // Reset recv so a reconnecting phone (counter starting at 1) isn't
        // rejected by replay protection. The send channel is intentionally
        // KEPT (§G-rev3): our socket never dropped, so its counter stays
        // monotonic and is reused as-is on the next "paired" event.
        recvChannel = null;
        virtualSocket = null;
        sessionCtx = null;
        break;

      case "error": {
        // Relay sends `{t:"error", code}` (shannon-relay proto.rs); read `code`
        // first, fall back to `message` for tolerant interop.
        const detail =
          (ctrl["code"] as string | undefined) ??
          (ctrl["message"] as string | undefined) ??
          "relay error";
        logger.error(`relay host: relay error: ${detail}`);
        if (pairReject) pairReject(new Error(`relay error: ${detail}`));
        break;
      }

      default:
        logger.debug(`relay host: unknown control frame type: ${type}`);
    }
  }

  /** Legacy (token-derived) channel setup — §G-rev2/§G-rev3 re-pair rules. */
  function establishLegacyChannels(): void {
    // Recv: fresh channel on every pair (§G-rev2 defense in depth) — a
    // reconnecting phone that lost its counter state restarts at 1 and
    // must still be accepted.
    recvChannel = new E2eChannel(opts.sessionKey);
    // Send: create once, PRESERVE across re-paired (§G-rev3) — this host
    // socket never dropped, so its send counter must stay monotonic; a
    // reconnecting phone's recv is parked at k and would reject frames
    // whose counter restarts at 1 (replay check), wedging host→phone.
    // Host process restart is unaffected: the relay treats the fresh
    // register as host replacement and the phone resets recv then.
    if (!sendChannel) sendChannel = new E2eChannel(opts.sessionKey);
    attachVirtualSocket();
    e2eEstablished = true;
    if (pairResolve) {
      pairResolve();
      pairResolve = null;
    }
  }

  /** Wire the virtual socket to the (current) send channel + fresh context. */
  function attachVirtualSocket(): void {
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
    opts.onContext?.(sessionCtx);
  }

  function handleBinaryFrame(data: unknown): void {
    if (!sessionCtx || !recvChannel || !sendChannel) {
      if (ecdh && !e2eEstablished) {
        const buf = toBuffer(data);
        const hello = isE2eHello(buf);
        if (hello) {
          // v2: derive the token-mixed ECDH session key and bring both
          // channels up. The hello itself carries no payload — consume it.
          const shared = hostSharedSecret(ecdh.privateKey, hello.pub);
          const k = deriveSessionKeyV2(ecdh.pairToken, shared);
          recvChannel = new E2eChannel(k);
          sendChannel = new E2eChannel(k);
          attachVirtualSocket();
          e2eEstablished = true;
          logger.info("relay host: e2e v2 handshake complete (X25519 + token)");
          if (pairResolve) {
            pairResolve();
            pairResolve = null;
          }
          return;
        }
        // Not a hello → a legacy phone sealed this with the token-derived
        // key; fall back so older clients keep working.
        logger.info("relay host: first frame is not e2e_hello — legacy token key");
        establishLegacyChannels();
        // fall through: process this frame through the legacy channels.
      } else {
        logger.warn("relay host: binary frame before paired, ignoring");
        return;
      }
    }
    const sealed = toBuffer(data);
    let plaintext: Buffer;
    try {
      plaintext = recvChannel!.open(sealed);
    } catch (err) {
      logger.warn(`relay host: E2E open failed: ${(err as Error).message}`);
      return;
    }

    void dispatchNdjson(
      plaintext.toString("utf8"),
      sessionCtx!,
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
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
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

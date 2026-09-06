import type { IncomingMessage } from "node:http";
import { createServer, type RequestListener, type Server } from "node:http";
import { WebSocket, WebSocketServer } from "ws";

import type { Logger } from "../adapters/types.js";
import type { ShannonEvent } from "./protocol.js";
import { dispatchNdjson } from "./dispatch.js";
import { MOBILE_PAGE_HTML } from "./web/page.js";

/**
 * The inbound mobile server — a WebSocket endpoint speaking NDJSON `shannon/*`
 * JSON-RPC. Unlike the 8 platform `ChannelAdapter`s (outbound chat clients with
 * a reply-target model), this is a *server* the phone dials, and the phone is a
 * first-class streaming client: it receives the full engine event stream as
 * `shannon/event` notifications (architecture doc §5.2, Option B).
 *
 * The server owns only transport + per-connection dispatch. Every method's
 * behavior is injected via `MethodHandlers` so:
 *  - P1.1a ships routing + handshake with mock handlers,
 *  - P1.1b wires the real engine bridge (query/cancel/approval/health/model),
 *  - tests inject fakes — no real socket or engine needed for unit coverage.
 *
 * Auth is an optional `authenticator` hook; P1.1 leaves it unset and P1.2
 * injects Ed25519 verification (every approval decision is signed).
 *
 * Dispatch logic is shared with the relay host transport via `dispatchNdjson`
 * (see dispatch.ts) so both transports — direct WebSocket and E2E relay — route
 * messages through the same JSON-RPC pipeline.
 *
 * P2-1: the same port also serves the built-in PWA page (GET /) so a phone
 * browser can act as the client — no native app required. The page speaks the
 * identical `shannon/*` protocol over this WebSocket. `onContext` hands every
 * accepted connection's `MethodContext` to the dispatch hub (P2-1) so paired
 * devices can receive gateway-side pushes.
 */

export interface MethodContext {
  /** The underlying socket (handlers may close it, e.g. on auth failure). */
  readonly socket: WebSocket;
  /**
   * Device session id once paired. `null` in P1.1 (no pairing yet); P1.2 sets
   * it after `shannon/pair` so subsequent methods bind to the device session.
   */
  sessionId: string | null;
  readonly logger: Logger;
  /**
   * P2-1: set by the dispatch hub via `registerConnection`. Called by the
   * pairing handlers when `shannon/pair` / `shannon/device.resume` binds (or
   * re-binds) this connection to a device, so pushes can reach it.
   */
  onSessionBound?: (deviceId: string) => void;
}

/** Discriminated handler outcome — unambiguous vs. duck-typing the result. */
export type HandlerOutcome =
  | { kind: "result"; result: unknown }
  | { kind: "stream"; stream: AsyncIterable<ShannonEvent>; result: unknown }
  | { kind: "error"; code: number; message: string; data?: unknown };

export type MethodHandler = (params: unknown, ctx: MethodContext) => Promise<HandlerOutcome> | HandlerOutcome;

export interface MethodHandlers {
  [method: string]: MethodHandler;
}

export interface AuthenticatorContext {
  socket: WebSocket;
  req: IncomingMessage;
}

export interface MobileServerOptions {
  host: string;
  port: number;
  logger: Logger;
  handlers: MethodHandlers;
  /** WS path (default "/"). */
  path?: string;
  /**
   * Optional connection gate. P1.1 leaves this unset (open for testing); P1.2
   * injects Ed25519 device verification. Returning `false` closes the socket
   * with code 4001 and dispatches no methods.
   */
  authenticator?: (ctx: AuthenticatorContext) => boolean | Promise<boolean>;
  /**
   * P2-1: notified for every accepted connection (after the authenticator).
   * The dispatch hub uses this to track sockets so it can push events to
   * paired devices. The returned detach function (if any) is called on stop.
   */
  onContext?: (ctx: MethodContext) => void | (() => void);
  /**
   * P2-1: serve the built-in PWA page on GET / (default true). Set false in
   * tests that want the old bare-WS behavior.
   */
  servePage?: boolean;
}

export interface MobileServerHandle {
  /** The bound port (useful when `port: 0` was requested for a free port). */
  readonly port: number;
  stop(): Promise<void>;
}

export class MobileServer {
  private wss: WebSocketServer | null = null;
  private httpServer: Server | null = null;
  private readonly detachers: Array<() => void> = [];
  private readonly opts: MobileServerOptions;

  constructor(opts: MobileServerOptions) {
    this.opts = opts;
  }

  /** Bind and wait for the listener. */
  async start(): Promise<MobileServerHandle> {
    // P2-1: serve the built-in PWA page on plain GETs; WS upgrades are routed
    // by the WebSocketServer attached to the same HTTP server.
    const servePage = this.opts.servePage !== false;
    const requestListener: RequestListener = (req, res) => {
      if (servePage && req.method === "GET" && (req.url ?? "/").split("?")[0] === "/") {
        res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
        res.end(MOBILE_PAGE_HTML);
        return;
      }
      res.writeHead(404, { "content-type": "text/plain; charset=utf-8" });
      res.end("not found");
    };
    const httpServer = createServer(requestListener);
    this.httpServer = httpServer;
    const wss = new WebSocketServer({ server: httpServer, path: this.opts.path ?? "/" });
    this.wss = wss;
    await new Promise<void>((resolve, reject) => {
      httpServer.once("listening", resolve);
      httpServer.once("error", reject);
      httpServer.listen(this.opts.port, this.opts.host);
    });
    const address = httpServer.address();
    const boundPort =
      this.opts.port === 0 && address && typeof address === "object" ? address.port : this.opts.port;

    wss.on("connection", (socket, req) => {
      void this.onConnection(socket, req);
    });

    this.opts.logger.info(
      `mobile server listening on ${this.opts.host}:${boundPort}${this.opts.path ?? "/"}`,
    );
    return {
      get port() {
        return boundPort;
      },
      stop: () => this.stop(),
    };
  }

  async stop(): Promise<void> {
    while (this.detachers.length > 0) this.detachers.pop()?.();
    // Terminate live clients so close() resolves promptly (the page may keep
    // the socket open indefinitely).
    for (const client of this.wss?.clients ?? []) client.terminate();
    this.wss?.close();
    this.wss = null;
    const httpServer = this.httpServer;
    this.httpServer = null;
    if (!httpServer) return;
    await new Promise<void>((resolve) => httpServer.close(() => resolve()));
  }

  // ── connection lifecycle ──────────────────────────────────────────────

  private async onConnection(socket: WebSocket, req: IncomingMessage): Promise<void> {
    if (this.opts.authenticator) {
      let ok = false;
      try {
        ok = await this.opts.authenticator({ socket, req });
      } catch {
        ok = false;
      }
      if (!ok) {
        socket.close(4001, "unauthorized");
        return;
      }
    }
    const ctx: MethodContext = { socket, sessionId: null, logger: this.opts.logger };
    const detach = this.opts.onContext?.(ctx);
    if (typeof detach === "function") this.detachers.push(detach);

    socket.on("message", (data) => {
      const text = frameToString(data);
      void this.onMessage(text, ctx);
    });
    socket.on("error", (err) =>
      this.opts.logger.warn(`mobile socket error: ${(err as Error).message}`),
    );
  }

  private async onMessage(text: string, ctx: MethodContext): Promise<void> {
    await dispatchNdjson(
      text,
      ctx,
      this.opts.handlers,
      (data) => this.send(ctx, data),
      this.opts.logger,
    );
  }

  private send(ctx: MethodContext, data: string): void {
    if (ctx.socket.readyState !== WebSocket.OPEN) return;
    ctx.socket.send(data);
  }
}

// ── helpers ──────────────────────────────────────────────────────────────────

function frameToString(data: unknown): string {
  if (typeof data === "string") return data;
  if (Buffer.isBuffer(data)) return data.toString("utf8");
  if (data instanceof ArrayBuffer) return new TextDecoder().decode(data);
  if (Array.isArray(data)) return Buffer.concat(data as Buffer[]).toString("utf8");
  if (ArrayBuffer.isView(data)) return new TextDecoder().decode(data as Uint8Array);
  return String(data);
}

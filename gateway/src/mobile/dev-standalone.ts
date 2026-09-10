/**
 * Dev-only standalone mobile host: boots the REAL gateway mobile server
 * (pairing + engine bridge, `requireSession` enforced) with in-memory pair
 * tokens / device registry, and prints one JSON line
 *
 *   {"host":..,"port":<n>,"token":"..","expiresAt":<epochMs>,"enginePort":<n>}
 *
 * plus a ready-to-paste QR v1 payload, so a client smoke test
 * (shannon-mobile `tool/gateway_smoke.dart`) or an on-device joint-debug
 * session can exercise the live `shannon/*` wire contract without the desktop
 * app.
 *
 * Environment:
 *  - SHANNON_MOBILE_HOST (default 127.0.0.1) — bind host (e.g. 0.0.0.0 for LAN)
 *  - SHANNON_MOBILE_PORT (default 0 = ephemeral)
 *  - SHANNON_QR_HOST (default = bind host) — host written into the QR payload
 *    (an Android emulator dials the host loopback as 10.0.2.2)
 *  - FAKE_ENGINE=1 — also boot an in-process FAKE engine (see below) and point
 *    the bridge at it instead of ws://127.0.0.1:33420
 *
 * The fake engine speaks the `shannon-api-protocol` WS surface (session_info
 * handshake, text/tool/usage events, approval_request → HTTP
 * /api/approval/respond → continue → completed, cancel → cancelled) with a
 * deterministic script: every turn streams two text deltas, then requests an
 * Edit approval, then (allow) applies the patch narratively or (deny) stands
 * down, and completes. It exists ONLY for joint debugging — production never
 * imports this module.
 *
 * Exits on stdin EOF or SIGINT.
 */
import { createServer, type Server } from "node:http";
import { WebSocket, WebSocketServer } from "ws";

import { createConsoleLogger } from "../logger.js";
import { GATEWAY_VERSION } from "../version.js";
import {
  createMobileHandlers,
  DeviceRegistry,
  PairTokenStore,
} from "./pairing.js";
import { MobileServer } from "./server.js";

const bindHost = process.env.SHANNON_MOBILE_HOST ?? "127.0.0.1";
const bindPort = Number(process.env.SHANNON_MOBILE_PORT ?? 0);
const qrHost = process.env.SHANNON_QR_HOST ?? bindHost;
const fakeEngine = process.env.FAKE_ENGINE === "1";

const logger = createConsoleLogger("warn");

// ── fake engine (shannon-api-protocol WS surface) ──────────────────────────

interface PendingApproval {
  resolve: (choice: "allow_once" | "deny") => void;
}

class FakeEngine {
  private httpServer: Server | null = null;
  private wss: WebSocketServer | null = null;
  private readonly pending = new Map<string, PendingApproval>();
  private turnSeq = 0;
  readonly port = 0;

  async start(): Promise<number> {
    const timers = new Set<NodeJS.Timeout>();
    const later = (ms: number, fn: () => void): void => {
      const t = setTimeout(() => {
        timers.delete(t);
        fn();
      }, ms);
      timers.add(t);
    };

    this.httpServer = createServer((req, res) => {
      if (req.method === "GET" && (req.url ?? "").split("?")[0] === "/api/health") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ ok: true, version: GATEWAY_VERSION }));
        return;
      }
      if (req.method === "POST" && (req.url ?? "").split("?")[0] === "/api/approval/respond") {
        let body = "";
        req.on("data", (c) => (body += c));
        req.on("end", () => {
          try {
            const parsed = JSON.parse(body) as { request_id?: string; choice?: string };
            const requestId = parsed.request_id ?? "";
            const pending = this.pending.get(requestId);
            if (pending) {
              this.pending.delete(requestId);
              pending.resolve(parsed.choice === "deny" ? "deny" : "allow_once");
            }
            console.log(`[fake-engine] approval ${parsed.choice} for ${requestId}`);
            res.writeHead(200, { "content-type": "application/json" });
            res.end("{}");
          } catch {
            res.writeHead(400).end("bad json");
          }
        });
        return;
      }
      res.writeHead(404).end("not found");
    });

    await new Promise<void>((resolve, reject) => {
      this.httpServer!.once("listening", resolve);
      this.httpServer!.once("error", reject);
      this.httpServer!.listen(0, "127.0.0.1");
    });
    const port = (this.httpServer.address() as { port: number }).port;

    this.wss = new WebSocketServer({ server: this.httpServer, path: "/api/ws" });
    this.wss.on("connection", (socket) => {
      let cancelled = false;
      const send = (m: unknown): void => {
        if (socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify(m));
      };
      send({
        type: "session_info",
        message_count: 0,
        model: "claude-sonnet-4-6",
        protocol_version: "0.6.0",
      });

      socket.on("message", (data) => {
        let frame: { type?: string; prompt?: string };
        try {
          frame = JSON.parse(String(data));
        } catch {
          return;
        }
        if (frame.type === "cancel") {
          cancelled = true;
          send({ type: "cancelled" });
          return;
        }
        if (frame.type !== "query") return;
        const prompt = frame.prompt ?? "";
        const turn = ++this.turnSeq;
        const requestId = `aprv-dev-${turn}`;
        cancelled = false;

        // Scripted turn: 2 deltas → approval gate → (allow) patch narrative
        // / (deny) stand-down → usage → completed.
        later(120, () => {
          if (cancelled) return;
          send({ type: "text", content: `On it — "${prompt}". ` });
        });
        later(240, () => {
          if (cancelled) return;
          send({ type: "text", content: "Let me apply the edit.\n\n" });
        });
        later(360, () => {
          if (cancelled) return;
          send({
            type: "approval_request",
            request_id: requestId,
            tool_name: "Edit",
            tool_input: { path: "src/main.rs", change: `re: ${prompt}` },
            description: `Apply a patch to src/main.rs (${prompt})`,
            is_destructive: false,
            diff_preview: "- let x = 1;\n+ let x = 2;",
          });
          this.pending.set(requestId, {
            resolve: (choice) => {
              if (cancelled) return;
              if (choice === "allow_once") {
                send({
                  type: "tool_use",
                  name: "Edit",
                  input: { path: "src/main.rs" },
                });
                later(150, () =>
                  send({ type: "tool_result", name: "Edit", output: "patched src/main.rs" }),
                );
                later(320, () => {
                  send({
                    type: "text",
                    content: "Patch applied — 1 file changed.",
                  });
                });
              } else {
                later(150, () => {
                  send({ type: "text", content: "Understood — I left the file untouched." });
                });
              }
              later(500, () => {
                send({ type: "usage", input_tokens: 128, output_tokens: 64, cost_usd: 0.0042 });
              });
              later(560, () => send({ type: "completed", model: "claude-sonnet-4-6" }));
            },
          });
        });
      });
    });

    console.log(`[fake-engine] listening on 127.0.0.1:${port}`);
    return port;
  }

  async stop(): Promise<void> {
    for (const socket of this.wss?.clients ?? []) socket.terminate();
    this.wss?.close();
    await new Promise<void>((resolve) =>
      this.httpServer ? this.httpServer.close(() => resolve()) : resolve(),
    );
    this.httpServer = null;
    this.wss = null;
  }
}

// ── mobile host (the REAL gateway handlers) ────────────────────────────────

let engineWsUrl = "ws://127.0.0.1:33420/api/ws";
let engineHttpBase = "http://127.0.0.1:33420";
let engine: FakeEngine | null = null;
if (fakeEngine) {
  engine = new FakeEngine();
  const enginePort = await engine.start();
  engineWsUrl = `ws://127.0.0.1:${enginePort}/api/ws`;
  engineHttpBase = `http://127.0.0.1:${enginePort}`;
}

const tokens = new PairTokenStore({
  // Joint debugging drives the pairing UI over adb — the production 75s TTL
  // expires before the QR payload makes it onto the emulator. Dev-only knob.
  ttlMs: Number(process.env.SHANNON_TOKEN_TTL_MS ?? 75_000),
});
const registry = new DeviceRegistry();
const handlers = createMobileHandlers({
  engine: {
    engineWsUrl,
    engineHttpBaseUrl: engineHttpBase,
    defaultModel: "claude-sonnet-4-6",
    version: GATEWAY_VERSION,
    logger,
  },
  tokens,
  registry,
  logger,
});

const server = new MobileServer({
  host: bindHost,
  port: bindPort,
  logger,
  handlers,
  servePage: false,
});
const handle = await server.start();
const record = tokens.issue();
const exp = record.expiresAt;
const qr = { v: 1, scheme: "ws", host: qrHost, port: handle.port, token: record.token, exp };
process.stdout.write(
  `${JSON.stringify({
    host: qrHost,
    port: handle.port,
    token: record.token,
    expiresAt: exp,
    enginePort: engineHttpBase,
    qr: JSON.stringify(qr),
  })}\n`,
);

let stopping = false;
const shutdown = (): void => {
  if (stopping) return;
  stopping = true;
  void (async () => {
    await server.stop().catch(() => {});
    await engine?.stop().catch(() => {});
    process.exit(0);
  })();
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
process.stdin.on("end", shutdown);

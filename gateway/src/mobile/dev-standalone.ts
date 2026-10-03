/**
 * Dev-only standalone mobile host: boots the REAL gateway mobile server
 * (pairing + engine bridge, `requireSession` enforced) with in-memory pair
 * tokens / device registry, and prints TWO JSON lines
 *
 *   {"host":..,"port":<n>,"token":"..","expiresAt":<epochMs>,"enginePort":<n>}
 *
 * — line 1 = device A, line 2 = device B (same shape, different tokens, same
 * host/port/expiry; the mobile smoke pairs a second bystander device from
 * line 2 for its "events reach ONLY the initiating device" negative check),
 * plus a ready-to-paste QR v1 payload per line, so a client smoke test
 * (shannon-mobile `tool/gateway_smoke.dart`) or an on-device joint-debug
 * session can exercise the live `shannon/*` wire contract without the desktop
 * app. Line 1 stays byte-compatible with the historical single-line boot
 * record, so consumers that take the first parseable line are unaffected.
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
 * /api/approval/respond → continue → completed, cancel → cancelled, and the
 * §J `sessions.list` / `session.history` one-shots backed by two static fake
 * sessions) with a deterministic script: every turn streams two text deltas,
 * then requests an Edit approval (with §L1 ts/agent/risk rich fields), then
 * (allow) applies the patch narratively or (deny) stands down, and completes.
 * It exists ONLY for joint debugging — production never imports this module.
 *
 * Scope note: the standalone host serves the FULL §K task face (this file
 * wires `createTaskHandlers` + the mobile-only dispatch pipeline from
 * `dispatchPipeline.ts` — hub → SessionRouter lane → §K3 task turn handler →
 * engine — the same components the live bootstrap assembles), so
 * `shannon/task.dispatch` / `shannon/task.list` run the real journal + task
 * stream against the fake engine. What it deliberately does NOT mount is the
 * desktop-pairing `access` surface (`shannon/pairing.pending` / `.approve`):
 * there is no IM allowlist / pairing store to serve here — exercise that
 * against the real gateway.
 *
 * Exits on stdin EOF or SIGINT.
 */
import { createServer, type Server } from "node:http";
import { WebSocket, WebSocketServer } from "ws";

import { createConsoleLogger } from "../logger.js";
import { GATEWAY_VERSION } from "../version.js";
import { ActiveQueryRegistry } from "../router/activeQueries.js";
import {
  createMobileHandlers,
  DeviceRegistry,
  PairTokenStore,
} from "./pairing.js";
import { MobileServer } from "./server.js";
import { ApprovalRegistry } from "./approvalRegistry.js";
import { createMobileDispatchPipeline } from "./dispatchPipeline.js";
import { MobileDispatchHub } from "./hub.js";
import { createTaskHandlers } from "./taskHandlers.js";

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

  // §J dev data: two fake sessions with transcripts so the phone's Chat list
  // seeding (sessions.list) and thread fills (session.history) have something
  // honest to render during joint debugging. Static — the fake engine's turns
  // don't write back into them (it exists only to exercise the wire).
  // Token totals (C8): sess-dev-0001 carries real positive ints so the smoke
  // sees the session.list token keys end to end; sess-dev-0002 deliberately
  // omits them — "engine has no data → wire omits the keys" is the degradation
  // path the phone must render honestly (never invent 0).
  private readonly sessions = [
    {
      session_id: "sess-dev-0001",
      agent_id: null,
      title: "Fix flaky login tests",
      updated_at: new Date(Date.now() - 3 * 60_000).toISOString(),
      total_input_tokens: 1834,
      total_output_tokens: 902,
      transcript: [
        { role: "user", content: "The login tests fail about one in five runs — investigate." },
        {
          role: "assistant",
          content:
            "Found it: the token refresh test asserts on wall-clock time. I made the clock injectable and pinned it — suite is green across 50 runs.",
        },
      ],
    },
    {
      session_id: "sess-dev-0002",
      agent_id: null,
      title: "Draft release notes",
      updated_at: new Date(Date.now() - 45 * 60_000).toISOString(),
      transcript: [
        { role: "user", content: "Draft the v0.13 release notes from the merged PRs." },
        {
          role: "assistant",
          content: "Draft ready: highlights on direct-link E2E, mobile TLS pinning, and the relay auto-reconnect.",
        },
      ],
    },
  ] as const;

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
        let frame: { type?: string; prompt?: string; session_id?: string; before?: string; limit?: number };
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
        // §J: the one-shot session-data RPCs the gateway's shannon/session.*
        // surface proxies (see engineSessions.ts for the frame matchers).
        if (frame.type === "sessions.list") {
          send({
            type: "sessions.snapshot",
            sessions: this.sessions.map(({ transcript: _t, ...s }) => s),
          });
          return;
        }
        if (frame.type === "session.history") {
          const session = this.sessions.find((s) => s.session_id === frame.session_id);
          const messages = session ? [...session.transcript] : [];
          const limit =
            typeof frame.limit === "number" && Number.isFinite(frame.limit) && frame.limit >= 1
              ? Math.floor(frame.limit)
              : 50;
          send({
            type: "session.transcript",
            session_id: frame.session_id ?? "",
            messages: messages.slice(0, limit),
            has_more: messages.length > limit,
          });
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
            // §L1 rich fields — exercise the phone's agent/ts/risk rendering.
            ts: Date.now(),
            agent: { id: "agent-dev-1", name: "Dev Agent" },
            risk: { destructive: false, scope: "repo", reversible: true },
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

// ── §K task face (the REAL dispatch pipeline) ────────────────────────────────
// Same composition the bootstrap builds for platform "mobile": a dispatch
// hub (journal + §K3 structured stream + approval parking) fed into the
// mobile-only SessionRouter pipeline (serial per-device lane → §K3 task turn
// handler → engine), with `shannon/task.*` served off the same hub. The fake
// engine's scripted turns (text deltas → Edit approval gate → usage →
// completed) exercise the whole chain end to end.
const DEFAULT_MODEL = "claude-sonnet-4-6";
const approvals = new ApprovalRegistry();
const hub = new MobileDispatchHub({ logger, approvals });
// Shared in-flight query registry: the engine bridge (shannon/query/cancel)
// and the dispatch pipeline's lane clients register against ONE instance, so
// shannon/cancel can interrupt a dispatched task's engine turn (same
// instance-injection pattern as the approval registry above).
const activeQueries = new ActiveQueryRegistry();
const pipeline = createMobileDispatchPipeline({
  hub,
  engineWsUrl,
  engineHttpBaseUrl: engineHttpBase,
  defaultModel: DEFAULT_MODEL,
  logger,
  activeQueries,
});
hub.setSubmit(pipeline.submit);

const handlers = createMobileHandlers({
  engine: {
    engineWsUrl,
    engineHttpBaseUrl: engineHttpBase,
    defaultModel: DEFAULT_MODEL,
    version: GATEWAY_VERSION,
    logger,
    // §K: a signed shannon/approval/decide unblocks a dispatched task's
    // parked approval lane (same wiring as the bootstrap).
    approvalDecisionSink: (requestId, choice) => hub.settleApproval(requestId, choice),
    // r2-w2d: shannon/cancel inside the approval-parking window deny-settles
    // the device's parked approvals (same wiring as the bootstrap) — the
    // cancelled task's query.failed terminal lands immediately.
    cancelPendingApprovals: (deviceId: string) => hub.cancelPendingApprovals(deviceId),
    // Shared with the pipeline above — cancel reaches dispatched tasks.
    activeQueries,
  },
  tokens,
  registry,
  logger,
  // §L2: the same registry the hub feeds — approval.list / snapshot
  // pendingApprovals stay truthful in dev too.
  approvalRegistry: approvals,
  tasks: createTaskHandlers({
    hub,
    // review §P1-13 parity: revoked devices can't dispatch (dev registries
    // are in-memory, but the check mirrors the live wiring exactly).
    isDeviceTrusted: (deviceId) => registry.has(deviceId),
  }),
  // `access` (shannon/pairing.pending / .approve) intentionally NOT mounted —
  // no IM allowlist / pairing store exists in this host (see header).
});

const server = new MobileServer({
  host: bindHost,
  port: bindPort,
  logger,
  handlers,
  servePage: false,
  // P2-1: hand every accepted connection to the hub so paired devices become
  // push targets for the §K3 task stream.
  onContext: (ctx) => hub.registerConnection(ctx),
});
const handle = await server.start();
// Two consecutive one-time tokens: line 1 is device A (the original smoke
// flow), line 2 is device B — the mobile smoke's "dispatch events reach ONLY
// the initiating device" negative half needs a second paired bystander.
// PairTokenStore.issue() has no rate limit / single-mint assumption (single-USE
// applies to consume only), so back-to-back issuance is fine; both tokens are
// minted against ONE issuedAt so the two lines share the same expiresAt.
const bootIssuedAt = Date.now();
const bootLine = (token: string, expiresAt: number): string => {
  const qr = { v: 1, scheme: "ws", host: qrHost, port: handle.port, token, exp: expiresAt };
  return JSON.stringify({
    host: qrHost,
    port: handle.port,
    token,
    expiresAt,
    enginePort: engineHttpBase,
    qr: JSON.stringify(qr),
  });
};
const recordA = tokens.issue({ issuedAt: bootIssuedAt });
const recordB = tokens.issue({ issuedAt: bootIssuedAt });
process.stdout.write(`${bootLine(recordA.token, recordA.expiresAt)}\n`);
process.stdout.write(`${bootLine(recordB.token, recordB.expiresAt)}\n`);

let stopping = false;
const shutdown = (): void => {
  if (stopping) return;
  stopping = true;
  void (async () => {
    await server.stop().catch(() => {});
    await pipeline.stop().catch(() => {});
    await engine?.stop().catch(() => {});
    process.exit(0);
  })();
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
process.stdin.on("end", shutdown);

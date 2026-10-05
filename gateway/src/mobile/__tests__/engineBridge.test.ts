import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import type { EngineEvent } from "../../engine/runtime.js";
import { ShannonError, type ShannonEvent } from "../protocol.js";
import { MobileServer, type MethodHandlers } from "../server.js";
import { createEngineHandlers, mapEngineEvent, type EngineClient } from "../engineBridge.js";

/**
 * P1.1b acceptance: the engine bridge wires `shannon/*` to a mock engine through
 * a real MobileServer + real WS client. `mapEngineEvent` (the engine→mobile
 * semantics) is covered as a pure table; the rest exercise the full
 * NDJSON → dispatch → handler → engine → notification → response path.
 */
const logger = createConsoleLogger("error");

let servers: { stop: () => Promise<void> }[] = [];
afterEach(async () => {
  await Promise.all(servers.map((s) => s.stop().catch(() => {})));
  servers = [];
});

async function start(handlers: MethodHandlers): Promise<{ port: number; stop: () => Promise<void> }> {
  const server = new MobileServer({ host: "127.0.0.1", port: 0, logger, handlers });
  const handle = await server.start();
  servers.push(handle);
  return handle;
}

function connect(port: number): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
    socket.once("open", () => resolve(socket));
    socket.once("error", reject);
  });
}

let rpcId = 0;
function rpc(socket: WebSocket, method: string, params?: unknown): Promise<any> {
  const id = ++rpcId;
  return new Promise((resolve, reject) => {
    const onMessage = (data: unknown): void => {
      const msg = JSON.parse(String(data)) as { id?: number };
      if (msg.id === id) {
        socket.off("message", onMessage);
        resolve(msg);
      }
    };
    socket.on("message", onMessage);
    socket.on("error", reject);
    socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
  });
}

/** Send a request and collect every `shannon/event` notification + the terminal response. */
function rpcStream(socket: WebSocket, method: string, params: unknown): {
  events: ShannonEvent[];
  response: Promise<any>;
} {
  const id = ++rpcId;
  const events: ShannonEvent[] = [];
  const response = new Promise<any>((resolve, reject) => {
    const onMessage = (data: unknown): void => {
      const msg = JSON.parse(String(data));
      if (msg.method === "shannon/event") {
        events.push(msg.params as ShannonEvent);
        return;
      }
      if (msg.id === id) {
        socket.off("message", onMessage);
        resolve(msg);
      }
    };
    socket.on("message", onMessage);
    socket.on("error", reject);
    socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
  });
  return { events, response };
}

// ── mock engine ─────────────────────────────────────────────────────────────

interface FakeEngineOpts {
  script: EngineEvent[];
  /** Block inside runQuery until `cancel()` fires, then emit `cancelled`. */
  awaitCancel?: boolean;
  /** Force connect() to throw (engine down). */
  connectFails?: boolean;
}

class FakeEngine implements EngineClient {
  private readonly opts: FakeEngineOpts;
  cancelled = false;
  private cancelResolver: (() => void) | null = null;
  lastPrompt: string | null = null;
  lastModel: string | null = null;
  lastSessionId: string | null = null;

  constructor(opts: FakeEngineOpts) {
    this.opts = opts;
  }

  async connect(): Promise<void> {
    if (this.opts.connectFails) throw new Error("engine socket refused");
  }

  cancel(): void {
    this.cancelled = true;
    this.cancelResolver?.();
  }

  async close(): Promise<void> {}

  async *runQuery(
    prompt: string,
    o?: { model?: string | null; sessionId?: string | null },
  ): AsyncGenerator<EngineEvent> {
    this.lastPrompt = prompt;
    this.lastModel = o?.model ?? null;
    this.lastSessionId = o?.sessionId ?? null;
    for (const ev of this.opts.script) yield ev;
    if (this.opts.awaitCancel) {
      if (!this.cancelled) {
        await new Promise<void>((r) => {
          this.cancelResolver = r;
        });
      }
      yield { type: "cancelled" };
    }
  }
}

/** A factory that records the most-recent engine so a test can drive/cancel it. */
function fakeFactory(holder: { current: FakeEngine | null }, opts: FakeEngineOpts) {
  return () => {
    const f = new FakeEngine(opts);
    holder.current = f;
    return f;
  };
}

function mockResponse(status: number, body = ""): Response {
  return { status, ok: status >= 200 && status < 300, text: async () => body } as unknown as Response;
}

// ── mapEngineEvent (pure unit) ──────────────────────────────────────────────

describe("mapEngineEvent", () => {
  it("maps text → task.progress(content)", () => {
    expect(mapEngineEvent({ type: "text", content: "hi" })).toEqual({
      type: "task.progress",
      content: "hi",
    });
  });

  it("maps tool_use / tool_result → task.progress(tool)", () => {
    expect(mapEngineEvent({ type: "tool_use", name: "bash", input: { cmd: "ls" } })).toEqual({
      type: "task.progress",
      tool: { kind: "use", name: "bash", input: { cmd: "ls" } },
    });
    expect(mapEngineEvent({ type: "tool_result", name: "bash", output: "ok" })).toEqual({
      type: "task.progress",
      tool: { kind: "result", name: "bash", output: "ok" } as never,
    });
  });

  it("maps usage → task.progress(usage)", () => {
    expect(
      mapEngineEvent({ type: "usage", input_tokens: 10, output_tokens: 5, cost_usd: 0.01 }),
    ).toEqual({
      type: "task.progress",
      usage: { input_tokens: 10, output_tokens: 5, cost_usd: 0.01 },
    });
  });

  it("maps terminal events", () => {
    expect(mapEngineEvent({ type: "completed", model: "gpt-x" })).toEqual({
      type: "query.completed",
      model: "gpt-x",
    });
    expect(mapEngineEvent({ type: "failed", error: "boom" })).toEqual({
      type: "query.failed",
      error: "boom",
    });
    expect(mapEngineEvent({ type: "cancelled" })).toEqual({ type: "query.cancelled" });
    expect(mapEngineEvent({ type: "error", message: "oops" })).toEqual({
      type: "query.failed",
      error: "oops",
    });
  });

  it("maps approval_request → approval.request with all fields", () => {
    const ev = {
      type: "approval_request",
      request_id: "r1",
      tool_name: "write_file",
      tool_input: { path: "/x" },
      description: "write /x",
      is_destructive: true,
      diff_preview: "--- a\n+++ b\n",
    } as const;
    expect(mapEngineEvent(ev)).toEqual({ ...ev, type: "approval.request" });
  });

  it("§L1: passes the engine's rich ts/agent/risk through the approval.request mapping", () => {
    const ev = {
      type: "approval_request" as const,
      request_id: "r2",
      tool_name: "Edit",
      tool_input: { path: "/x" },
      description: "edit /x",
      is_destructive: false,
      diff_preview: null,
      ts: 1759500000000,
      agent: { id: "agent-0001", name: "Planner" },
      risk: { destructive: false, scope: "repo" as const, reversible: true },
    };
    expect(mapEngineEvent(ev)).toEqual({
      type: "approval.request",
      request_id: "r2",
      tool_name: "Edit",
      tool_input: { path: "/x" },
      description: "edit /x",
      is_destructive: false,
      diff_preview: null,
      ts: 1759500000000,
      agent: { id: "agent-0001", name: "Planner" },
      risk: { destructive: false, scope: "repo", reversible: true },
    });
  });

  it("§L1: omits rich keys when the engine doesn't supply usable values (legacy engines)", () => {
    // Absent fields → keys absent (the legacy six-key shape, byte-identical).
    const bare = mapEngineEvent({
      type: "approval_request",
      request_id: "r3",
      tool_name: "Bash",
      tool_input: {},
      description: "run",
      is_destructive: false,
      diff_preview: null,
    });
    expect(bare).toEqual({
      type: "approval.request",
      request_id: "r3",
      tool_name: "Bash",
      tool_input: {},
      description: "run",
      is_destructive: false,
      diff_preview: null,
    });
    // Null ts / null agent (serde #[serde(default)]) / malformed risk → omitted.
    const nulled = mapEngineEvent({
      type: "approval_request",
      request_id: "r4",
      tool_name: "Bash",
      tool_input: {},
      description: "run",
      is_destructive: false,
      diff_preview: null,
      ts: null,
      agent: null,
      risk: null,
    } as never);
    expect(nulled).not.toHaveProperty("ts");
    expect(nulled).not.toHaveProperty("agent");
    expect(nulled).not.toHaveProperty("risk");
    // A risk that misses the required scope/reversible pair is not invented.
    const badRisk = mapEngineEvent({
      type: "approval_request",
      request_id: "r5",
      tool_name: "Bash",
      tool_input: {},
      description: "run",
      is_destructive: false,
      diff_preview: null,
      risk: { scope: "galaxy" },
    } as never);
    expect(badRisk).not.toHaveProperty("risk");
  });

  it("drops session_info (metadata-only) → null", () => {
    expect(mapEngineEvent({ type: "session_info", message_count: 3, model: "gpt-x" })).toBeNull();
  });

  it("drops thinking (WP-15 P0-2) — reasoning never leaks into task.progress", () => {
    expect(mapEngineEvent({ type: "thinking", content: "chain of thought" })).toBeNull();
  });
});

// ── bridge through a real MobileServer ──────────────────────────────────────

describe("createEngineHandlers (P1.1b)", () => {
  it("streams query.started + mapped events + terminal {ok:true}", async () => {
    const holder = { current: null as FakeEngine | null };
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://127.0.0.1:9",
      version: "test",
      logger,
      engineClientFactory: fakeFactory(holder, {
        script: [
          { type: "text", content: "Hel" },
          { type: "text", content: "lo" },
          { type: "usage", input_tokens: 1, output_tokens: 2, cost_usd: 0 },
          { type: "completed", model: "gpt-x" },
        ],
      }),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const { events, response } = rpcStream(socket, "shannon/query", { prompt: "hi", model: "gpt-x" });
    const res = await response;
    expect(res.result).toEqual({ ok: true });
    expect(events.map((e) => e.type)).toEqual([
      "query.started",
      "task.progress",
      "task.progress",
      "task.progress",
      "query.completed",
    ]);
    expect((events[0] as { turn_id: string }).turn_id).toMatch(/^[0-9a-f-]{36}$/);
    expect((events[1] as { content: string }).content).toBe("Hel");
    // WP-15 P2-8: every task.progress carries the routing key so clients can
    // correlate without the one-in-flight-turn-per-socket convention.
    const turnId = (events[0] as { turn_id: string }).turn_id;
    for (const ev of events) {
      if (ev.type === "task.progress") {
        expect((ev as { turn_id?: string }).turn_id).toBe(turnId);
      }
    }
    expect(holder.current?.lastPrompt).toBe("hi");
    expect(holder.current?.lastModel).toBe("gpt-x");
    socket.close();
  });

  it("rejects query without a prompt with BAD_PARAMS", async () => {
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://127.0.0.1:9",
      version: "test",
      logger,
      engineClientFactory: fakeFactory({ current: null }, { script: [] }),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/query", {});
    expect(res.error?.code).toBe(ShannonError.BAD_PARAMS);
    socket.close();
  });

  it("cancel interrupts the in-flight query and emits query.cancelled", async () => {
    const holder = { current: null as FakeEngine | null };
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://127.0.0.1:9",
      version: "test",
      logger,
      engineClientFactory: fakeFactory(holder, {
        script: [{ type: "text", content: "partial" }],
        awaitCancel: true,
      }),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const { events, response } = rpcStream(socket, "shannon/query", { prompt: "x" });

    // Wait until the turn has started (client is registered), then cancel.
    await vi.waitFor(() => expect(events.length).toBeGreaterThan(0));
    const cancelRes = await rpc(socket, "shannon/cancel", {});
    const res = await response;

    expect(cancelRes.result).toEqual({ ok: true });
    expect(holder.current?.cancelled).toBe(true);
    expect(events.map((e) => e.type)).toEqual(["query.started", "task.progress", "query.cancelled"]);
    expect(res.result).toEqual({ ok: true });
    socket.close();
  });

  it("approval/decide POSTs allow→allow_once and returns ok", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      engineClientFactory: fakeFactory({ current: null }, { script: [] }),
      fetchImpl: fetchMock,
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/approval/decide", {
      request_id: "r1",
      choice: "allow",
      signature: "sig",
    });
    expect(res.result).toEqual({ ok: true });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(String(url)).toBe("http://engine:33420/api/approval/respond");
    expect((init as RequestInit).method).toBe("POST");
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({
      request_id: "r1",
      choice: "allow_once",
    });
    socket.close();
  });

  it("health reports engine up on 2xx and down on connection failure", async () => {
    const up = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      engineClientFactory: fakeFactory({ current: null }, { script: [] }),
      fetchImpl: up,
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const okRes = await rpc(socket, "shannon/health");
    expect(okRes.result).toEqual({ gateway: "ok", engine: "ok", version: "test" });

    // Switch the fetch to a refusing one and re-probe.
    up.mockImplementation(async () => {
      throw new Error("ECONNREFUSED");
    });
    const downRes = await rpc(socket, "shannon/health");
    expect(downRes.result.engine).toBe("down");
    expect(downRes.result.gateway).toBe("ok");
    socket.close();
  });

  it("model.switch overrides model.list", async () => {
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      defaultModel: "default-m",
      version: "test",
      logger,
      engineClientFactory: fakeFactory({ current: null }, { script: [] }),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const before = await rpc(socket, "shannon/model.list");
    expect(before.result).toEqual({ models: [{ id: "default-m" }], current: "default-m" });

    const sw = await rpc(socket, "shannon/model.switch", { model: "big-m" });
    expect(sw.result).toEqual({ ok: true });

    const after = await rpc(socket, "shannon/model.list");
    expect(after.result).toEqual({ models: [{ id: "big-m" }], current: "big-m" });
    socket.close();
  });

  it("pair / agent.detail return NOT_IMPLEMENTED", async () => {
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      engineClientFactory: fakeFactory({ current: null }, { script: [] }),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const pair = await rpc(socket, "shannon/pair");
    expect(pair.error?.code).toBe(ShannonError.NOT_IMPLEMENTED);
    const detail = await rpc(socket, "shannon/agent.detail", { session_id: "s1" });
    expect(detail.error?.code).toBe(ShannonError.NOT_IMPLEMENTED);
    socket.close();
  });
});

// ── §J session face (engine one-shot call seam) ──────────────────────────────

/** A session-capable fake engine: answers `sessions.list` / `session.history` via `call`. */
class SessionFakeEngine implements EngineClient {
  sent: unknown[] = [];
  constructor(
    private readonly respond: (message: any) => unknown,
    private readonly opts: { connectFails?: boolean } = {},
  ) {}
  async connect(): Promise<void> {
    if (this.opts.connectFails) throw new Error("engine socket refused");
  }
  cancel(): void {}
  async close(): Promise<void> {}
  async *runQuery(): AsyncGenerator<EngineEvent> {}
  async call<T>(
    message: unknown,
    match: (frame: unknown) => T | null,
  ): Promise<T> {
    this.sent.push(message);
    const frame = this.respond(message);
    const matched = match(frame);
    if (matched === null) {
      throw new Error(`fake engine produced an unmatched frame: ${JSON.stringify(frame)}`);
    }
    return matched;
  }
}

function sessionHandlers(engine: EngineClient): MethodHandlers {
  return createEngineHandlers({
    engineWsUrl: "ws://127.0.0.1:9",
    engineHttpBaseUrl: "http://engine:33420",
    version: "test",
    logger,
    engineClientFactory: () => engine,
  });
}

describe("shannon/session.list + session.history (§J)", () => {
  it("maps the engine snapshot to the §J1 wire shape (id required; optional keys omitted)", async () => {
    const engine = new SessionFakeEngine(() => ({
      type: "sessions.snapshot",
      sessions: [
        {
          session_id: "sess-1",
          title: "Refactor transport client",
          updated_at: "2026-06-28T14:21:00Z",
          // C8 additive: lifetime token totals ride through camelCase.
          total_input_tokens: 1520,
          total_output_tokens: 843,
          preview: "…",
          turn_count: 4,
        },
        { session_id: "sess-2", title: null, updated_at: null }, // no totals → keys omitted
        { session_id: "", title: "unusable" }, // no id → dropped
      ],
    }));
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/session.list", {});
    expect(res.result).toEqual({
      sessions: [
        {
          id: "sess-1",
          title: "Refactor transport client",
          updatedAt: "2026-06-28T14:21:00Z",
          totalInputTokens: 1520,
          totalOutputTokens: 843,
        },
        { id: "sess-2" },
      ],
    });
    // v1 request frame is the bare type tag (unknown params never forwarded).
    expect(engine.sent).toEqual([{ type: "sessions.list" }]);
    socket.close();
  });

  it("session.list token totals: non-numeric / negative engine values are omitted, camelCase fallback accepted", async () => {
    const engine = new SessionFakeEngine(() => ({
      type: "sessions_snapshot",
      sessions: [
        {
          session_id: "sess-a",
          total_input_tokens: "many", // junk → omitted
          total_output_tokens: -4, // nonsense → omitted
        },
        {
          session_id: "sess-b",
          totalInputTokens: 10, // camelCase fallback (engine WS convention tolerance)
          totalOutputTokens: 0, // zero IS a usable value
        },
      ],
    }));
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/session.list", {});
    expect(res.result).toEqual({
      sessions: [
        { id: "sess-a" },
        { id: "sess-b", totalInputTokens: 10, totalOutputTokens: 0 },
      ],
    });
    socket.close();
  });

  it("session.history happy path: §J2 wire shape, pagination passthrough, epoch ts normalized", async () => {
    const engine = new SessionFakeEngine((message: any) => {
      expect(message.type).toBe("session.history");
      expect(message.session_id).toBe("sess-1");
      expect(message.before).toBe("2026-06-28T14:20:00Z");
      expect(message.limit).toBe(10);
      return {
        type: "session.transcript",
        session_id: "sess-1",
        has_more: true,
        messages: [
          { role: "user", content: "refactor the reconnect backoff", ts: 1_759_500_000_000 },
          { role: "assistant", content: "on it", ts: "2026-06-28T14:20:05Z" },
          { content: "no role but string content stays" },
        ],
      };
    });
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/session.history", {
      sessionId: "sess-1",
      before: "2026-06-28T14:20:00Z",
      limit: 10,
    });
    expect(res.result).toEqual({
      sessionId: "sess-1",
      hasMore: true,
      messages: [
        { role: "user", content: "refactor the reconnect backoff", ts: "2025-10-03T14:00:00.000Z" },
        { role: "assistant", content: "on it", ts: "2026-06-28T14:20:05Z" },
        { role: "assistant", content: "no role but string content stays" },
      ],
    });
    socket.close();
  });

  it("session.history without pagination omits the optional engine params", async () => {
    const engine = new SessionFakeEngine((message: any) => {
      expect(message.before).toBeUndefined();
      expect(message.limit).toBeUndefined();
      return { type: "session.transcript", session_id: "sess-1", messages: [], has_more: false };
    });
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/session.history", { sessionId: "sess-1" });
    expect(res.result).toEqual({ sessionId: "sess-1", messages: [], hasMore: false });
    socket.close();
  });

  it("session.history limit<1 passes through for the engine-side §J4 clamp (handover §5)", async () => {
    // §J4: "<1 按 1 处理" is ENGINE semantics — the gateway must forward the
    // value verbatim (the old >= 1 guard turned limit<1 into the default-50
    // page, so the clamp never reached the wire). Non-number/non-finite still
    // means "absent".
    const engine = new SessionFakeEngine((message: any) => {
      expect(message.limit).toBe(0);
      return { type: "session.transcript", session_id: "sess-1", messages: [], has_more: false };
    });
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    await rpc(socket, "shannon/session.history", { sessionId: "sess-1", limit: 0 });
    await rpc(socket, "shannon/session.history", { sessionId: "sess-1", limit: -3 });
    socket.close();

    const nonFinite = new SessionFakeEngine((message: any) => {
      expect(message.limit).toBeUndefined();
      return { type: "session.transcript", session_id: "sess-1", messages: [], has_more: false };
    });
    const { port: port2 } = await start(sessionHandlers(nonFinite));
    const socket2 = await connect(port2);
    await rpc(socket2, "shannon/session.history", { sessionId: "sess-1", limit: "5" });
    socket2.close();
  });

  it("session.history missing/blank sessionId → INVALID_PARAMS (no engine call)", async () => {
    const engine = new SessionFakeEngine(() => {
      throw new Error("engine must not be called");
    });
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const missing = await rpc(socket, "shannon/session.history", {});
    expect(missing.error?.code).toBe(ShannonError.BAD_PARAMS);
    const blank = await rpc(socket, "shannon/session.history", { sessionId: "" });
    expect(blank.error?.code).toBe(ShannonError.BAD_PARAMS);
    socket.close();
  });

  it("an engine error frame on history degrades to the honest empty transcript (§J2)", async () => {
    const engine = new SessionFakeEngine(() => ({ type: "error", message: "no such session" }));
    const { port } = await start(sessionHandlers(engine));
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/session.history", { sessionId: "ghost" });
    expect(res.error).toBeUndefined();
    expect(res.result).toEqual({ sessionId: "ghost", messages: [], hasMore: false });
    socket.close();
  });

  it("engine connect failure / call-less client → ENGINE_ERROR (distinguishable from empty)", async () => {
    const down = new SessionFakeEngine(() => ({}), { connectFails: true });
    const { port } = await start(sessionHandlers(down));
    const socket = await connect(port);
    const downList = await rpc(socket, "shannon/session.list", {});
    expect(downList.error?.code).toBe(ShannonError.ENGINE_ERROR);
    expect(downList.error?.message).toMatch(/engine session call failed/);
    const downHistory = await rpc(socket, "shannon/session.history", { sessionId: "s" });
    expect(downHistory.error?.code).toBe(ShannonError.ENGINE_ERROR);
    socket.close();

    // A legacy fake without `call` reports the surface as unavailable —
    // still ENGINE_ERROR, never a silent empty list.
    const legacy = new FakeEngine({ script: [] });
    const { port: port2 } = await start(sessionHandlers(legacy));
    const socket2 = await connect(port2);
    const noCall = await rpc(socket2, "shannon/session.list", {});
    expect(noCall.error?.code).toBe(ShannonError.ENGINE_ERROR);
    expect(noCall.error?.message).toMatch(/one-shot calls/);
    socket2.close();
  });
});

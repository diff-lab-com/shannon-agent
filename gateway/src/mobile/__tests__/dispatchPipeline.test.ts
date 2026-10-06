import { EventEmitter } from "node:events";

import { WebSocket } from "ws";
import { describe, expect, it, vi } from "vitest";

import type { Logger } from "../../adapters/types.js";
import type { EngineEvent } from "../../engine/runtime.js";
import type { EngineWsClient } from "../../engine/wsClient.js";
import { createConsoleLogger } from "../../logger.js";
import { ActiveQueryRegistry } from "../../router/activeQueries.js";
import { ApprovalRegistry } from "../approvalRegistry.js";
import { createEngineHandlers } from "../engineBridge.js";
import { createMobileDispatchPipeline } from "../dispatchPipeline.js";
import { TASK_CANCELLED_ERROR } from "../taskTurnHandler.js";
import { MobileDispatchHub } from "../hub.js";
import { createTaskHandlers } from "../taskHandlers.js";
import type { MethodContext } from "../server.js";

/**
 * The §K task face as `dev-standalone.ts` mounts it: `createMobileHandlers`
 * gets `tasks` + the `dispatchPipeline` submit, and a dispatch walks the REAL
 * pipeline (hub → mobile-only SessionRouter lane → §K3 task turn handler →
 * engine) with the §K3 structured stream keyed by the task id and the journal
 * transitions owned by the hub. These tests pin the factory contract — the
 * per-lane engine client is built from the router's `mobile:<deviceId>`
 * session key, `stop()` closes the lane, and an engine-down dispatch fails
 * honestly (the no-FAKE_ENGINE smoke posture).
 */

const logger: Logger = createConsoleLogger("error");

class FakeDeviceSocket extends EventEmitter {
  readyState = WebSocket.OPEN;
  frames: any[] = [];

  send(data: string): void {
    this.frames.push(JSON.parse(data));
  }
}

function fakeCtx(deviceId: string | null, socket = new FakeDeviceSocket()): MethodContext {
  return { socket, sessionId: deviceId, logger } as unknown as MethodContext;
}

function eventsOf(ctx: MethodContext): any[] {
  return ((ctx.socket as unknown as FakeDeviceSocket).frames as any[])
    .filter((f) => f.method === "shannon/event")
    .map((f) => f.params);
}

function mockEngineClient(events: EngineEvent[]): EngineWsClient {
  return {
    connect: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    cancel: vi.fn(() => {}),
    runQuery: vi.fn(async function* (): AsyncGenerator<EngineEvent> {
      for (const e of events) yield e;
    }),
  } as unknown as EngineWsClient;
}

function textEvent(content: string): EngineEvent {
  return { type: "text", content } as EngineEvent;
}

interface Harness {
  hub: MobileDispatchHub;
  approvals: ApprovalRegistry;
  handlers: ReturnType<typeof createTaskHandlers>;
  clientFactory: ReturnType<typeof vi.fn>;
  clients: EngineWsClient[];
  stop: () => Promise<void>;
}

function buildHarness(
  events: EngineEvent[],
  opts: { fetchImpl?: typeof fetch; approvals?: ApprovalRegistry } = {},
): Harness {
  const hub = new MobileDispatchHub({ logger, approvals: opts.approvals });
  const clients: EngineWsClient[] = [];
  const clientFactory = vi.fn((sessionKey: string) => {
    const client = mockEngineClient(events);
    (client as unknown as { __sessionKey?: string }).__sessionKey = sessionKey;
    clients.push(client);
    return client;
  });
  const pipeline = createMobileDispatchPipeline({
    hub,
    engineWsUrl: "ws://engine:33420/api/ws",
    engineHttpBaseUrl: "http://engine:33420",
    defaultModel: "claude-sonnet-4-6",
    logger,
    fetchImpl: opts.fetchImpl,
    engineClientFactory: clientFactory,
  });
  hub.setSubmit(pipeline.submit);
  return {
    hub,
    approvals: opts.approvals ?? new ApprovalRegistry(),
    handlers: createTaskHandlers({ hub }),
    clientFactory,
    clients,
    stop: pipeline.stop,
  };
}

describe("mobile dispatch pipeline (the dev-standalone §K assembly)", () => {
  it("dispatch → §K response → query.started/task.progress/task.message(session_id=task id) → journal completed", async () => {
    const { hub, handlers } = buildHarness([
      textEvent("hello "),
      textEvent("world"),
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const initiator = fakeCtx("dev-1");
    const bystander = fakeCtx("dev-2");
    hub.registerConnection(initiator);
    hub.registerConnection(bystander);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "deploy" }, initiator);
    expect(res.kind).toBe("result");
    const taskId = res.result.task.id as string;
    // §K1: response first — nothing is pushed inside the dispatch macrotask.
    expect(eventsOf(initiator)).toEqual([]);

    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    const events = eventsOf(initiator);
    expect(events.map((e) => e.type)).toEqual([
      "query.started",
      "task.progress",
      "task.progress",
      "task.message",
    ]);
    expect(events[0]).toMatchObject({ type: "query.started", session_id: taskId });
    expect(events[1]).toMatchObject({ session_id: taskId, content: "hello " });
    expect(events[3]).toMatchObject({ session_id: taskId, text: "hello world" });
    // 仅推发起设备.
    expect(eventsOf(bystander)).toEqual([]);
    // The turn handler's §K3 terminal owns the journal flip; the dispatch
    // submit's own completion is a no-op on an already-terminal record.
    expect(hub.listTasks("dev-1")[0]).toMatchObject({
      task_id: taskId,
      status: "completed",
      finished_at: expect.any(Number),
    });
  });

  it("the per-lane engine client is pinned to the router's mobile session key; stop() closes it", async () => {
    const { hub, handlers, clientFactory, clients, stop } = buildHarness([
      textEvent("done"),
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    // The lane's factory received the router session key (`platform:chatId`).
    expect(clientFactory).toHaveBeenCalledWith("mobile:dev-1");

    expect(clients[0]!.close).not.toHaveBeenCalled();
    await stop();
    expect(clients[0]!.close).toHaveBeenCalledTimes(1);
  });

  it("usage frames ride task.progress with the task id (C8 spend keyed by task thread)", async () => {
    const { hub, handlers } = buildHarness([
      textEvent("result"),
      { type: "usage", input_tokens: 12, output_tokens: 7, cost_usd: 0.02 } as EngineEvent,
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    expect(eventsOf(ctx)).toContainEqual(
      expect.objectContaining({
        type: "task.progress",
        session_id: res.result.task.id,
        usage: { input_tokens: 12, output_tokens: 7, cost_usd: 0.02 },
      }),
    );
  });

  it("the approval round trip: parked lane → signed-decide settle → engine POST → task terminal", async () => {
    const posts: Array<{ url: string; body: any }> = [];
    const fetchImpl = (async (url: any, init?: any) => {
      posts.push({ url: String(url), body: JSON.parse(init?.body ?? "{}") });
      return { ok: true, status: 200 } as unknown as Response;
    }) as unknown as typeof fetch;
    const approvals = new ApprovalRegistry();
    const { hub, handlers } = buildHarness(
      [
        {
          type: "approval_request",
          request_id: "req-pipe-1",
          tool_name: "Edit",
          tool_input: { path: "src/main.rs" },
          description: "apply patch",
          is_destructive: false,
          diff_preview: "- 1;\n+ 2;",
        } as EngineEvent,
        textEvent("patched"),
        { type: "completed", model: "m" } as EngineEvent,
      ],
      { fetchImpl, approvals },
    );
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "edit" }, ctx);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() => expect(hub.hasPendingApproval("dev-1")).toBe(true));
    // §L2: the ask is visible to the restore face while parked.
    expect(approvals.listPending().map((r) => r.requestId)).toEqual(["req-pipe-1"]);

    // What `shannon/approval/decide` → approvalDecisionSink does in dev.
    expect(hub.settleApproval("req-pipe-1", "allow")).toBe(true);
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    expect(posts).toEqual([
      { url: "http://engine:33420/api/approval/respond", body: { request_id: "req-pipe-1", choice: "allow_once" } },
    ]);
    expect(approvals.listPending()).toEqual([]);
    const events = eventsOf(ctx).map((e) => e.type);
    expect(events).toEqual(["query.started", "approval.request", "task.progress", "task.message"]);
    expect(eventsOf(ctx).at(-1)).toMatchObject({ session_id: taskId, text: "patched" });
  });

  it("engine down: the dispatch fails honestly — journal failed + query.failed(session_id), no fake terminal", async () => {
    const hub = new MobileDispatchHub({ logger });
    const pipeline = createMobileDispatchPipeline({
      hub,
      engineWsUrl: "ws://127.0.0.1:1/api/ws", // nothing listens here
      engineHttpBaseUrl: "http://127.0.0.1:1",
      logger,
      // Real EngineWsClient path (no factory seam) — the no-FAKE_ENGINE dev boot.
      engineClientFactory: (key) =>
        ({
          connect: async () => {
            throw new Error(`connect ECONNREFUSED (${key})`);
          },
          close: async () => {},
          cancel: () => {},
          async *runQuery(): AsyncGenerator<EngineEvent> {
            throw new Error("unreachable");
          },
        }) as unknown as EngineWsClient,
    });
    hub.setSubmit(pipeline.submit);
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "failed", error: expect.stringContaining("ECONNREFUSED") }),
    );
    const events = eventsOf(ctx);
    expect(events.map((e) => e.type)).toEqual(["query.started", "query.failed"]);
    expect(events[1]).toMatchObject({ session_id: taskId });
    expect(events[1].error).toContain("ECONNREFUSED");
  });

  it("cancel roundtrip: shannon/cancel interrupts the dispatched task's lane client → query.failed + journal failed + registry cleared", async () => {
    // The shared registry — the SAME instance the engine bridge below holds,
    // exactly what dev-standalone/bootstrap construct.
    const activeQueries = new ActiveQueryRegistry();
    const hub = new MobileDispatchHub({ logger });
    // A lane client that parks mid-turn until cancel() fires, then emits the
    // engine's `cancelled` terminal (the real EngineWsClient contract).
    let releaseTurn: (() => void) | null = null;
    const cancelRequested = new Promise<void>((resolve) => {
      releaseTurn = resolve;
    });
    const state = { cancelCalled: false };
    const pipeline = createMobileDispatchPipeline({
      hub,
      engineWsUrl: "ws://engine:33420/api/ws",
      engineHttpBaseUrl: "http://engine:33420",
      defaultModel: "claude-sonnet-4-6",
      logger,
      activeQueries,
      engineClientFactory: () =>
        ({
          connect: async () => {},
          close: async () => {},
          cancel: () => {
            state.cancelCalled = true;
            releaseTurn!();
          },
          async *runQuery(): AsyncGenerator<EngineEvent> {
            yield textEvent("partial ");
            await cancelRequested;
            yield { type: "cancelled" } as EngineEvent;
          },
        }) as unknown as EngineWsClient,
    });
    hub.setSubmit(pipeline.submit);
    const taskHandlers = createTaskHandlers({ hub });
    // The engine bridge sharing the registry — shannon/cancel's real handler.
    const bridgeHandlers = createEngineHandlers({
      engineWsUrl: "ws://engine:33420/api/ws",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      activeQueries,
    });

    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const res: any = await taskHandlers["shannon/task.dispatch"]!({ prompt: "long run" }, ctx);
    const taskId = res.result.task.id as string;

    // Turn started → the lane client is registered under the router session key.
    await vi.waitFor(() => expect(activeQueries.size).toBe(1));
    expect(state.cancelCalled).toBe(false);

    // The bare device session id — the alias probe (`mobile:dev-1`) is what
    // finds the dispatched task's lane client.
    const cancelRes: any = await bridgeHandlers["shannon/cancel"]!({ session_id: "dev-1" }, ctx);
    expect(cancelRes).toMatchObject({ kind: "result", result: { ok: true } });
    expect(state.cancelCalled).toBe(true);

    // §K3 failure terminal: journal failed + query.failed(session_id=task.id).
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "failed", error: TASK_CANCELLED_ERROR }),
    );
    expect(eventsOf(ctx).at(-1)).toMatchObject({
      type: "query.failed",
      session_id: taskId,
      error: TASK_CANCELLED_ERROR,
    });
    // The turn's terminal left the registry clean — nothing stale for the
    // lane's next turn.
    await vi.waitFor(() => expect(activeQueries.size).toBe(0));

    // Cancel with nothing in flight is still an idempotent no-op success.
    const cancelMiss: any = await bridgeHandlers["shannon/cancel"]!({ session_id: "dev-1" }, ctx);
    expect(cancelMiss).toMatchObject({ kind: "result", result: { ok: true } });
  });

  it("cancel inside the approval-parking window: parked approval deny-settles → query.failed arrives immediately (r2-w2d), approval.list clears, journal failed", async () => {
    // The dev-standalone/bootstrap assembly: shared ActiveQueryRegistry AND
    // shared ApprovalRegistry, the bridge wired to the hub's cancel settle.
    const activeQueries = new ActiveQueryRegistry();
    const approvals = new ApprovalRegistry();
    const hub = new MobileDispatchHub({ logger, approvals });

    // A lane client that parks at its approval gate until cancel() fires, then
    // emits the engine's `cancelled` terminal — the real contract (cancel
    // frame → engine aborts the parked query → `cancelled`). Before the fix
    // the cancel reached the engine but the turn handler stayed blocked in
    // resolveApprovalInChannel for the full 300s approval window.
    const posts: Array<{ url: string; body: any }> = [];
    const fetchImpl = (async (url: any, init?: any) => {
      posts.push({ url: String(url), body: JSON.parse(init?.body ?? "{}") });
      return { ok: true, status: 200 } as unknown as Response;
    }) as unknown as typeof fetch;
    let releaseCancelled: (() => void) | null = null;
    const cancelRequested = new Promise<void>((resolve) => {
      releaseCancelled = resolve;
    });
    const state = { cancelCalled: false };
    const pipeline = createMobileDispatchPipeline({
      hub,
      engineWsUrl: "ws://engine:33420/api/ws",
      engineHttpBaseUrl: "http://engine:33420",
      defaultModel: "claude-sonnet-4-6",
      logger,
      activeQueries,
      fetchImpl,
      engineClientFactory: () =>
        ({
          connect: async () => {},
          close: async () => {},
          cancel: () => {
            state.cancelCalled = true;
            releaseCancelled!();
          },
          async *runQuery(): AsyncGenerator<EngineEvent> {
            yield {
              type: "approval_request",
              request_id: "req-cancel-park-1",
              tool_name: "Edit",
              tool_input: { path: "src/main.rs" },
              description: "apply patch",
              is_destructive: false,
              diff_preview: "- 1;\n+ 2;",
            } as EngineEvent;
            await cancelRequested;
            yield { type: "cancelled" } as EngineEvent;
          },
        }) as unknown as EngineWsClient,
    });
    hub.setSubmit(pipeline.submit);
    const taskHandlers = createTaskHandlers({ hub });
    // The engine bridge sharing both registries — shannon/cancel's real
    // handler with the hub settle injected (the bootstrap/dev wiring).
    const bridgeHandlers = createEngineHandlers({
      engineWsUrl: "ws://engine:33420/api/ws",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      activeQueries,
      approvalRegistry: approvals,
      cancelPendingApprovals: (deviceId) => hub.cancelPendingApprovals(deviceId),
    });

    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const t0 = Date.now();
    const res: any = await taskHandlers["shannon/task.dispatch"]!({ prompt: "risky op" }, ctx);
    const taskId = res.result.task.id as string;

    // The turn is parked at its approval gate: the ask is pushed, visible to
    // the restore face AND still parked in the hub (the lane is blocked in
    // resolveApprovalInChannel — the parking window the cancel must hit).
    await vi.waitFor(() => expect(hub.hasPendingApproval("dev-1")).toBe(true));
    expect(approvals.listPending().map((r) => r.requestId)).toEqual(["req-cancel-park-1"]);
    expect(state.cancelCalled).toBe(false);

    // Cancel INSIDE the parking window (bare device session id — the lane
    // alias probe finds the dispatched task's client).
    const cancelRes: any = await bridgeHandlers["shannon/cancel"]!({ session_id: "dev-1" }, ctx);
    expect(cancelRes).toMatchObject({ kind: "result", result: { ok: true } });
    expect(state.cancelCalled).toBe(true);

    // The terminal lands WITHOUT the approval timeout: the parked ask was
    // deny-settled by the cancel, unblocking the lane to observe the engine's
    // `cancelled` event. (A regression here hangs on the 300s hub timeout —
    // vi.waitFor + the test's own budget fail long before that.)
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "failed", error: TASK_CANCELLED_ERROR }),
    );
    const elapsedMs = Date.now() - t0;
    expect(elapsedMs).toBeLessThan(10_000);
    expect(eventsOf(ctx).at(-1)).toMatchObject({
      type: "query.failed",
      session_id: taskId,
      error: TASK_CANCELLED_ERROR,
    });

    // The deny decision still rode the engine POST (existing tolerance: the
    // engine may have already aborted the query — a failed POST only warns).
    expect(posts).toEqual([
      { url: "http://engine:33420/api/approval/respond", body: { request_id: "req-cancel-park-1", choice: "deny" } },
    ]);

    // approval.list 出清: the settled ask left the restore face, and nothing
    // stays parked in the hub.
    expect(approvals.listPending()).toEqual([]);
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
    // The turn's terminal also cleaned the shared registry.
    await vi.waitFor(() => expect(activeQueries.size).toBe(0));
  });
});

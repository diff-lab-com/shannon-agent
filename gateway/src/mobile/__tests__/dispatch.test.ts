import { EventEmitter } from "node:events";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { WebSocket } from "ws";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  type Logger,
  type NormalizedInbound,
} from "../../adapters/types.js";
import { parseApprovalChoice } from "../../adapters/approvalChoice.js";
import type { EngineEvent } from "../../engine/runtime.js";
import type { EngineWsClient } from "../../engine/wsClient.js";
import { bootstrap } from "../../bootstrap.js";
import { createConsoleLogger } from "../../logger.js";
import { SessionRouter } from "../../router/router.js";
import { type TurnHandler } from "../../router/types.js";
import { createMobileChannelAdapter } from "../channel.js";
import {
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  signMessage,
} from "../crypto.js";
import { MobileDispatchHub } from "../hub.js";
import { createEngineHandlers } from "../engineBridge.js";
import { loadAgentRoster } from "../agentRoster.js";
import { ApprovalRegistry } from "../approvalRegistry.js";
import { createTaskHandlers } from "../taskHandlers.js";
import { createMobileTaskTurnHandler } from "../taskTurnHandler.js";
import { MobileServer, type MethodContext } from "../server.js";

/**
 * §K acceptance: the mobile task face over the T9 pipeline —
 * dispatch (`{prompt}` → §K task object), list (§K2 projection), the §K3
 * structured task stream (query.started / task.progress / task.message /
 * query.failed, session_id = task id, initiating device only), approvals via
 * the signed decide path (the Y/N text dialect left the RPC face), plus the
 * security gate (unpaired devices are rejected) and the PWA page serving.
 */

const logger: Logger = createConsoleLogger("error");

/** A fake open device socket: records pushed NDJSON notification frames. */
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

/** Events a fake device has received as shannon/event notifications. */
function eventsOf(ctx: MethodContext): any[] {
  return ((ctx.socket as unknown as FakeDeviceSocket).frames as any[])
    .filter((f) => f.method === "shannon/event")
    .map((f) => f.params);
}

/** Every raw notification frame a fake device has received, any method. */
function framesOf(ctx: MethodContext): any[] {
  return (ctx.socket as unknown as FakeDeviceSocket).frames as any[];
}

/** P2-9: the agent maps a fake device received as shannon/agent.state pushes. */
function agentStatesOf(ctx: MethodContext): any[] {
  return framesOf(ctx)
    .filter((f) => f.method === "shannon/agent.state")
    .map((f) => f.params.agent);
}

export function mockEngineClient(events: EngineEvent[]): EngineWsClient {
  return {
    connect: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
    cancel: vi.fn(() => {}),
    // Pull-based streaming: the generator stays suspended on the approval_request
    // yield until the turn handler finishes the approval round-trip and asks for
    // the next event — which is exactly the engine's own behavior.
    runQuery: vi.fn(async function* (): AsyncGenerator<EngineEvent> {
      for (const e of events) {
        yield e;
      }
    }),
  } as unknown as EngineWsClient;
}

function textEvent(content: string): EngineEvent {
  return { type: "text", content } as EngineEvent;
}

// ── unit: choice parsing + hub behaviors ─────────────────────────────────────

describe("mobile dispatch — hub & handlers", () => {
  it("parseApprovalChoice shares the DingTalk Y/N dialect (IM adapters)", () => {
    expect(parseApprovalChoice("y")).toBe("allow");
    expect(parseApprovalChoice("允许")).toBe("allow");
    expect(parseApprovalChoice(" 拒绝 ")).toBe("deny");
    expect(parseApprovalChoice("N")).toBe("deny");
    expect(parseApprovalChoice("run it")).toBeNull();
  });

  it("rejects task.dispatch and task.list from an unpaired connection", async () => {
    const hub = new MobileDispatchHub({ logger });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx(null); // no shannon/pair yet

    const dispatch = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    expect(dispatch).toMatchObject({ kind: "error", code: -32000 });

    const list = await handlers["shannon/task.list"]!({}, ctx);
    expect(list).toMatchObject({ kind: "error", code: -32000 });
  });

  it("rejects dispatch from a paired-but-revoked device (§P1-13)", async () => {
    const hub = new MobileDispatchHub({ logger });
    const handlers = createTaskHandlers({ hub, isDeviceTrusted: () => false });
    const ctx = fakeCtx("dev-1");
    const res = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    expect(res).toMatchObject({ kind: "error", code: -32000 });
  });

  it("rejects an empty prompt with INVALID_PARAMS (§K1)", async () => {
    const hub = new MobileDispatchHub({ logger });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    for (const bad of [{}, { prompt: "" }, { prompt: "   " }, { prompt: 7 }]) {
      const res = await handlers["shannon/task.dispatch"]!(bad, ctx);
      expect(res).toMatchObject({
        kind: "error",
        code: -32001,
        message: "params.prompt (non-empty string) is required",
      });
    }
  });

  it("rejects an agent_id outside the roster with INVALID_PARAMS — no silent re-route (§K1, B0)", async () => {
    const hub = new MobileDispatchHub({ logger });
    const seen: NormalizedInbound[] = [];
    hub.setSubmit(async (inbound) => {
      seen.push(inbound);
    });
    // Deterministically empty roster (the default would scan the real home).
    const handlers = createTaskHandlers({ hub, agentRosterDirs: [] });
    const ctx = fakeCtx("dev-1");

    for (const agentId of ["agent-0001", "me"]) {
      const res = await handlers["shannon/task.dispatch"]!(
        { prompt: "hi", agent_id: agentId },
        ctx,
      );
      expect(res).toMatchObject({ kind: "error", code: -32001 });
      expect((res as any).message).toContain("agent_id");
    }
    expect(seen).toHaveLength(0); // nothing was dispatched

    // Absent / null / blank agent_id ≈ absent: the host dispatches itself.
    const ok = await handlers["shannon/task.dispatch"]!({ prompt: "hi", agent_id: null }, ctx);
    expect(ok).toMatchObject({ kind: "result" });
    const blank = await handlers["shannon/task.dispatch"]!({ prompt: "hi", agent_id: "  " }, ctx);
    expect(blank).toMatchObject({ kind: "result" });
    await vi.waitFor(() => expect(seen).toHaveLength(2)); // deferred submission
  });

  it("B0: an agent_id in the roster is accepted — wire agent_id + journal attribution (§K1)", async () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-dispatch-roster-"));
    try {
      mkdirSync(dir, { recursive: true });
      writeFileSync(
        join(dir, "backend.toml"),
        ['name = "backend-dev"', 'description = "Backend development specialist"'].join("\n"),
        "utf8",
      );
      const hub = new MobileDispatchHub({ logger });
      const seen: NormalizedInbound[] = [];
      hub.setSubmit(async (inbound) => {
        seen.push(inbound);
      });
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      // Exact id — accepted; surrounding whitespace is trimmed before the
      // roster compare, so it lands on the same entry.
      for (const agentId of ["backend-dev", "  backend-dev  "]) {
        const res: any = await handlers["shannon/task.dispatch"]!(
          { prompt: "ship it", agent_id: agentId },
          ctx,
        );
        expect(res.kind).toBe("result");
        expect(res.result.task).toMatchObject({
          prompt: "ship it",
          status: "running",
          agent_id: "backend-dev",
        });
      }
      await vi.waitFor(() => expect(hub.listTasks("dev-1")).toHaveLength(2));
      await vi.waitFor(() =>
        expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "completed" }),
      );
      // The journal carries the attribution, and task.list projects it.
      for (const record of hub.listTasks("dev-1")) {
        expect(record.agent_id).toBe("backend-dev");
      }
      const list = (await handlers["shannon/task.list"]!({}, ctx)) as any;
      expect(list.result.tasks.map((t: any) => t.agent_id)).toEqual([
        "backend-dev",
        "backend-dev",
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("B0: dispatch without agent_id keeps agent_id null (backward compatible)", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {});
    const handlers = createTaskHandlers({ hub, agentRosterDirs: [] });
    const ctx = fakeCtx("dev-1");

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "plain" }, ctx);
    expect(res.kind).toBe("result");
    expect(res.result.task.agent_id).toBeNull();
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]!.agent_id).toBeNull());
  });

  it("dispatch answers the §K task object synchronously and journals the task", async () => {
    const hub = new MobileDispatchHub({ logger });
    const seen: NormalizedInbound[] = [];
    hub.setSubmit(async (inbound) => {
      seen.push(inbound);
    });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "部署 staging 环境" }, ctx);
    expect(res.kind).toBe("result");
    const task = res.result.task;
    expect(task).toMatchObject({
      prompt: "部署 staging 环境",
      status: "running",
      agent_id: null,
    });
    const taskId = task.id as string;
    expect(taskId).toBeTruthy();
    // created_at is ISO-8601 UTC.
    expect(new Date(task.created_at).toISOString()).toBe(task.created_at);

    // The turn submission is deferred (§K1: response first), so wait for it.
    await vi.waitFor(() => expect(seen).toHaveLength(1));
    expect(seen[0]).toMatchObject({
      platform: "mobile",
      chatId: "dev-1",
      senderId: "dev-1",
      text: "部署 staging 环境",
      isDirect: true,
    });
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({
        task_id: taskId,
        status: "completed",
        title: "部署 staging 环境",
      }),
    );
  });

  it("§K ruling: dispatch NEVER settles a pending approval — 'y' just creates a task", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {});
    void hub.requestApproval("dev-1", {
      requestId: "req-y",
      toolName: "Bash",
      toolInput: {},
      description: "运行命令",
      isDestructive: false,
      diffPreview: null,
    });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "y" }, ctx);
    expect(res.result.task).toBeTruthy(); // a task was created
    expect(hub.hasPendingApproval("dev-1")).toBe(true); // the ask still parks

    // …and the signed-decide settle keeps working afterwards.
    expect(hub.settleApproval("req-y", "deny")).toBe(true);
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
  });

  it("task.list returns the §K2 projection: own tasks, newest first, no legacy keys", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {});
    const mine = fakeCtx("dev-1");
    const other = fakeCtx("dev-2");
    hub.registerConnection(mine);
    hub.registerConnection(other);
    const handlers = createTaskHandlers({ hub });

    await handlers["shannon/task.dispatch"]!({ prompt: "mine first" }, mine);
    await handlers["shannon/task.dispatch"]!({ prompt: "mine second" }, mine);
    await handlers["shannon/task.dispatch"]!({ prompt: "other device" }, other);

    const res = (await handlers["shannon/task.list"]!({ limit: 10 }, mine)) as any;
    expect(res.result.tasks.map((t: any) => t.prompt)).toEqual(["mine second", "mine first"]);
    // §K2 + B1a wire shape: the five §K2 keys plus the additive `title`
    // (these tasks are still running here, so finished_at/error stay absent).
    expect(Object.keys(res.result.tasks[0])).toEqual([
      "id",
      "prompt",
      "status",
      "agent_id",
      "created_at",
      "title",
    ]);
    expect(res.result.tasks[0]!.title).toBe("mine second");
    expect(res.result.tasks[0]!.agent_id).toBeNull();
    expect(new Date(res.result.tasks[0]!.created_at).toISOString()).toBe(
      res.result.tasks[0]!.created_at,
    );
    // limit semantics preserved (default 20, cap 100, floor 1).
    expect(((await handlers["shannon/task.list"]!({ limit: 1 }, mine)) as any).result.tasks).toHaveLength(1);
  });

  it("B1a: completed tasks add finished_at, failed tasks add error — additive keys only", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async (inbound) => {
      if (inbound.chatId === "dev-bad") throw new Error("boom");
    });
    const okCtx = fakeCtx("dev-ok");
    const badCtx = fakeCtx("dev-bad");
    hub.registerConnection(okCtx);
    hub.registerConnection(badCtx);
    const handlers = createTaskHandlers({ hub });

    await handlers["shannon/task.dispatch"]!({ prompt: "fine task" }, okCtx);
    await handlers["shannon/task.dispatch"]!({ prompt: "doomed task" }, badCtx);
    await vi.waitFor(() => expect(hub.listTasks("dev-ok")[0]!.status).toBe("completed"));
    await vi.waitFor(() => expect(hub.listTasks("dev-bad")[0]!.status).toBe("failed"));

    const okList = (await handlers["shannon/task.list"]!({}, okCtx)) as any;
    expect(Object.keys(okList.result.tasks[0])).toEqual([
      "id",
      "prompt",
      "status",
      "agent_id",
      "created_at",
      "title",
      "finished_at",
    ]);
    expect(new Date(okList.result.tasks[0]!.finished_at).toISOString()).toBe(
      okList.result.tasks[0]!.finished_at,
    );
    expect(okList.result.tasks[0]).not.toHaveProperty("error");

    const badList = (await handlers["shannon/task.list"]!({}, badCtx)) as any;
    expect(Object.keys(badList.result.tasks[0])).toEqual([
      "id",
      "prompt",
      "status",
      "agent_id",
      "created_at",
      "title",
      "finished_at",
      "error",
    ]);
    expect(badList.result.tasks[0]!.error).toBe("boom");
  });

  it("a failed submit lands in the journal as failed AND closes the phone's stream with query.failed", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {
      throw new Error("engine exploded");
    });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    await handlers["shannon/task.dispatch"]!({ prompt: "doomed task" }, ctx);
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "failed", error: "engine exploded" }),
    );
    await vi.waitFor(() => {
      const failed = eventsOf(ctx).find((e) => e.type === "query.failed");
      expect(failed).toMatchObject({ error: "engine exploded" });
      expect(failed.session_id).toBe(hub.listTasks("dev-1")[0]!.task_id);
    });
  });

  it("adapter.send throws when the device has no open connection (legacy bubble path)", async () => {
    const hub = new MobileDispatchHub({ logger });
    const adapter = createMobileChannelAdapter({ hub });
    await expect(adapter.send({ platform: "mobile", chatId: "ghost" }, "hello")).rejects.toThrow(
      /no connected device/,
    );
  });

  it("§M2 broadcastEvent reaches every connected device except the excluded one", () => {
    const hub = new MobileDispatchHub({ logger });
    const a = fakeCtx("dev-a");
    const b = fakeCtx("dev-b");
    const c = fakeCtx("dev-c"); // registered but never bound
    hub.registerConnection(a);
    hub.registerConnection(b);
    hub.registerConnection(c);
    c.sessionId = null; // no session → never a push target

    hub.broadcastEvent({ type: "device.revoked", device_id: "dev-b" }, "dev-b");
    const aEvents = eventsOf(a).filter((e) => e.type === "device.revoked");
    const bEvents = eventsOf(b).filter((e) => e.type === "device.revoked");
    expect(aEvents).toHaveLength(1);
    expect(aEvents[0]).toMatchObject({ type: "device.revoked", device_id: "dev-b" });
    // The revoked device itself does not hear the broadcast.
    expect(bEvents).toHaveLength(0);
  });
});

// ── §L2: the hub feeds the pending-approval registry ─────────────────────────

describe("mobile dispatch — approval registry integration", () => {
  const req = {
    requestId: "req-reg-1",
    toolName: "Bash",
    toolInput: { command: "echo hi" },
    description: "运行命令",
    isDestructive: false,
    diffPreview: null,
  };

  it("requestApproval records the ask; the §K settleApproval (decide wiring) resolves it", async () => {
    const approvals = new ApprovalRegistry();
    const hub = new MobileDispatchHub({ logger, approvals });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const pending = hub.requestApproval("dev-1", req);
    expect(approvals.listPending().map((r) => r.requestId)).toEqual(["req-reg-1"]);
    expect(approvals.listPending()[0]).toMatchObject({
      toolName: "Bash",
      description: "运行命令",
      isDestructive: false,
    });

    expect(hub.settleApproval("req-reg-1", "allow")).toBe(true);
    await expect(pending).resolves.toBe("allow");
    expect(approvals.listPending()).toEqual([]);
  });

  it("B1b: engine-rich fields ride BOTH the push event and the registry record", async () => {
    // Same frozen clock as the injected ts — the registry's TTL sweep would
    // otherwise prune a 2023-era record against the real clock.
    const approvals = new ApprovalRegistry({ now: () => 1_700_000_123_456 });
    const hub = new MobileDispatchHub({ logger, approvals });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    void hub.requestApproval("dev-1", {
      ...req,
      requestId: "req-rich-1",
      ts: 1_700_000_123_456,
      agent: { id: "agent-1", name: "Scout" },
      risk: { destructive: true, scope: "system", reversible: false },
    });

    const ev = eventsOf(ctx).find((e) => e.type === "approval.request")!;
    expect(ev).toEqual({
      seq: expect.any(Number),
      type: "approval.request",
      request_id: "req-rich-1",
      tool_name: "Bash",
      tool_input: { command: "echo hi" },
      description: "运行命令",
      is_destructive: false,
      diff_preview: null,
      ts: 1_700_000_123_456,
      agent: { id: "agent-1", name: "Scout" },
      risk: { destructive: true, scope: "system", reversible: false },
    });

    const rec = approvals.listPending().find((r) => r.requestId === "req-rich-1")!;
    expect(rec).toMatchObject({
      ts: 1_700_000_123_456,
      agent: { id: "agent-1", name: "Scout" },
      risk: { destructive: true, scope: "system", reversible: false },
    });
  });

  it("B1b: an engine event without rich fields gets nothing invented", async () => {
    const approvals = new ApprovalRegistry({ now: () => 1_700_000_000_000 });
    const hub = new MobileDispatchHub({
      logger,
      approvals,
      now: () => 1_700_000_000_000,
    });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    void hub.requestApproval("dev-1", { ...req, requestId: "req-plain-1" });

    // The push stays byte-identical to the legacy shape — no ts/agent/risk keys.
    const ev = eventsOf(ctx).find((e) => e.type === "approval.request")!;
    expect(Object.keys(ev).sort()).toEqual([
      "description",
      "diff_preview",
      "is_destructive",
      "request_id",
      "seq",
      "tool_input",
      "tool_name",
      "type",
    ]);
    // The restore-face record still carries a ts (hub-clock fallback), with
    // the rich fields normalized to null rather than invented.
    const rec = approvals.listPending()[0]!;
    expect(rec.ts).toBe(1_700_000_000_000);
    expect(rec.agent).toBeNull();
    expect(rec.risk).toBeNull();
  });

  it("settleApproval is a no-op for unknown/already-settled requests", () => {
    const hub = new MobileDispatchHub({ logger });
    expect(hub.settleApproval("nope", "allow")).toBe(false);
  });

  it("the timeout settle (deny) also resolves the registry entry", async () => {
    const approvals = new ApprovalRegistry();
    const hub = new MobileDispatchHub({ logger, approvals, approvalTimeoutMs: 25 });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    void hub.requestApproval("dev-1", req);
    await vi.waitFor(() => expect(approvals.listPending()).toEqual([]));
  });

  it("cancelPendingApprovals deny-settles ALL of the device's parked asks — and only theirs", async () => {
    const approvals = new ApprovalRegistry();
    const hub = new MobileDispatchHub({ logger, approvals });
    hub.registerConnection(fakeCtx("dev-1"));
    hub.registerConnection(fakeCtx("dev-2"));

    const p1 = hub.requestApproval("dev-1", { ...req, requestId: "req-cp-1" });
    const p2 = hub.requestApproval("dev-1", { ...req, requestId: "req-cp-2" });
    const otherDevice = hub.requestApproval("dev-2", { ...req, requestId: "req-cp-3" });

    expect(hub.cancelPendingApprovals("dev-1")).toBe(2);
    await expect(p1).resolves.toBe("deny");
    await expect(p2).resolves.toBe("deny");
    expect(approvals.listPending().map((r) => r.requestId)).toEqual(["req-cp-3"]);
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
    // Another device's parked ask is untouched.
    expect(hub.hasPendingApproval("dev-2")).toBe(true);
    // Idempotent: the second pass finds nothing left.
    expect(hub.cancelPendingApprovals("dev-1")).toBe(0);
    // Another device's ask still settles normally through the decide path.
    expect(hub.settleApproval("req-cp-3", "allow")).toBe(true);
    await expect(otherDevice).resolves.toBe("allow");
  });

  it("cancelPendingApprovals vs the 300s timeout race: first settle wins, the loser is a no-op", async () => {
    const approvals = new ApprovalRegistry();
    const hub = new MobileDispatchHub({ logger, approvals, approvalTimeoutMs: 25 });
    hub.registerConnection(fakeCtx("dev-1"));

    // Cancel wins the race: the waiter resolves once with the cancel's deny,
    // the timer's own deny is absorbed (Promise.resolve is idempotent), and
    // the timeout pass finds the queue already empty.
    const won = hub.requestApproval("dev-1", { ...req, requestId: "req-race-1" });
    expect(hub.cancelPendingApprovals("dev-1")).toBe(1);
    await expect(won).resolves.toBe("deny");
    await new Promise((r) => setTimeout(r, 40)); // let the (cleared) timer fire
    expect(hub.cancelPendingApprovals("dev-1")).toBe(0);
    expect(approvals.size).toBe(0);

    // Timeout wins the race: the entry is already gone, cancel settles 0.
    void hub.requestApproval("dev-1", { ...req, requestId: "req-race-2" });
    await vi.waitFor(() => expect(hub.hasPendingApproval("dev-1")).toBe(false));
    expect(hub.cancelPendingApprovals("dev-1")).toBe(0);
    expect(approvals.size).toBe(0);
  });
});

// ── B0: session.list attribution (hub journal → engineBridge) ────────────────

describe("mobile dispatch — B0 session.list agent attribution", () => {
  function tmpRosterDir(): string {
    const dir = mkdtempSync(join(tmpdir(), "gw-dispatch-sess-"));
    mkdirSync(dir, { recursive: true });
    writeFileSync(join(dir, "backend.toml"), 'name = "backend-dev"', "utf8");
    return dir;
  }

  /** A call-capable fake engine answering `sessions.list` with a fixed snapshot. */
  function sessionsListEngine(sessions: unknown[]): any {
    return {
      connect: vi.fn(async () => {}),
      close: vi.fn(async () => {}),
      cancel: vi.fn(() => {}),
      async *runQuery(): AsyncGenerator<EngineEvent> {},
      call: vi.fn(async (_message: unknown, match: (frame: unknown) => unknown) => {
        const matched = match({ type: "sessions.snapshot", sessions });
        if (matched == null) throw new Error("fake engine produced an unmatched frame");
        return matched;
      }),
    };
  }

  it("session.list fills agent_id on task sessions from the hub journal; unknown/foreign sessions stay unattributed", async () => {
    const dir = tmpRosterDir();
    try {
      const hub = new MobileDispatchHub({ logger });
      hub.setSubmit(async () => {});
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      // One attributed dispatch (roster agent) + one un-attributed dispatch.
      const owned: any = await handlers["shannon/task.dispatch"]!(
        { prompt: "thread", agent_id: "backend-dev" },
        ctx,
      );
      const plain: any = await handlers["shannon/task.dispatch"]!({ prompt: "plain" }, ctx);
      const taskId = owned.result.task.id as string;

      const bridge = createEngineHandlers({
        engineWsUrl: "ws://127.0.0.1:9",
        engineHttpBaseUrl: "http://engine",
        version: "test",
        logger,
        engineClientFactory: () =>
          sessionsListEngine([
            // The task thread (engine session = task id) — journal knows its agent.
            { session_id: taskId, title: "ship it" },
            // A session the journal has no attribution for → no agentId.
            { session_id: plain.result.task.id as string },
            { session_id: "desktop-only" },
            // Engine-supplied agent_id wins over the journal.
            { session_id: "engine-owned", agent_id: "engine-agent" },
          ]),
        taskAgentLookup: (sessionId) => hub.agentForSession(sessionId),
      });

      const list: any = await bridge["shannon/session.list"]!({}, ctx);
      expect(list.kind).toBe("result");
      expect(list.result.sessions).toEqual([
        { id: taskId, agentId: "backend-dev", title: "ship it" },
        { id: plain.result.task.id },
        { id: "desktop-only" },
        { id: "engine-owned", agentId: "engine-agent" },
      ]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("without taskAgentLookup the enrichment is absent (legacy shape unchanged)", async () => {
    const bridge = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine",
      version: "test",
      logger,
      engineClientFactory: () =>
        sessionsListEngine([{ session_id: "sess-1", title: "t" }]),
    });
    const list: any = await bridge["shannon/session.list"]!({}, fakeCtx("dev-1"));
    expect(list.result.sessions).toEqual([{ id: "sess-1", title: "t" }]);
  });
});

// ── P2-9: shannon/agent.state roster live-status pushes ──────────────────────

describe("mobile dispatch — P2-9 agent.state roster pushes", () => {
  /** A roster dir with ONE definition: backend-dev (role + model declared). */
  function tmpRosterDir(): string {
    const dir = mkdtempSync(join(tmpdir(), "gw-agent-state-"));
    mkdirSync(dir, { recursive: true });
    writeFileSync(
      join(dir, "backend.toml"),
      [
        'name = "backend-dev"',
        'description = "Backend development specialist"',
        'model = "claude-haiku-4-5"',
      ].join("\n"),
      "utf8",
    );
    return dir;
  }

  /** The per-push roster resolver the bootstrap wires (same read-per-push). */
  function rosterLookup(dir: string): (agentId: string) => ReturnType<typeof loadAgentRoster>[number] | null {
    return (agentId) => loadAgentRoster([dir]).find((a) => a.id === agentId) ?? null;
  }

  it("dispatch WITH agent_id pushes {agent: running, currentTask: prompt} — full roster map, seq-free — then idle at the terminal", async () => {
    const dir = tmpRosterDir();
    try {
      const hub = new MobileDispatchHub({ logger, rosterEntry: rosterLookup(dir) });
      let release!: () => void;
      const gate = new Promise<void>((r) => {
        release = r;
      });
      hub.setSubmit(() => gate); // hold the turn open so the running state sticks
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      const res: any = await handlers["shannon/task.dispatch"]!(
        { prompt: "ship it", agent_id: "backend-dev" },
        ctx,
      );
      expect(res.kind).toBe("result");

      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(1));
      // The FULL roster shape — the phone's consumer REPLACES the roster
      // entry with this map, so name/role/model must ride (camelCase
      // currentTask per agentFromMap, the only consumer).
      expect(agentStatesOf(ctx)[0]).toEqual({
        id: "backend-dev",
        name: "backend-dev",
        role: "Backend development specialist",
        model: "claude-haiku-4-5",
        status: "running",
        activity: [],
        currentTask: "ship it",
      });
      // Dedicated notification METHOD (not a shannon/event type), no seq —
      // the phone's LiveAgentsNotifier filters on the method and reads
      // params.agent; ephemeral roster state stays out of the replay ring.
      const frame = framesOf(ctx).find((f) => f.method === "shannon/agent.state")!;
      expect(Object.keys(frame)).toEqual(["jsonrpc", "method", "params"]);
      expect(frame.jsonrpc).toBe("2.0");
      expect(Object.keys(frame.params)).toEqual(["agent"]);
      // …and it is NOT riding the shannon/event stream.
      expect(eventsOf(ctx).some((e) => (e as any).agent?.id === "backend-dev")).toBe(false);

      // Terminal (the dispatch resolution's finishTask): idle, currentTask null.
      release();
      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(2));
      expect(agentStatesOf(ctx)[1]).toEqual({
        id: "backend-dev",
        name: "backend-dev",
        role: "Backend development specialist",
        model: "claude-haiku-4-5",
        status: "idle",
        activity: [],
        currentTask: null,
      });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("dispatch WITHOUT agent_id pushes no agent.state — 无归属归 host，roster 无行可动", async () => {
    const dir = tmpRosterDir();
    try {
      const hub = new MobileDispatchHub({ logger, rosterEntry: rosterLookup(dir) });
      hub.setSubmit(async () => {});
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      await handlers["shannon/task.dispatch"]!({ prompt: "host's own task" }, ctx);
      await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
      // The push machinery ran (query.started landed) but no roster row moved.
      expect(eventsOf(ctx).map((e) => e.type)).toContain("query.started");
      expect(agentStatesOf(ctx)).toEqual([]);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("turn-handler terminals over the real pipeline: completeActiveTask → running→idle, failActiveTask → running→idle", async () => {
    const dir = tmpRosterDir();
    try {
      const buildPipeline = (client: EngineWsClient) => {
        const hub = new MobileDispatchHub({ logger, rosterEntry: rosterLookup(dir) });
        const adapter = createMobileChannelAdapter({ hub });
        const registry = { get: (p: string) => (p === "mobile" ? adapter : undefined) } as any;
        const turnHandler: TurnHandler = createMobileTaskTurnHandler({
          hub,
          engineBaseUrl: "http://engine",
        });
        const router = new SessionRouter({ registry, clientFactory: () => client, turnHandler, logger });
        hub.setSubmit((inbound) => router.handleInbound(inbound));
        return hub;
      };

      const okHub = buildPipeline(mockEngineClient([textEvent("done"), { type: "completed", model: "m" } as EngineEvent]));
      const okCtx = fakeCtx("dev-ok");
      okHub.registerConnection(okCtx);
      const okHandlers = createTaskHandlers({ hub: okHub, agentRosterDirs: [dir] });
      await okHandlers["shannon/task.dispatch"]!({ prompt: "long task", agent_id: "backend-dev" }, okCtx);
      await vi.waitFor(() => expect(okHub.listTasks("dev-ok")[0]?.status).toBe("completed"));
      const okStates = agentStatesOf(okCtx);
      expect(okStates.map((s) => s.status)).toEqual(["running", "idle"]);
      expect(okStates[0].currentTask).toBe("long task");
      expect(okStates[1].currentTask).toBeNull();

      const badHub = buildPipeline(mockEngineClient([textEvent("partial"), { type: "failed", error: "boom" } as EngineEvent]));
      const badCtx = fakeCtx("dev-bad");
      badHub.registerConnection(badCtx);
      const badHandlers = createTaskHandlers({ hub: badHub, agentRosterDirs: [dir] });
      await badHandlers["shannon/task.dispatch"]!({ prompt: "doomed", agent_id: "backend-dev" }, badCtx);
      await vi.waitFor(() => expect(badHub.listTasks("dev-bad")[0]?.status).toBe("failed"));
      const badStates = agentStatesOf(badCtx);
      expect(badStates.map((s) => s.status)).toEqual(["running", "idle"]);
      expect(badStates[1].currentTask).toBeNull();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("a queued same-agent task keeps the roster running until IT terminates (no idle blink between queue drains)", async () => {
    const dir = tmpRosterDir();
    try {
      const hub = new MobileDispatchHub({ logger, rosterEntry: rosterLookup(dir) });
      const gates: Array<() => void> = [];
      hub.setSubmit(() => new Promise<void>((r) => gates.push(r)));
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      await handlers["shannon/task.dispatch"]!({ prompt: "first", agent_id: "backend-dev" }, ctx);
      await handlers["shannon/task.dispatch"]!({ prompt: "second", agent_id: "backend-dev" }, ctx);
      // Both acceptances pushed running — each with its own task text.
      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(2));
      expect(agentStatesOf(ctx).map((s) => s.currentTask)).toEqual(["first", "second"]);

      // First terminal: the SAME agent still owns the queued second task —
      // running continues with the new currentTask, no idle blink.
      gates[0]!();
      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(3));
      expect(agentStatesOf(ctx)[2]).toMatchObject({ status: "running", currentTask: "second" });

      // Last terminal: nothing left under the agent — idle.
      gates[1]!();
      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(4));
      expect(agentStatesOf(ctx)[3]).toMatchObject({ status: "idle", currentTask: null });
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("an agent removed from the roster mid-task pushes NO terminal state (never ghost-add an unknown id)", async () => {
    const dir = tmpRosterDir();
    try {
      const hub = new MobileDispatchHub({ logger, rosterEntry: rosterLookup(dir) });
      let release!: () => void;
      const gate = new Promise<void>((r) => {
        release = r;
      });
      hub.setSubmit(() => gate);
      const handlers = createTaskHandlers({ hub, agentRosterDirs: [dir] });
      const ctx = fakeCtx("dev-1");
      hub.registerConnection(ctx);

      await handlers["shannon/task.dispatch"]!({ prompt: "ship it", agent_id: "backend-dev" }, ctx);
      await vi.waitFor(() => expect(agentStatesOf(ctx)).toHaveLength(1));

      // The definition file disappears mid-task; the resolver re-reads per
      // push, so the settle lookup misses — the push is skipped instead of
      // fabricating a roster row the phone would ADD.
      rmSync(join(dir, "backend.toml"));
      release();
      await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
      expect(agentStatesOf(ctx)).toHaveLength(1);
      expect(agentStatesOf(ctx)[0].status).toBe("running");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

// ── pipeline: dispatch → lane → §K3 task stream → phone pushes ────────────────

describe("mobile dispatch — §K3 structured task stream", () => {
  function buildPipeline(opts: {
    client: EngineWsClient;
    fetchImpl?: typeof fetch;
    approvalTimeoutMs?: number;
  }) {
    const hub = new MobileDispatchHub({ logger, approvalTimeoutMs: opts.approvalTimeoutMs });
    const adapter = createMobileChannelAdapter({ hub });
    const registry = {
      get: (platform: string) => (platform === "mobile" ? adapter : undefined),
    } as any;
    // Same composition the live bootstrap uses: platform "mobile" turns run
    // the §K3 task handler (IM platforms would get the lifecycle-wrapped one).
    const turnHandler: TurnHandler = createMobileTaskTurnHandler({
      hub,
      engineBaseUrl: "http://engine",
      fetchImpl: opts.fetchImpl,
    });
    const router = new SessionRouter({
      registry,
      clientFactory: () => opts.client,
      turnHandler,
      logger,
    });
    hub.setSubmit((inbound) => router.handleInbound(inbound));
    return { hub, adapter, router };
  }

  it("§K1 ordering: the dispatch response lands on the wire BEFORE the event stream", async () => {
    const client = mockEngineClient([textEvent("x"), { type: "completed", model: "m" } as EngineEvent]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    const taskId = res.result.task.id as string;
    // Still inside the dispatch handler's microtask chain — the acceptance
    // push is deferred to setImmediate precisely so the response (which the
    // phone keys its thread by) is written first.
    expect(eventsOf(ctx)).toEqual([]);

    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    const events = eventsOf(ctx);
    expect(events[0]).toMatchObject({ type: "query.started", session_id: taskId });
    expect(events[events.length - 1]).toMatchObject({ type: "task.message", session_id: taskId });
  });

  it("streams query.started → task.progress → task.message(session_id=task id) to the initiating device ONLY", async () => {
    const client = mockEngineClient([
      textEvent("hello "),
      textEvent("world"),
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const { hub } = buildPipeline({ client });
    const initiator = fakeCtx("dev-1");
    const bystander = fakeCtx("dev-2");
    hub.registerConnection(initiator);
    hub.registerConnection(bystander);
    const handlers = createTaskHandlers({ hub });

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, initiator);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));

    const events = eventsOf(initiator);
    expect(events.map((e) => e.type)).toEqual([
      "query.started",
      "task.progress",
      "task.progress",
      "task.message",
    ]);
    expect(events[0]).toMatchObject({ turn_id: expect.any(String), session_id: taskId });
    expect(events[1]).toMatchObject({ session_id: taskId, content: "hello " });
    expect(events[2]).toMatchObject({ session_id: taskId, content: "world" });
    // The terminal carries the FINAL COMPLETE reply and closes the stream.
    expect(events[3]).toMatchObject({ session_id: taskId, text: "hello world" });
    // §K3: no IM lifecycle stamps, no query.completed on a task stream.
    expect(events.some((e) => e.type === "query.completed")).toBe(false);
    expect(
      events.some((e) => e.type === "task.message" && /^[🚀✅❌]/.test(e.text || "")),
    ).toBe(false);
    // 仅推给发起设备 — the bystander's socket heard nothing.
    expect(eventsOf(bystander)).toEqual([]);
  });

  it("usage frames ride task.progress with the task's session_id (spend keyed by task thread)", async () => {
    const client = mockEngineClient([
      textEvent("result"),
      { type: "usage", input_tokens: 10, output_tokens: 5, cost_usd: 0.01 } as EngineEvent,
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));

    const usage = eventsOf(ctx).find((e) => e.type === "task.progress" && e.usage);
    expect(usage).toMatchObject({
      session_id: taskId,
      usage: { input_tokens: 10, output_tokens: 5, cost_usd: 0.01 },
    });
  });

  it("approval: request reaches the phone; the decide settle unblocks the lane and forwards the choice", async () => {
    const client = mockEngineClient([
      {
        type: "approval_request",
        request_id: "req-1",
        tool_name: "Bash",
        tool_input: { command: "rm -rf /tmp/x" },
        description: "删除临时目录",
        is_destructive: true,
        diff_preview: null,
      } as EngineEvent,
      textEvent("done"),
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const posts: Array<{ url: string; body: any }> = [];
    const fetchImpl = (async (url: any, init?: any) => {
      posts.push({ url: String(url), body: JSON.parse(init?.body ?? "{}") });
      return { ok: true, status: 200 } as unknown as Response;
    }) as unknown as typeof fetch;

    const { hub } = buildPipeline({ client, fetchImpl });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    const dispatchRes: any = await handlers["shannon/task.dispatch"]!({ prompt: "清理临时目录" }, ctx);
    expect(dispatchRes.result.task.status).toBe("running");

    // The approval request reaches the phone as a structured event.
    await vi.waitFor(() => {
      expect(eventsOf(ctx).some((e) => e.type === "approval.request" && e.request_id === "req-1")).toBe(true);
    });
    const approval = eventsOf(ctx).find((e) => e.type === "approval.request")!;
    expect(approval).toMatchObject({
      request_id: "req-1",
      tool_name: "Bash",
      description: "删除临时目录",
      is_destructive: true,
    });
    expect(hub.hasPendingApproval("dev-1")).toBe(true);

    // The phone answers via signed shannon/approval/decide; the bootstrap
    // wires that decision into hub.settleApproval — replay it here.
    expect(hub.settleApproval("req-1", "allow")).toBe(true);

    // The turn handler forwards the decision to the engine HTTP API.
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]!.url).toBe("http://engine/api/approval/respond");
    // Gateway choice "allow" maps to the engine wire enum "allow_once" (one-shot).
    expect(posts[0]!.body).toEqual({ request_id: "req-1", choice: "allow_once" });

    // The turn then completes: journal + §K3 terminal line up.
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
    const events = eventsOf(ctx).map((e) => e.type);
    expect(events).toEqual(["query.started", "approval.request", "task.progress", "task.message"]);
    const terminal = eventsOf(ctx).find((e) => e.type === "task.message")!;
    expect(terminal).toMatchObject({ text: "done", session_id: hub.listTasks("dev-1")[0]!.task_id });
  });

  it("approval: the decide settle with deny forwards deny to the engine", async () => {
    const client = mockEngineClient([
      {
        type: "approval_request",
        request_id: "req-2",
        tool_name: "Write",
        tool_input: { path: "/etc/passwd" },
        description: "写系统文件",
        is_destructive: false,
        diff_preview: null,
      } as EngineEvent,
      textEvent("ok"),
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const posts: Array<{ url: string; body: any }> = [];
    const fetchImpl = (async (url: any, init?: any) => {
      posts.push({ url: String(url), body: JSON.parse(init?.body ?? "{}") });
      return { ok: true, status: 200 } as unknown as Response;
    }) as unknown as typeof fetch;

    const { hub } = buildPipeline({ client, fetchImpl });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    void (await handlers["shannon/task.dispatch"]!({ prompt: "改系统文件" }, ctx));
    await vi.waitFor(() => expect(hub.hasPendingApproval("dev-1")).toBe(true));

    expect(hub.settleApproval("req-2", "deny")).toBe(true);
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]!.body).toEqual({ request_id: "req-2", choice: "deny" });
  });

  it("approval times out to deny when the phone never answers", async () => {
    const client = mockEngineClient([
      {
        type: "approval_request",
        request_id: "req-3",
        tool_name: "Bash",
        tool_input: {},
        description: "无人应答",
        is_destructive: false,
        diff_preview: null,
      } as EngineEvent,
      { type: "completed", model: "m" } as EngineEvent,
    ]);
    const posts: Array<{ body: any }> = [];
    const fetchImpl = (async (_url: any, init?: any) => {
      posts.push({ body: JSON.parse(init?.body ?? "{}") });
      return { ok: true, status: 200 } as unknown as Response;
    }) as unknown as typeof fetch;

    const { hub } = buildPipeline({ client, fetchImpl, approvalTimeoutMs: 25 });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    void (await handlers["shannon/task.dispatch"]!({ prompt: "needs approval" }, ctx));
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]!.body).toEqual({ request_id: "req-3", choice: "deny" });
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
  });

  it("B0: the task turn's engine session IS the task's UUID (engine rejects non-UUID session ids)", async () => {
    const client = mockEngineClient([textEvent("done"), { type: "completed", model: "m" } as EngineEvent]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "hi" }, ctx);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    // The turn's runQuery carried the task UUID as the engine session_id —
    // NOT the lane default (`mobile:<deviceId>`), which the engine's WS gate
    // (Uuid::parse_str) would reject with an error frame.
    expect(client.runQuery).toHaveBeenCalledWith(
      "hi",
      expect.objectContaining({ sessionId: taskId }),
    );
    expect(taskId).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("an engine failure closes the stream with query.failed(session_id) and flips the journal — no ❌ bubble", async () => {
    const client = mockEngineClient([
      textEvent("partial"),
      { type: "failed", error: "模型超时" } as EngineEvent,
    ]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    const res: any = await handlers["shannon/task.dispatch"]!({ prompt: "会失败的任务" }, ctx);
    const taskId = res.result.task.id as string;
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({
        status: "failed",
        error: "模型超时",
      }),
    );
    const events = eventsOf(ctx);
    expect(events.map((e) => e.type)).toEqual(["query.started", "task.progress", "query.failed"]);
    expect(events[2]).toMatchObject({ session_id: taskId, error: "模型超时" });
    // §K3: no lifecycle stamp and no reply bubble on the task stream.
    expect(events.some((e) => e.type === "task.message")).toBe(false);
  });
});

// ── transport: page serving + real-WS end-to-end through bootstrap ───────────

describe("mobile dispatch — server page + bootstrap end-to-end", () => {
  it("serves the PWA page on GET /", async () => {
    const server = new MobileServer({
      host: "127.0.0.1",
      port: 0,
      logger,
      handlers: {},
    });
    const handle = await server.start();
    try {
      const res = await fetch(`http://127.0.0.1:${handle.port}/`);
      expect(res.status).toBe(200);
      const html = await res.text();
      expect(html).toContain("shannon/task.dispatch");
      expect(html).toContain("shannon/pair");
      expect(html).toContain("shannon/approval/decide");
      expect(html).toContain("移动派发");
      // The vendored signer ships inside the page.
      expect(html).toContain("nacl.sign");
      // Unknown paths 404.
      const miss = await fetch(`http://127.0.0.1:${handle.port}/nope`);
      expect(miss.status).toBe(404);
    } finally {
      await server.stop();
    }
  });

  it("end-to-end over a real socket: pair → dispatch {prompt} → §K3 stream → signed decide → §K terminal", async () => {
    const posts: Array<{ body: any }> = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: any, init?: any) => {
        posts.push({ body: JSON.parse(init?.body ?? "{}") });
        return { ok: true, status: 200 } as unknown as Response;
      }),
    );
    const dir = mkdtempSync(join(tmpdir(), "gw-dispatch-"));
    const tokensFile = join(dir, "tokens.jsonl");
    const devicesFile = join(dir, "devices.json");

    // The desktop appends a one-time token (Design D control channel).
    const kp = generateEd25519KeyPair();
    const token = "e2e-test-token";
    const record = { token, issuedAt: Date.now(), expiresAt: Date.now() + 75_000 };
    writeFileSync(tokensFile, JSON.stringify(record) + "\n", "utf8");

    // The engine parks on the approval until the decision lands.
    const client = mockEngineClient([
      {
        type: "approval_request",
        request_id: "req-e2e",
        tool_name: "Bash",
        tool_input: { command: "echo hi" },
        description: "运行命令",
        is_destructive: false,
        diff_preview: null,
      } as unknown as EngineEvent,
      textEvent("all done"),
      { type: "completed", model: "m" } as unknown as EngineEvent,
    ]);

    const handle = await bootstrap(
      {
        engine: { wsUrl: "ws://mock/api/ws", httpBaseUrl: "http://mock" },
        adapters: [],
        mobile: { enabled: true, host: "127.0.0.1", port: 0, tokensFile, devicesFile },
      },
      {
        factories: new Map(),
        engineClientFactory: () => client,
        logger,
      },
    );
    const port = handle.mobilePort!;
    const deviceId = deviceIdFromPublicKey(kp.publicKeyB64Url);

    const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
    await new Promise<void>((resolve, reject) => {
      socket.once("open", () => resolve());
      socket.once("error", reject);
    });

    const rpc = (method: string, params: unknown): Promise<any> =>
      new Promise((resolve, reject) => {
        const id = Math.floor(Math.random() * 1e9);
        const onMsg = (data: unknown): void => {
          const msg = JSON.parse(String(data)) as any;
          if (msg.id === id) {
            socket.off("message", onMsg);
            if (msg.error) reject(new Error(msg.error.message));
            else resolve(msg.result);
          }
        };
        socket.on("message", onMsg);
        socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      });

    const notifications: any[] = [];
    socket.on("message", (data: unknown) => {
      const msg = JSON.parse(String(data)) as any;
      if (msg.method === "shannon/event") notifications.push(msg.params);
    });

    try {
      // Pair with a one-time token + proof-of-possession.
      const pop = signMessage(kp.privateKey, pairPopMessage(token, kp.publicKeyB64Url));
      const pairRes = await rpc("shannon/pair", {
        pair_token: token,
        device_public_key: kp.publicKeyB64Url,
        pop_signature: pop,
        device_label: "vitest phone",
      });
      expect(pairRes.device_id).toBe(deviceId);

      // 派发：{prompt} → the §K task object, synchronously.
      const dispatchRes = await rpc("shannon/task.dispatch", { prompt: "deploy the staging env" });
      expect(dispatchRes.task).toMatchObject({
        prompt: "deploy the staging env",
        status: "running",
        agent_id: null,
      });
      const taskId = dispatchRes.task.id as string;
      expect(taskId.length).toBeGreaterThan(0);
      expect(new Date(dispatchRes.task.created_at).toISOString()).toBe(dispatchRes.task.created_at);

      // §K3 #1: query.started announcing the task's own session key.
      await vi.waitFor(() =>
        expect(notifications.some((n) => n.type === "query.started" && n.session_id === taskId)).toBe(true),
      );

      // 审批：request pushed; the phone decides via the SIGNED decide RPC.
      await vi.waitFor(() =>
        expect(notifications.some((n) => n.type === "approval.request" && n.request_id === "req-e2e")).toBe(true),
      );
      const ts = Date.now();
      const decideSig = signMessage(kp.privateKey, `req-e2e:allow:${ts}`);
      const answer = await rpc("shannon/approval/decide", {
        request_id: "req-e2e",
        choice: "allow",
        signature: decideSig,
        timestamp: ts,
      });
      expect(answer).toEqual({ ok: true });

      // The decision reaches the engine (decide's own POST + the unblocked
      // turn handler's POST — both carry the same allow_once choice).
      await vi.waitFor(() => expect(posts.length).toBeGreaterThanOrEqual(1));
      for (const p of posts) {
        expect(p.body).toEqual({ request_id: "req-e2e", choice: "allow_once" });
      }

      // §K3 terminal: task.message(session_id = task id) with the full reply.
      await vi.waitFor(() =>
        expect(
          notifications.some((n) => n.type === "task.message" && n.session_id === taskId && n.text === "all done"),
        ).toBe(true),
      );

      // 看任务：journal shows the completed task in the §K2 + B1a shape.
      const list = await rpc("shannon/task.list", { limit: 10 });
      expect(list.tasks).toHaveLength(1);
      expect(list.tasks[0]).toEqual({
        id: taskId,
        prompt: "deploy the staging env",
        status: "completed",
        agent_id: null,
        created_at: expect.any(String),
        // B1a additive: title rides (non-empty), finished_at once terminal.
        title: "deploy the staging env",
        finished_at: expect.any(String),
      });

      // 未配对设备拒绝：a second, unpaired connection can't dispatch or list.
      const stranger = new WebSocket(`ws://127.0.0.1:${port}/`);
      await new Promise<void>((resolve) => stranger.once("open", () => resolve()));
      const strangerRpc = (method: string, params: unknown): Promise<any> =>
        new Promise((resolve) => {
          const id = Math.floor(Math.random() * 1e9);
          const onMsg = (data: unknown): void => {
            const msg = JSON.parse(String(data)) as any;
            if (msg.id === id) {
              stranger.off("message", onMsg);
              resolve(msg);
            }
          };
          stranger.on("message", onMsg);
          stranger.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
        });
      const denied = await strangerRpc("shannon/task.dispatch", { prompt: "sneak in" });
      expect(denied.error?.code).toBe(-32000);
      const deniedList = await strangerRpc("shannon/task.list", {});
      expect(deniedList.error?.code).toBe(-32000);
      stranger.close();
    } finally {
      socket.close();
      await handle.stop();
      vi.unstubAllGlobals();
      rmSync(dir, { recursive: true, force: true });
    }
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });
});

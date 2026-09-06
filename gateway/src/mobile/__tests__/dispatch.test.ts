import { EventEmitter } from "node:events";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
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
import { formatTaskCompleted, formatTaskStarted } from "../../router/lifecycle.js";
import { createApprovalTurnHandler } from "../../router/approvalTurnHandler.js";
import { SessionRouter } from "../../router/router.js";
import { withTaskLifecycle } from "../../router/lifecycle.js";
import { createMobileChannelAdapter } from "../channel.js";
import {
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  signMessage,
} from "../crypto.js";
import { MobileDispatchHub } from "../hub.js";
import { createTaskHandlers } from "../taskHandlers.js";
import { MobileServer, type MethodContext } from "../server.js";

/**
 * P2-1 acceptance: the four mobile dispatch actions over the T9 pipeline —
 * 派发 (text → task), 看任务 (task.list), 审批 (Y/N → engine decision),
 * 进度推送 (started/completed/failed stamps to the phone channel) — plus the
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

function mockEngineClient(events: EngineEvent[]): EngineWsClient {
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
  it("parseApprovalChoice shares the DingTalk Y/N dialect", () => {
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

    const dispatch = await handlers["shannon/task.dispatch"]!({ text: "hi" }, ctx);
    expect(dispatch).toMatchObject({ kind: "error", code: -32000 });

    const list = await handlers["shannon/task.list"]!({}, ctx);
    expect(list).toMatchObject({ kind: "error", code: -32000 });
  });

  it("rejects empty dispatch text with BAD_PARAMS", async () => {
    const hub = new MobileDispatchHub({ logger });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    const res = await handlers["shannon/task.dispatch"]!({ text: "   " }, ctx);
    expect(res).toMatchObject({ kind: "error", code: -32001 });
  });

  it("dispatch creates a task, fabricates a direct inbound, and journals it", async () => {
    const hub = new MobileDispatchHub({ logger });
    const seen: NormalizedInbound[] = [];
    hub.setSubmit(async (inbound) => {
      seen.push(inbound);
    });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);

    const res = await handlers["shannon/task.dispatch"]!({ text: "部署 staging 环境" }, ctx);
    expect(res).toMatchObject({ kind: "result", result: { ok: true, kind: "task" } });
    const taskId = (res as any).result.task_id as string;
    expect(taskId).toBeTruthy();

    expect(seen).toHaveLength(1);
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

  it("task.list returns the device's own tasks only, newest first, capped", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {});
    const mine = fakeCtx("dev-1");
    const other = fakeCtx("dev-2");
    hub.registerConnection(mine);
    hub.registerConnection(other);
    const handlers = createTaskHandlers({ hub });

    await handlers["shannon/task.dispatch"]!({ text: "mine first" }, mine);
    await handlers["shannon/task.dispatch"]!({ text: "mine second" }, mine);
    await handlers["shannon/task.dispatch"]!({ text: "other device" }, other);

    const res = (await handlers["shannon/task.list"]!({ limit: 10 }, mine)) as any;
    expect(res.result.tasks.map((t: any) => t.text)).toEqual(["mine second", "mine first"]);
    expect(res.result.tasks[0].device_id).toBe("dev-1");
    // Wire shape is snake_case for the phone client.
    expect(Object.keys(res.result.tasks[0])).toEqual(
      expect.arrayContaining(["task_id", "device_id", "title", "text", "status", "started_at", "finished_at", "error"]),
    );
  });

  it("a failed turn lands in the journal as failed", async () => {
    const hub = new MobileDispatchHub({ logger });
    hub.setSubmit(async () => {
      throw new Error("engine exploded");
    });
    const handlers = createTaskHandlers({ hub });
    const ctx = fakeCtx("dev-1");
    await handlers["shannon/task.dispatch"]!({ text: "doomed task" }, ctx);
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({ status: "failed", error: "engine exploded" }),
    );
  });

  it("adapter.send throws when the device has no open connection", async () => {
    const hub = new MobileDispatchHub({ logger });
    const adapter = createMobileChannelAdapter({ hub });
    await expect(adapter.send({ platform: "mobile", chatId: "ghost" }, "hello")).rejects.toThrow(
      /no connected device/,
    );
  });
});

// ── pipeline: dispatch → lane → approval/lifecycle → phone pushes ────────────

describe("mobile dispatch — pipeline with lifecycle + approval", () => {
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
    const turnHandler = withTaskLifecycle(
      createApprovalTurnHandler({ engineBaseUrl: "http://engine", fetchImpl: opts.fetchImpl }),
    );
    const router = new SessionRouter({
      registry,
      clientFactory: () => opts.client,
      turnHandler,
      logger,
    });
    hub.setSubmit((inbound) => router.handleInbound(inbound));
    return { hub, adapter, router };
  }

  it("progress push: started / answer / completed reach the phone", async () => {
    const client = mockEngineClient([textEvent("hello world"), { type: "completed", model: "m" } as EngineEvent]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    await handlers["shannon/task.dispatch"]!({ text: "hi" }, ctx);
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));

    const texts = eventsOf(ctx)
      .filter((e) => e.type === "task.message")
      .map((e) => e.text);
    expect(texts).toEqual([
      formatTaskStarted("hi"),
      "hello world",
      formatTaskCompleted("hi"),
    ]);
  });

  it("approval: request pushed to phone, text Y resolves, decision forwarded to engine", async () => {
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

    const dispatchRes: any = await handlers["shannon/task.dispatch"]!({ text: "清理临时目录" }, ctx);
    expect(dispatchRes.result.kind).toBe("task");

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

    // The phone answers with a Y text through the same dispatch method.
    const answerRes: any = await handlers["shannon/task.dispatch"]!({ text: "y" }, ctx);
    expect(answerRes.result).toMatchObject({ ok: true, kind: "approval", choice: "allow", task_id: null });

    // The turn handler forwards the decision to the engine HTTP API.
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]!.url).toBe("http://engine/api/approval/respond");
    // Gateway choice "allow" maps to the engine wire enum "allow_once" (one-shot).
    expect(posts[0]!.body).toEqual({ request_id: "req-1", choice: "allow_once" });

    // The turn then completes and the failure-free journal + stamps line up.
    await vi.waitFor(() => expect(hub.listTasks("dev-1")[0]?.status).toBe("completed"));
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
  });

  it("approval: 拒绝 (deny) text forwards deny to the engine", async () => {
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

    void (await handlers["shannon/task.dispatch"]!({ text: "改系统文件" }, ctx));
    await vi.waitFor(() => expect(hub.hasPendingApproval("dev-1")).toBe(true));

    const answerRes: any = await handlers["shannon/task.dispatch"]!({ text: "拒绝" }, ctx);
    expect(answerRes.result.choice).toBe("deny");
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

    void (await handlers["shannon/task.dispatch"]!({ text: "needs approval" }, ctx));
    await vi.waitFor(() => expect(posts).toHaveLength(1));
    expect(posts[0]!.body).toEqual({ request_id: "req-3", choice: "deny" });
    expect(hub.hasPendingApproval("dev-1")).toBe(false);
  });

  it("an engine failure turns the journal entry failed via the ❌ lifecycle stamp", async () => {
    const client = mockEngineClient([
      textEvent("partial"),
      { type: "failed", error: "模型超时" } as EngineEvent,
    ]);
    const { hub } = buildPipeline({ client });
    const ctx = fakeCtx("dev-1");
    hub.registerConnection(ctx);
    const handlers = createTaskHandlers({ hub });

    await handlers["shannon/task.dispatch"]!({ text: "会失败的任务" }, ctx);
    await vi.waitFor(() =>
      expect(hub.listTasks("dev-1")[0]).toMatchObject({
        status: "failed",
        error: "模型超时",
      }),
    );
    // The phone saw the failure stamp too.
    const texts = eventsOf(ctx).filter((e) => e.type === "task.message").map((e) => e.text);
    expect(texts.some((t) => t.includes("❌ 任务失败") && t.includes("模型超时"))).toBe(true);
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

  it("end-to-end over a real socket: pair → dispatch → approval y → engine", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: any, init?: any) => {
        expect(JSON.parse(init?.body ?? "{}")).toEqual({ request_id: "req-e2e", choice: "allow_once" });
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

    // The engine parks on the approval until the phone answers.
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

      // 派发：text → task through the IM pipeline.
      const dispatchRes = await rpc("shannon/task.dispatch", { text: "deploy the staging env" });
      expect(dispatchRes).toMatchObject({ ok: true, kind: "task" });
      const taskId = dispatchRes.task_id as string;

      // 进度推送：started stamp arrives on the phone channel.
      await vi.waitFor(() =>
        expect(notifications.some((n) => n.type === "task.message" && n.text.startsWith("🚀"))).toBe(true),
      );

      // 审批：request pushed; answer y; decision forwarded (fetch stub asserts).
      await vi.waitFor(() =>
        expect(notifications.some((n) => n.type === "approval.request" && n.request_id === "req-e2e")).toBe(true),
      );
      const answer = await rpc("shannon/task.dispatch", { text: "y" });
      expect(answer).toMatchObject({ ok: true, kind: "approval", choice: "allow" });

      await vi.waitFor(() =>
        expect(notifications.some((n) => n.type === "task.message" && n.text.startsWith("✅"))).toBe(true),
      );

      // 看任务：journal shows the completed task.
      const list = await rpc("shannon/task.list", { limit: 10 });
      expect(list.tasks).toHaveLength(1);
      expect(list.tasks[0]).toMatchObject({
        task_id: taskId,
        device_id: deviceId,
        status: "completed",
        title: "deploy the staging env",
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
      const denied = await strangerRpc("shannon/task.dispatch", { text: "sneak in" });
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

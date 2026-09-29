import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { WebSocket } from "ws";
import { describe, expect, it, vi } from "vitest";

import {
  type AdapterCapabilities,
  type AdapterContext,
  type ApprovalDecision,
  type ApprovalReq,
  type ChannelAdapter,
  type Logger,
  type MessageReceipt,
  type NormalizedInbound,
  type ReplyTarget,
  type SessionConversation,
} from "../adapters/types.js";
import { type EngineEvent } from "../engine/runtime.js";
import { type EngineWsClient } from "../engine/wsClient.js";
import { type GatewayConfig } from "../config/types.js";
import { type InboundGuard } from "../access/guard.js";
import { bootstrap, type AdapterFactory } from "../bootstrap.js";

const noopLogger: Logger = {
  debug() {},
  info() {},
  warn() {},
  error() {},
};

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

interface MockAdapter extends ChannelAdapter {
  sent: Array<{ target: ReplyTarget; text: string }>;
  pushInbound(m: NormalizedInbound): void;
}

function mockAdapter(): MockAdapter {
  let onMsg: ((m: NormalizedInbound) => void) | null = null;
  const sent: MockAdapter["sent"] = [];
  const capabilities: AdapterCapabilities = {
    threading: false,
    pairing: false,
    approvalButtons: false,
    streaming: "none",
  };
  return {
    platform: "slack",
    capabilities,
    start: vi.fn(async () => {}),
    stop: vi.fn(async () => {}),
    onMessage: (h: (m: NormalizedInbound) => void) => {
      onMsg = h;
    },
    send: async (target: ReplyTarget, text: string): Promise<MessageReceipt> => {
      sent.push({ target, text });
      return { messageId: `m${sent.length}` };
    },
    requestApproval: vi.fn(
      async (_t: ReplyTarget, req: ApprovalReq): Promise<ApprovalDecision> => ({
        requestId: req.requestId,
        choice: "allow",
      }),
    ),
    resolveSessionConversation: (id: string): SessionConversation => ({ baseChatId: id }),
    sent,
    pushInbound(m: NormalizedInbound): void {
      onMsg?.(m);
    },
  } as unknown as MockAdapter;
}

const baseConfig: GatewayConfig = {
  engine: { wsUrl: "ws://mock/api/ws", httpBaseUrl: "http://mock" },
  adapters: [{ platform: "slack", enabled: true }],
};

/** Test guard that always allows — keeps legacy bootstrap tests focused on
 *  the trigger-gate / router / engine layers instead of access control. */
const alwaysAllowGuard: InboundGuard = {
  check: () => ({ decision: "allow" }),
};

describe("bootstrap", () => {
  it("wires inbound → trigger gate → router → engine → reply end-to-end", async () => {
    const adapter = mockAdapter();
    const factory: AdapterFactory = () => adapter;
    const client = mockEngineClient([
      { type: "text", content: "hello world" },
      { type: "completed", model: "mock" },
    ]);

    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", factory]]),
      engineClientFactory: () => client,
        accessGuard: alwaysAllowGuard,
      logger: noopLogger,
    });

    expect(handle.adapterCount).toBe(1);
    expect(adapter.start).toHaveBeenCalledTimes(1);

    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "U1",
      senderName: "ed",
      text: "hi",
      timestamp: Date.now(),
      isDirect: true, // DM → direct response under the P1-4 trigger policy
    });

    // onMessage is sync-void; the turn resolves asynchronously in the lane.
    await vi.waitFor(() => {
      expect(adapter.sent.length).toBeGreaterThan(0);
    });
    // P1-4 lifecycle stamps wrap the engine answer: start, answer, completed.
    expect(adapter.sent.map((s) => s.text)).toEqual([
      "🚀 已开始任务：hi",
      "hello world",
      "✅ 任务完成：hi",
    ]);
    expect(adapter.sent[1]?.target.chatId).toBe("C1");

    await handle.stop();
    expect(adapter.stop).toHaveBeenCalledTimes(1);
  });

  it("drops a group message with no mention/prefix (P1-4 trigger policy)", async () => {
    const adapter = mockAdapter();
    const client = mockEngineClient([{ type: "completed", model: "mock" }]);
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => client,
        accessGuard: alwaysAllowGuard,
      logger: noopLogger,
    });

    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "U1",
      senderName: "ed",
      text: "just chatting",
      timestamp: Date.now(),
      // isDirect absent → group; no <@…> mention, no /shannon prefix → ignored
    });
    await new Promise((r) => setTimeout(r, 20));
    expect(adapter.sent).toEqual([]);

    // The /shannon prefix arms it, and the prefix is stripped from the prompt.
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "U1",
      senderName: "ed",
      text: "/shannon 帮我写周报",
      timestamp: Date.now(),
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    expect(adapter.sent[0]?.text).toBe("🚀 已开始任务：帮我写周报");

    await handle.stop();
  });

  it("keeps the answer first when im.taskLifecycle is off (opt-out)", async () => {
    const adapter = mockAdapter();
    const client = mockEngineClient([
      { type: "text", content: "plain answer" },
      { type: "completed", model: "mock" },
    ]);
    const handle = await bootstrap(
      { ...baseConfig, im: { taskLifecycle: false } },
      {
        factories: new Map([["slack", () => adapter]]),
        engineClientFactory: () => client,
        accessGuard: alwaysAllowGuard,
        logger: noopLogger,
      },
    );

    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "U1",
      senderName: "ed",
      text: "hi",
      timestamp: Date.now(),
      isDirect: true,
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    expect(adapter.sent.map((s) => s.text)).toEqual(["plain answer"]);

    await handle.stop();
  });

  it("throws when an enabled adapter has no factory registered", async () => {
    await expect(bootstrap(baseConfig, { factories: new Map() })).rejects.toThrow(
      /no adapter factory.*slack/,
    );
  });

  it("skips disabled adapters", async () => {
    const cfg: GatewayConfig = {
      engine: { wsUrl: "ws://m/ws", httpBaseUrl: "http://m" },
      adapters: [{ platform: "slack", enabled: false }],
    };
    const handle = await bootstrap(cfg, {
      factories: new Map(),
      engineClientFactory: () => mockEngineClient([]),
      logger: noopLogger,
    });
    expect(handle.adapterCount).toBe(0);
    await handle.stop();
  });

  it("passes keyring secrets to the adapter via AdapterContext", async () => {
    const seen: AdapterContext[] = [];
    const adapter = mockAdapter();
    const factory: AdapterFactory = (_cfg, ctx) => {
      seen.push(ctx);
      return adapter;
    };
    const handle = await bootstrap(
      {
        engine: { wsUrl: "ws://m/ws", httpBaseUrl: "http://m" },
        adapters: [{ platform: "slack", enabled: true }],
      },
      {
        factories: new Map([["slack", factory]]),
        engineClientFactory: () => mockEngineClient([]),
        secretProvider: { get: async (k: string) => (k === "slack/bot-token" ? "tok" : null) },
        logger: noopLogger,
      },
    );
    expect(seen[0]?.getSecret).toBeTypeOf("function");
    expect(await seen[0]?.getSecret("slack/bot-token")).toBe("tok");
    expect(await seen[0]?.getSecret("missing/key")).toBeNull();
    await handle.stop();
  });

  it("starts the mobile shannon/* server when config.mobile.enabled and stops it", async () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-mobile-"));
    const cfg: GatewayConfig = {
      engine: { wsUrl: "ws://m/ws", httpBaseUrl: "http://m" },
      adapters: [],
      mobile: {
        enabled: true,
        host: "127.0.0.1",
        port: 0,
        tokensFile: join(dir, "tokens.jsonl"),
        devicesFile: join(dir, "devices.json"),
      },
    };
    // Health probe uses HTTP; stub it so no real engine is required.
    const fetchMock = vi.fn(async () => ({ status: 200, ok: true }) as unknown as Response);
    const handle = await bootstrap(cfg, {
      factories: new Map(),
      logger: noopLogger,
      mobileFetchImpl: fetchMock as unknown as typeof fetch,
    });

    const port = handle.mobilePort;
    expect(typeof port).toBe("number");
    expect((port ?? 0) > 0).toBe(true);

    // A real WS client can connect and call shannon/health (no pairing needed).
    const healthRes = await new Promise<any>((resolve, reject) => {
      const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
      socket.on("open", () => {
        socket.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "shannon/health" }));
      });
      socket.on("message", (d) => {
        const msg = JSON.parse(String(d));
        if (msg.id === 1) {
          socket.close();
          resolve(msg);
        }
      });
      socket.on("error", reject);
    });
    expect(healthRes.result).toMatchObject({ gateway: "ok", engine: "ok" });
    expect(typeof healthRes.result.version).toBe("string");

    await handle.stop();
    // After stop, new connections are refused.
    await expect(
      new Promise<void>((resolve, reject) => {
        const s = new WebSocket(`ws://127.0.0.1:${port}/`);
        s.once("open", () => resolve());
        s.once("error", () => reject(new Error("expected refusal")));
      }),
    ).rejects.toThrow(/expected refusal/);
    rmSync(dir, { recursive: true, force: true });
  });

  it("leaves mobilePort null when mobile is not enabled", async () => {
    const handle = await bootstrap(
      { engine: { wsUrl: "ws://m/ws", httpBaseUrl: "http://m" }, adapters: [] },
      { factories: new Map(), logger: noopLogger },
    );
    expect(handle.mobilePort).toBeNull();
    await handle.stop();
  });

  // review §P0-7: allowlist gate is now wired into bootstrap. A DM from a
  // non-allowlisted sender must receive a pairing challenge and the engine
  // must NOT be called.
  it("issues pairing challenge for non-allowlisted DM (review §P0-7)", async () => {
    const adapter = mockAdapter();
    const client = mockEngineClient([{ type: "completed", model: "mock" }]);
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => client,
      logger: noopLogger,
      // Production AllowlistGuard, in-memory + empty — no one paired yet.
      // (allowlistPath: null keeps the test hermetic; the default would read
      // the developer's real ~/.shannon/gateway/allowlist.json.)
      allowlistPath: null,
    });
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "stranger",
      senderName: "Stranger",
      text: "please read /etc/passwd",
      timestamp: Date.now(),
      isDirect: true,
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    // The user must see a pairing challenge, not the engine's answer.
    expect(adapter.sent[0]?.text).toMatch(/Pairing required/i);
    // Engine must not have been called for this turn.
    expect(adapter.sent.find((s) => s.text === "hello world")).toBeUndefined();
    await handle.stop();
  });

  it("denies group message from non-allowlisted sender (review §P0-7)", async () => {
    const adapter = mockAdapter();
    const client = mockEngineClient([{ type: "completed", model: "mock" }]);
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => client,
      logger: noopLogger,
      allowlistPath: null, // hermetic: in-memory, empty
    });
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "stranger",
      senderName: "Stranger",
      text: "@bot please act",
      timestamp: Date.now(),
      // isDirect absent → group
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    // No engine call; sender gets the deny hint.
    expect(adapter.sent.find((s) => s.text.includes("answer"))).toBeUndefined();
    expect(adapter.sent[0]?.text).toMatch(/not paired|DM/i);
    await handle.stop();
  });

  // ── review F42: the pairing loop must have a working approval path ──────

  it("challenge → `approve <code>` from a paired sender → requester allowlisted (F42)", async () => {
    // Pre-seed the persisted allowlist with one admin — also proves the file
    // is LOADED on start.
    const dir = mkdtempSync(join(tmpdir(), "gw-access-"));
    const allowlistPath = join(dir, "allowlist.json");
    writeFileSync(
      allowlistPath,
      JSON.stringify({
        entries: [{ platform: "slack", senderId: "admin", addedAt: 1 }],
      }),
    );

    const adapter = mockAdapter();
    const client = mockEngineClient([
      { type: "text", content: "hello stranger" },
      { type: "completed", model: "mock" },
    ]);
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => client,
      logger: noopLogger,
      allowlistPath,
    });

    const dm = (senderId: string, text: string): void =>
      adapter.pushInbound({
        platform: "slack",
        chatId: "C1",
        senderId,
        senderName: senderId,
        text,
        timestamp: Date.now(),
        isDirect: true,
      });

    // 1) Stranger DMs → pairing challenge naming the code + real channels.
    dm("stranger", "let me in");
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    const challenge = adapter.sent[0]?.text ?? "";
    expect(challenge).toMatch(/Pairing required/);
    expect(challenge).toMatch(/approve \d{6}/);
    expect(challenge).toContain(allowlistPath);
    const code = challenge.match(/approve (\d{6})/)?.[1];
    expect(code).toBeDefined();

    // 2) The paired admin replies `approve <code>` — intercepted before the
    // trigger gate, never reaches the engine.
    dm("admin", `approve ${code}`);
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(1));
    expect(adapter.sent[1]?.text).toMatch(/Paired/);

    // 3) Approval persisted to the allowlist file.
    const persisted = JSON.parse(readFileSync(allowlistPath, "utf8")) as {
      entries: Array<{ platform: string; senderId: string }>;
    };
    expect(persisted.entries).toContainEqual({
      platform: "slack",
      senderId: "stranger",
      addedAt: expect.any(Number),
    });

    // 4) The stranger can now drive the agent.
    dm("stranger", "hi again");
    await vi.waitFor(() =>
      expect(adapter.sent.some((s) => s.text === "hello stranger")).toBe(true),
    );

    await handle.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  it("rejects `approve <code>` from an unpaired sender (F42, no self-approval)", async () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-access-"));
    const allowlistPath = join(dir, "allowlist.json");
    const adapter = mockAdapter();
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => mockEngineClient([]),
      logger: noopLogger,
      allowlistPath,
    });
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "stranger",
      senderName: "Stranger",
      text: "approve 123456",
      timestamp: Date.now(),
      isDirect: true,
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    // Not allowlisted → their "approve" is just another unpaired message: they
    // get a fresh challenge, never an approval.
    expect(adapter.sent[0]?.text).toMatch(/Pairing required/);
    expect(adapter.sent[0]?.text).not.toContain("123456");
    await handle.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  it("challenge → desktop approves over the pairing HTTP RPC (T9) → requester allowlisted", async () => {
    // The desktop leg of review F42: instead of an IM `approve <code>` reply,
    // the owner approves from the Shannon desktop app. The desktop authenticates
    // with a pair token minted into the shared JSONL (Design D control channel).
    const dir = mkdtempSync(join(tmpdir(), "gw-access-"));
    const allowlistPath = join(dir, "allowlist.json");
    const tokensFile = join(dir, "tokens.jsonl");
    writeFileSync(
      allowlistPath,
      JSON.stringify({
        entries: [{ platform: "slack", senderId: "admin", addedAt: 1 }],
      }),
    );
    const adapter = mockAdapter();
    const cfg: GatewayConfig = {
      ...baseConfig,
      mobile: {
        enabled: true,
        host: "127.0.0.1",
        port: 0,
        tokensFile,
        devicesFile: join(dir, "devices.json"),
      },
    };
    const handle = await bootstrap(cfg, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () =>
        mockEngineClient([{ type: "text", content: "hello from stranger" }]),
      logger: noopLogger,
      allowlistPath,
    });
    const port = handle.mobilePort;
    expect(typeof port).toBe("number");
    const base = `http://127.0.0.1:${port}`;

    // 1) Stranger DMs → challenge with a code (the IM path, unchanged).
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "stranger",
      senderName: "Stranger",
      text: "let me in",
      timestamp: Date.now(),
      isDirect: true,
    });
    await vi.waitFor(() => expect(adapter.sent.length).toBeGreaterThan(0));
    const code = (adapter.sent[0]?.text.match(/approve (\d{6})/)?.[1] ?? "") as string;
    expect(code).toMatch(/^\d{6}$/);

    // 2) The desktop mints one-time pair tokens into the shared JSONL — the
    // exact record shape `mobile_generate_pair_token` appends (one per call:
    // pending verifies, approve consumes).
    const mint = (token: string): void => {
      writeFileSync(
        tokensFile,
        JSON.stringify({ token, issuedAt: Date.now(), expiresAt: Date.now() + 75_000 }) + "\n",
        { flag: "a" },
      );
    };
    mint("test-desktop-token-pending");
    mint("test-desktop-token-approve");

    // 3) pending lists the challenge.
    const pendingRes = await fetch(`${base}/rpc/pairing/pending`, {
      method: "POST",
      body: JSON.stringify({ token: "test-desktop-token-pending" }),
    });
    expect(pendingRes.status).toBe(200);
    const pendingBody = (await pendingRes.json()) as {
      result: { pending: Array<Record<string, unknown>> };
    };
    expect(pendingBody.result.pending).toContainEqual({
      code,
      platform: "slack",
      senderId: "stranger",
      requestedAt: expect.any(Number),
      expiresAt: expect.any(Number),
    });

    // 4) approve consumes the code (its own token) and persists the allowlist.
    const approveRes = await fetch(`${base}/rpc/pairing/approve`, {
      method: "POST",
      body: JSON.stringify({ token: "test-desktop-token-approve", code }),
    });
    expect(approveRes.status).toBe(200);
    const approveBody = (await approveRes.json()) as {
      result: { ok: true; record: { senderId: string } };
    };
    expect(approveBody.result.record.senderId).toBe("stranger");
    const persisted = JSON.parse(readFileSync(allowlistPath, "utf8")) as {
      entries: Array<{ platform: string; senderId: string }>;
    };
    expect(persisted.entries).toContainEqual({
      platform: "slack",
      senderId: "stranger",
      addedAt: expect.any(Number),
    });

    // 5) The stranger can now drive the agent (guard sees the entry).
    adapter.pushInbound({
      platform: "slack",
      chatId: "C1",
      senderId: "stranger",
      senderName: "Stranger",
      text: "hi again",
      timestamp: Date.now(),
      isDirect: true,
    });
    await vi.waitFor(() =>
      expect(adapter.sent.some((s) => s.text === "hello from stranger")).toBe(true),
    );

    await handle.stop();
    rmSync(dir, { recursive: true, force: true });
  });

  // ── review F40: a failed turn must not crash or wedge the gateway ───────

  it("contains a failed turn and keeps serving later turns (F40)", async () => {
    const adapter = mockAdapter();
    let calls = 0;
    const failingHandler = {
      async handle(): Promise<void> {
        calls += 1;
        if (calls === 1) throw new Error("engine socket closed mid-stream");
      },
    };
    const handle = await bootstrap(baseConfig, {
      factories: new Map([["slack", () => adapter]]),
      engineClientFactory: () => mockEngineClient([]),
      turnHandler: failingHandler,
      accessGuard: alwaysAllowGuard,
      logger: noopLogger,
    });

    // Fail the FIRST turn via an unhandled-rejection tripwire: before the fix
    // this rejection escaped `void router.handleInbound(...)` and crashed the
    // process.
    const rejections: unknown[] = [];
    const onRejection = (reason: unknown): void => {
      rejections.push(reason);
    };
    process.on("unhandledRejection", onRejection);
    try {
      const dm = (text: string): void =>
        adapter.pushInbound({
          platform: "slack",
          chatId: "C1",
          senderId: "U1",
          senderName: "ed",
          text,
          timestamp: Date.now(),
          isDirect: true,
        });

      dm("first");
      await new Promise((r) => setTimeout(r, 30)); // let the rejection surface if it will
      dm("second");
      await vi.waitFor(() => expect(calls).toBe(2));

      expect(rejections).toEqual([]);
      // Both turns ran on the same lane — the failed one didn't wedge it and
      // the process kept serving (calls === 2 is the whole point).
      expect(calls).toBe(2);
    } finally {
      process.off("unhandledRejection", onRejection);
    }
    await handle.stop();
  });
});

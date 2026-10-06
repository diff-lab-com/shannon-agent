/**
 * §O groundwork acceptance — `shannon/push.register` (§O2) + the hub wake
 * trigger seam (§O3).
 *
 * Contract source: shannon-mobile `cross-repo-adaptation-spec.md` §O2/§O3.
 * The face exists before any relay repo does: without a binding sink it must
 * answer a STRUCTURED error (the phone's honest "推送不可用" state), never a
 * mocked success. The wake seam is inert until the §O batch wires the
 * desktop→relay leg.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import { createSeqCounter } from "../seq.js";
import { PushReplayBuffer } from "../pushReplay.js";
import { MobileDispatchHub } from "../hub.js";
import { createMobileHandlers, DeviceRegistry, PairTokenStore } from "../pairing.js";
import type { PushBindingSink, PushUnbindSink } from "../engineBridge.js";
import { MobileServer, type MethodContext, type MethodHandlers } from "../server.js";
import {
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  signMessage,
} from "../crypto.js";

const logger = createConsoleLogger("error");
const servers: { stop: () => Promise<void> }[] = [];

afterEach(async () => {
  while (servers.length) await servers.pop()!.stop();
  vi.restoreAllMocks();
});

async function start(handlers: MethodHandlers): Promise<{ port: number }> {
  const server = new MobileServer({ host: "127.0.0.1", port: 0, logger, handlers });
  const handle = await server.start();
  servers.push(handle);
  return { port: handle.port };
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
      const msg = JSON.parse(String(data)) as { id?: number; result?: any; error?: any };
      if (msg.id === id) {
        socket.off("message", onMessage);
        resolve(msg.error ? { __error: msg.error } : msg.result);
      }
    };
    socket.on("message", onMessage);
    socket.on("error", reject);
    socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
  });
}

function handlers(opts: {
  tokens: PairTokenStore;
  registry: DeviceRegistry;
  pushBindingSink?: PushBindingSink;
  pushUnbindSink?: PushUnbindSink;
}): MethodHandlers {
  return createMobileHandlers({
    engine: {
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      pushBindingSink: opts.pushBindingSink,
      pushUnbindSink: opts.pushUnbindSink,
    },
    tokens: opts.tokens,
    registry: opts.registry,
    logger,
  });
}

async function pairDevice(
  socket: WebSocket,
  tokens: PairTokenStore,
  kp: ReturnType<typeof generateEd25519KeyPair>,
): Promise<string> {
  const rec = tokens.issue();
  await rpc(socket, "shannon/pair", {
    pair_token: rec.token,
    device_public_key: kp.publicKeyB64Url,
    pop_signature: signMessage(kp.privateKey, pairPopMessage(rec.token, kp.publicKeyB64Url)),
    device_label: "push test",
  });
  return deviceIdFromPublicKey(kp.publicKeyB64Url);
}

// ── §O2: shannon/push.register ──────────────────────────────────────────────

describe("shannon/push.register (§O2)", () => {
  it("gates on pairing like every other mutating RPC", async () => {
    const { port } = await start(handlers({ tokens: new PairTokenStore(), registry: new DeviceRegistry() }));
    const stranger = await connect(port);
    const res = await rpc(stranger, "shannon/push.register", {
      enable: true,
      platform: "fcm",
      token: "tok",
    });
    expect(res.__error.code).toBe(-32000); // PAIRING_REQUIRED
    stranger.close();
  });

  it("validates params: platform, token, enable", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);
    await pairDevice(socket, tokens, generateEd25519KeyPair());

    const badPlatform = await rpc(socket, "shannon/push.register", { enable: true, platform: "apns2", token: "t" });
    expect(badPlatform.__error.code).toBe(-32001);
    const noToken = await rpc(socket, "shannon/push.register", { enable: true, platform: "fcm" });
    expect(noToken.__error.code).toBe(-32001);
    const badEnable = await rpc(socket, "shannon/push.register", { enable: "yes", platform: "fcm", token: "t" });
    expect(badEnable.__error.code).toBe(-32001);
    socket.close();
  });

  it("unregister (enable:false) is an honest local ok without any sink", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);
    await pairDevice(socket, tokens, generateEd25519KeyPair());
    const res = await rpc(socket, "shannon/push.register", { enable: false });
    expect(res).toEqual({ ok: true });
    socket.close();
  });

  // 修正1 (frame contract review 2026-10-05 §4 acceptance): the enable:false
  // → relay-unbind WIRING assertion. The old local no-op violated §O2
  // (「enable:false → 注销：desktop 指示 relay 摘除该 deviceId 绑定」).
  it("unregister (enable:false) forwards push.unbind under the device session, still ok:true", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const seen: string[] = [];
    const unbindSink: PushUnbindSink = async (deviceId) => {
      seen.push(deviceId);
    };
    const { port } = await start(handlers({ tokens, registry, pushUnbindSink: unbindSink }));
    const socket = await connect(port);
    const deviceId = await pairDevice(socket, tokens, generateEd25519KeyPair());

    const res = await rpc(socket, "shannon/push.register", { enable: false });
    expect(res).toEqual({ ok: true }); // the phone's honest ok is unchanged
    await vi.waitFor(() => expect(seen).toEqual([deviceId])); // best-effort forward, not awaited by the RPC
    socket.close();
  });

  it("unregister stays honest ok even when the unbind forward fails (对账兜底)", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const unbindSink: PushUnbindSink = async () => {
      throw new Error("relay link down");
    };
    const { port } = await start(handlers({ tokens, registry, pushUnbindSink: unbindSink }));
    const socket = await connect(port);
    await pairDevice(socket, tokens, generateEd25519KeyPair());
    const res = await rpc(socket, "shannon/push.register", { enable: false });
    expect(res).toEqual({ ok: true });
    await new Promise((r) => setTimeout(r, 10)); // the rejection is logged, never surfaced
    socket.close();
  });

  it("without a binding sink: structured NOT_IMPLEMENTED — the honest 推送不可用, never a mock", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);
    await pairDevice(socket, tokens, generateEd25519KeyPair());
    const res = await rpc(socket, "shannon/push.register", { enable: true, platform: "apns", token: "tok" });
    expect(res.__error.code).toBe(-32603); // NOT_IMPLEMENTED
    expect(res.__error.message).toMatch(/not configured/);
    socket.close();
  });

  it("with a sink: forwards {platform, token} under the device session, returns the handle", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const seen: Array<{ deviceId: string; platform: string; token: string }> = [];
    const sink: PushBindingSink = async (deviceId, binding) => {
      seen.push({ deviceId, platform: binding.platform, token: binding.token });
      return { handle: "h-relay-123" };
    };
    const { port } = await start(handlers({ tokens, registry, pushBindingSink: sink }));
    const socket = await connect(port);
    const deviceId = await pairDevice(socket, tokens, generateEd25519KeyPair());

    const res = await rpc(socket, "shannon/push.register", { enable: true, platform: "fcm", token: "tok-1" });
    expect(res).toEqual({ ok: true, handle: "h-relay-123" });
    expect(seen).toEqual([{ deviceId, platform: "fcm", token: "tok-1" }]);
    socket.close();
  });

  it("a sink rejection is a structured error, still never a fake success", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const sink: PushBindingSink = async () => {
      throw new Error("relay has no vendor credentials");
    };
    const { port } = await start(handlers({ tokens, registry, pushBindingSink: sink }));
    const socket = await connect(port);
    await pairDevice(socket, tokens, generateEd25519KeyPair());
    const res = await rpc(socket, "shannon/push.register", { enable: true, platform: "fcm", token: "tok" });
    expect(res.__error.code).toBe(-32002); // ENGINE_ERROR (structured upstream refusal)
    expect(res.__error.message).toMatch(/relay has no vendor credentials/);
    socket.close();
  });
});

// ── §O3: hub wake trigger seam ──────────────────────────────────────────────

describe("MobileDispatchHub wake seam (§O3)", () => {
  it("fires for approval.request and turn terminals — including OFFLINE pushes", () => {
    const fired: Array<{ deviceId: string; seq: number; type: string }> = [];
    const hub = new MobileDispatchHub({
      logger,
      seqCounter: createSeqCounter(),
      replay: new PushReplayBuffer(),
      wake: (deviceId, seq, event) => fired.push({ deviceId, seq, type: event.type }),
    });
    const sent: string[] = [];
    const ctx = {
      socket: { readyState: WebSocket.OPEN, send: (f: string) => sent.push(f), on: () => {}, off: () => {} },
      sessionId: "devA",
      logger,
    } as unknown as MethodContext;
    const detach = hub.registerConnection(ctx);

    hub.pushEvent("devA", { type: "approval.request", request_id: "r1", tool_name: "Edit", tool_input: {}, description: "d", is_destructive: false, diff_preview: null } as any);
    hub.pushEvent("devA", { type: "task.progress", content: "delta" });
    detach(); // device goes offline
    hub.pushEvent("devA", { type: "query.failed", error: "boom" });

    // approval.request (online) + query.failed (offline — the point of wake).
    expect(fired.map((f) => f.type)).toEqual(["approval.request", "query.failed"]);
    expect(fired[0]).toMatchObject({ deviceId: "devA", seq: 1 });
    expect(fired[1]?.seq).toBe(3);
    // Progress still DELIVERS while online (sent: approval + progress) — the
    // wake filter only decides who rings the doorbell, not who gets mail.
    expect(sent.length).toBe(2);
  });

  it("a throwing wake never fails the push (same posture as the §M2 broadcast)", () => {
    const hub = new MobileDispatchHub({
      logger,
      seqCounter: createSeqCounter(),
      replay: new PushReplayBuffer(),
      wake: () => {
        throw new Error("relay down");
      },
    });
    expect(hub.pushEvent("devA", { type: "query.failed", error: "boom" })).toBe(false);
  });
});

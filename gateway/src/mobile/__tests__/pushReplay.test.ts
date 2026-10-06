/**
 * §O4 replay buffer acceptance — per-device ring semantics + resume wiring.
 *
 * Contract source: cross-repo spec §O4 / shannon-mobile `live_sync.dart`
 * (replayed entries fan out to the domain providers; each entry is a full
 * `shannon/event` params object carrying its seq). The pre-buffer contract —
 * `replayed: []` means "cursor at head", hole/gap → GAP_TOO_LARGE → the
 * phone re-snapshots — is unchanged; the buffer only fills the array.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import { createSeqCounter } from "../seq.js";
import { PushReplayBuffer } from "../pushReplay.js";
import { MobileDispatchHub } from "../hub.js";
import {
  createMobileHandlers,
  DeviceRegistry,
  PairTokenStore,
} from "../pairing.js";
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
  seqCounter: ReturnType<typeof createSeqCounter>;
  replay: PushReplayBuffer;
}): MethodHandlers {
  return createMobileHandlers({
    engine: {
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
    },
    tokens: opts.tokens,
    registry: opts.registry,
    logger,
    seq: opts.seqCounter,
    replayBuffer: opts.replay,
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
    device_label: "replay test",
  });
  return deviceIdFromPublicKey(kp.publicKeyB64Url);
}

// ── PushReplayBuffer (the ring itself) ──────────────────────────────────────

describe("PushReplayBuffer (§O4 ring)", () => {
  it("replays a device's own entries after the cursor, ascending; other devices never leak", () => {
    const buf = new PushReplayBuffer();
    buf.record("devA", 5, { type: "query.started", turn_id: "t1" });
    buf.record("devB", 7, { type: "query.started", turn_id: "t2" });
    buf.record("devA", 9, { type: "task.message", text: "done", session_id: "s1" });

    expect(buf.replay("devA", 0)).toEqual({
      complete: true,
      entries: [
        { seq: 5, event: { type: "query.started", turn_id: "t1" } },
        { seq: 9, event: { type: "task.message", text: "done", session_id: "s1" } },
      ],
    });
    expect(buf.replay("devA", 5).entries.map((e) => e.seq)).toEqual([9]);
    expect(buf.replay("devA", 9)).toEqual({ entries: [], complete: true });
  });

  it("a cursor before the ring's oldest entry is a hole → complete:false (caller GAPs)", () => {
    const buf = new PushReplayBuffer({ capacityPerDevice: 2 });
    buf.record("devA", 1, { type: "query.started", turn_id: "t1" });
    buf.record("devA", 2, { type: "query.started", turn_id: "t2" });
    buf.record("devA", 3, { type: "query.started", turn_id: "t3" });

    expect(buf.replay("devA", 0)).toEqual({ entries: [], complete: false });
    // At the ring's edge the retained run covers the gap again.
    const atEdge = buf.replay("devA", 1);
    expect(atEdge.complete).toBe(true);
    expect(atEdge.entries.map((e) => e.seq)).toEqual([2, 3]);
    expect(buf.replay("devA", 2).entries.map((e) => e.seq)).toEqual([3]);
  });

  it("a device with no buffered entries is trivially complete (pre-buffer answer)", () => {
    const buf = new PushReplayBuffer();
    expect(buf.replay("ghost", 0)).toEqual({ entries: [], complete: true });
    expect(buf.replay(null, 0)).toEqual({ entries: [], complete: true });
  });

  it("forget drops the device's ring (revocation hygiene)", () => {
    const buf = new PushReplayBuffer();
    buf.record("devA", 1, { type: "query.started", turn_id: "t1" });
    buf.forget("devA");
    expect(buf.replay("devA", 0)).toEqual({ entries: [], complete: true });
  });
});

// ── hub integration: record at seq-stamp time, online or not ────────────────

describe("MobileDispatchHub → replay ring", () => {
  it("records online and offline pushes alike", () => {
    const seq = createSeqCounter();
    const replay = new PushReplayBuffer();
    const hub = new MobileDispatchHub({ logger, seqCounter: seq, replay });

    const sent: string[] = [];
    const ctx = {
      socket: {
        readyState: WebSocket.OPEN,
        send: (frame: string) => sent.push(frame),
        on: () => {},
        off: () => {},
      },
      sessionId: "devA",
      logger,
    } as unknown as MethodContext;
    const detach = hub.registerConnection(ctx);

    expect(hub.pushEvent("devA", { type: "query.started", turn_id: "t1" })).toBe(true);
    // Offline: detach the only socket → not delivered, but still seq-stamped
    // and recorded (an offline push is exactly what resume replays later).
    detach();
    expect(hub.pushEvent("devA", { type: "task.message", text: "hi", session_id: "s1" })).toBe(false);

    const replayed = replay.replay("devA", 0).entries;
    expect(replayed.map((e) => e.seq)).toEqual([1, 2]);
    expect(replayed[1]?.event).toEqual({ type: "task.message", text: "hi", session_id: "s1" });
    // The delivered frame and the ring entry carry the same seq.
    expect(JSON.parse(sent[0]!).params.seq).toBe(1);
  });
});

// ── resume integration: the real handlers replay the ring ───────────────────

describe("shannon/resume × replayBuffer (§O4)", () => {
  function setup(capacityPerDevice?: number) {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const seqCounter = createSeqCounter();
    const replay = new PushReplayBuffer({ capacityPerDevice });
    const hub = new MobileDispatchHub({ logger, seqCounter, replay });
    return { tokens, registry, seqCounter, replay, hub };
  }

  it("replays the device's offline pushes after the cursor, wire shape verbatim", async () => {
    const s = setup();
    const { port } = await start(handlers(s));
    const socket = await connect(port);
    const deviceId = await pairDevice(socket, s.tokens, generateEd25519KeyPair());

    // Offline pushes (no sockets registered for the device in this wiring).
    s.hub.pushEvent(deviceId, { type: "query.started", turn_id: "t1" });
    s.hub.pushEvent(deviceId, { type: "task.message", text: "hi", session_id: "s1" });

    const res = await rpc(socket, "shannon/resume", { sinceSeq: 0 });
    expect(res.replayed).toEqual([
      { seq: 1, type: "query.started", turn_id: "t1" },
      { seq: 2, type: "task.message", text: "hi", session_id: "s1" },
    ]);
    expect(res.lastSeq).toBe(2);

    // Cursor at head → empty replay, unchanged pre-buffer semantics.
    const head = await rpc(socket, "shannon/resume", { sinceSeq: 2 });
    expect(head.replayed).toEqual([]);
    socket.close();
  });

  it("a hole in the device's buffered stream answers GAP_TOO_LARGE (phone re-snapshots)", async () => {
    const s = setup(2);
    const { port } = await start(handlers(s));
    const socket = await connect(port);
    const deviceId = await pairDevice(socket, s.tokens, generateEd25519KeyPair());

    for (const turn of ["t1", "t2", "t3"]) {
      s.hub.pushEvent(deviceId, { type: "query.started", turn_id: turn });
    }
    const res = await rpc(socket, "shannon/resume", { sinceSeq: 0 });
    expect(res.__error.code).toBe(-32014);
    expect(res.__error.message).toMatch(/gap exceeds retained window/);
    socket.close();
  });

  it("revocation forgets the device's ring", async () => {
    const s = setup();
    const { port } = await start(handlers(s));
    const socket = await connect(port);
    const deviceId = await pairDevice(socket, s.tokens, generateEd25519KeyPair());
    s.hub.pushEvent(deviceId, { type: "query.started", turn_id: "t1" });

    await rpc(socket, "shannon/device.revoke", { deviceId });
    expect(s.replay.replay(deviceId, 0)).toEqual({ entries: [], complete: true });
    socket.close();
  });
});

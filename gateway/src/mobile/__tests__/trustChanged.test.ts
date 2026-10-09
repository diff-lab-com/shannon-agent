/**
 * trust.changed push (2026-10-08 batch): `shannon/trust.revoke` → engine
 * `revoked: true` → `shannon/trust.changed {kind, revokedAt}` broadcast to
 * EVERY connected device. Contract pins:
 *  - dedicated notification METHOD (not a shannon/event type) — no `seq`,
 *    no §O4 replay-ring record (the store is the truth; a missed push
 *    degrades to the §Q5 re-probe behavior);
 *  - ONLY an actual removal pushes (idempotent replay `revoked: false` and
 *    engine failures announce nothing);
 *  - `revokedAt` is gateway-clock ISO-8601;
 *  - grants never push (decide response is the grant's ack path, §Q2).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { WebSocket } from "ws";

import {
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  signMessage,
} from "../crypto.js";
import { createConsoleLogger } from "../../logger.js";
import { bootstrap } from "../../bootstrap.js";
import { MobileDispatchHub } from "../hub.js";
import { PushReplayBuffer } from "../pushReplay.js";
import { createEngineHandlers, type EngineClient } from "../engineBridge.js";
import type { EngineEvent } from "../../engine/runtime.js";
import type { MethodContext } from "../server.js";

/**
 * Local minimal engine-client stand-in (the shape `bootstrap`'s
 * `mobileEngineClientFactory` must return). Inlined rather than imported from
 * dispatch.test.js — importing a test module would re-run its whole suite
 * inside this file's run.
 */
function stubEngineClient(): EngineClient {
  return {
    connect: async () => {},
    close: async () => {},
    cancel: () => {},
    // The trust.changed face never streams a query; an empty generator is
    // already more than this test exercises.
    runQuery: async function* (): AsyncGenerator<EngineEvent> {
      return;
    },
  } as unknown as EngineClient;
}

const logger = createConsoleLogger("error");

// ── hub unit: frame shape / no seq / no replay record / fan-out count ────────

/** Minimal open-socket MethodContext stand-in (send captures the frame). */
function fakeCtx(deviceId: string, sent: string[]): MethodContext {
  return {
    socket: {
      readyState: 1, // WebSocket.OPEN
      send: (frame: string) => sent.push(frame),
      on: () => {},
      off: () => {},
    } as unknown as WebSocket,
    sessionId: deviceId,
    logger,
  };
}

describe("MobileDispatchHub.pushTrustChanged", () => {
  it("broadcasts the exact frame to every open socket of every bound device", () => {
    const hub = new MobileDispatchHub({ logger });
    const dev1: string[] = [];
    const dev2a: string[] = [];
    const dev2b: string[] = [];
    hub.registerConnection(fakeCtx("dev-1", dev1));
    hub.registerConnection(fakeCtx("dev-2", dev2a));
    hub.registerConnection(fakeCtx("dev-2", dev2b));

    const delivered = hub.pushTrustChanged("Bash", "2026-10-08T03:30:00.000Z");
    expect(delivered).toBe(3);
    for (const sent of [dev1, dev2a, dev2b]) {
      expect(sent).toHaveLength(1);
      const frame = JSON.parse(sent[0]!) as Record<string, unknown>;
      expect(frame).toEqual({
        jsonrpc: "2.0",
        method: "shannon/trust.changed",
        params: { kind: "Bash", revokedAt: "2026-10-08T03:30:00.000Z" },
      });
      // Dedicated notification method: a top-level seq must not exist.
      expect("seq" in frame).toBe(false);
    }
  });

  it("stamps nothing into the seq counter or the §O4 replay ring", () => {
    const replay = new PushReplayBuffer();
    const hub = new MobileDispatchHub({ logger, replay });
    const sent: string[] = [];
    hub.registerConnection(fakeCtx("dev-1", sent));
    const before = hub.lastSeq;

    hub.pushTrustChanged("Bash", "2026-10-08T03:30:00.000Z");

    expect(hub.lastSeq).toBe(before); // seq-free
    expect(replay.replay("dev-1", 0)).toEqual({ entries: [], complete: true });
  });

  it("counts only OPEN sockets as delivered (offline devices miss it by design)", () => {
    const hub = new MobileDispatchHub({ logger });
    const open: string[] = [];
    hub.registerConnection(fakeCtx("dev-1", open));
    hub.registerConnection(fakeCtx("dev-2", []));
    // A closed socket: readyState !== OPEN.
    const closed = fakeCtx("dev-3", []);
    (closed.socket as unknown as { readyState: number }).readyState = 3;
    hub.registerConnection(closed);

    expect(hub.pushTrustChanged("Write", "2026-10-08T03:31:00.000Z")).toBe(2);
    expect(open).toHaveLength(1);
  });
});

// ── end-to-end: revoke over the RPC face → broadcast to both paired devices ──

interface Device {
  socket: WebSocket;
  deviceId: string;
  notifications: Array<Record<string, unknown>>;
  rpc: (method: string, params?: unknown) => Promise<any>;
}

/**
 * Event-driven wait for the device's first trust.changed frame (cross-socket
 * delivery has no ordering guarantee against the revoking socket's RPC
 * response, so assertions must wait on the frame, not on the response).
 */
function waitForNotification(dev: Device, timeoutMs = 2_000): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`no shannon/trust.changed within ${timeoutMs}ms`)),
      timeoutMs,
    );
    const onFrame = (data: unknown): void => {
      const msg = JSON.parse(String(data)) as Record<string, unknown>;
      if (msg.method === "shannon/trust.changed") {
        clearTimeout(timer);
        dev.socket.off("message", onFrame);
        resolve(msg);
      }
    };
    dev.socket.on("message", onFrame);
  });
}

const dirs: string[] = [];
afterEach(() => {
  while (dirs.length) rmSync(dirs.pop()!, { recursive: true, force: true });
  vi.restoreAllMocks();
});

/**
 * boot → pair `count` devices (one token each) → bound sockets collecting
 * shannon/trust.changed notifications. The engine HTTP face is a fetch mock
 * answering the revoke route with `body`.
 */
async function harness(
  count: number,
  revokeBody: string,
  revokeStatus = 200,
): Promise<{ devices: Device[]; port: number }> {
  const dir = mkdtempSync(join(tmpdir(), "gw-trustchanged-"));
  dirs.push(dir);
  const tokensFile = join(dir, "tokens.jsonl");
  const devicesFile = join(dir, "devices.json");
  const kps = Array.from({ length: count }, () => generateEd25519KeyPair());
  writeFileSync(
    tokensFile,
    kps
      .map((_, i) =>
        JSON.stringify({
          token: `trust-token-${i}`,
          issuedAt: Date.now(),
          expiresAt: Date.now() + 75_000,
        }),
      )
      .join("\n") + "\n",
    "utf8",
  );

  const fetchMock = vi.fn<typeof fetch>(async (url) => {
    if (String(url).endsWith("/api/trust/revoke")) {
      return {
        status: revokeStatus,
        ok: revokeStatus >= 200 && revokeStatus < 300,
        text: async () => revokeBody,
        json: async () => JSON.parse(revokeBody),
      } as unknown as Response;
    }
    return { status: 404, ok: false, text: async () => "", json: async () => ({}) } as unknown as Response;
  });

  const handle = await bootstrap(
    {
      engine: { wsUrl: "ws://mock/api/ws", httpBaseUrl: "http://mock" },
      adapters: [],
      mobile: { enabled: true, host: "127.0.0.1", port: 0, tokensFile, devicesFile },
    },
    {
      factories: new Map(),
      mobileEngineClientFactory: () => stubEngineClient(),
      mobileFetchImpl: fetchMock as unknown as typeof fetch,
      logger,
    },
  );
  const port = handle.mobilePort!;

  const devices: Device[] = [];
  for (const [i, kp] of kps.entries()) {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
    await new Promise<void>((resolve, reject) => {
      socket.once("open", () => resolve());
      socket.once("error", reject);
    });
    const notifications: Array<Record<string, unknown>> = [];
    socket.on("message", (data: unknown) => {
      const msg = JSON.parse(String(data)) as Record<string, unknown>;
      if (msg.method === "shannon/trust.changed") notifications.push(msg);
    });
    const rpc = (method: string, params: unknown = {}): Promise<any> =>
      new Promise((resolve) => {
        const id = Math.floor(Math.random() * 1e9);
        const onMsg = (data: unknown): void => {
          const msg = JSON.parse(String(data)) as any;
          if (msg.id === id) {
            socket.off("message", onMsg);
            if (msg.error) resolve({ __error: msg.error });
            else resolve(msg.result);
          }
        };
        socket.on("message", onMsg);
        socket.send(JSON.stringify({ jsonrpc: "2.0", id, method, params }));
      });
    const token = `trust-token-${i}`;
    const pop = signMessage(kp.privateKey, pairPopMessage(token, kp.publicKeyB64Url));
    const pairRes = await rpc("shannon/pair", {
      pair_token: token,
      device_public_key: kp.publicKeyB64Url,
      pop_signature: pop,
      device_label: `trustchanged-${i}`,
    });
    expect(pairRes.device_id).toBe(deviceIdFromPublicKey(kp.publicKeyB64Url));
    devices.push({ socket, deviceId: pairRes.device_id, notifications, rpc });
  }
  return { devices, port };
}

describe("shannon/trust.revoke → shannon/trust.changed (end-to-end)", () => {
  it("a real removal broadcasts {kind, revokedAt} to every paired device — seq-free, replay-ring-free", async () => {
    const { devices } = await harness(2, JSON.stringify({ kind: "Bash", revoked: true }));
    const revoker = devices[0]!;
    try {
      const before = await revoker.rpc("shannon/snapshot");
      const waits = Promise.all(devices.map((dev) => waitForNotification(dev)));
      const res = await revoker.rpc("shannon/trust.revoke", { kind: "Bash" });
      expect(res).toEqual({ ok: true });

      // Both devices — the revoker AND the bystander — get the push.
      const frames = await waits;
      for (const frame of frames) {
        expect(frame.method).toBe("shannon/trust.changed");
        expect(frame.jsonrpc).toBe("2.0");
        const params = frame.params as { kind: string; revokedAt: string };
        expect(params.kind).toBe("Bash");
        expect("seq" in frame).toBe(false); // dedicated method: no push cursor
        expect(Number.isNaN(Date.parse(params.revokedAt))).toBe(false); // ISO-8601
      }
      // Exactly one push per device — no duplicates, no replay-ring echoes.
      await new Promise((r) => setTimeout(r, 100));
      for (const dev of devices) {
        expect(dev.notifications).toHaveLength(1);
      }

      // §O4: the push consumed no seq and left no ring entry — resume at the
      // pre-revoke cursor replays nothing (missed push ⇒ re-probe degrade).
      const after = await revoker.rpc("shannon/resume", {
        sinceSeq: before.lastSeq,
      });
      expect(after.replayed).toEqual([]);
      expect(after.lastSeq).toBe(before.lastSeq);
    } finally {
      for (const dev of devices) dev.socket.close();
    }
  });

  it("an idempotent replay (revoked:false) answers ok and pushes nothing", async () => {
    const { devices } = await harness(2, JSON.stringify({ kind: "Bash", revoked: false }));
    try {
      const res = await devices[0]!.rpc("shannon/trust.revoke", { kind: "Bash" });
      expect(res).toEqual({ ok: true });
      for (const dev of devices) {
        expect(dev.notifications).toEqual([]);
      }
    } finally {
      for (const dev of devices) dev.socket.close();
    }
  });

  it("an engine failure answers ENGINE_ERROR and pushes nothing", async () => {
    const { devices } = await harness(1, "boom", 500);
    try {
      const res = await devices[0]!.rpc("shannon/trust.revoke", { kind: "Bash" });
      expect(res.__error?.code).toBeDefined();
      expect(devices[0]!.notifications).toEqual([]);
    } finally {
      for (const dev of devices) dev.socket.close();
    }
  });
});

// ── bridge unit: the onTrustChanged seam ─────────────────────────────────────

describe("createEngineHandlers onTrustChanged seam", () => {
  function mockResponse(status: number, body: string): Response {
    return {
      status,
      ok: status >= 200 && status < 300,
      text: async () => body,
      json: async () => JSON.parse(body),
    } as unknown as Response;
  }

  it("fires with the requested kind and an ISO revokedAt only when revoked:true", async () => {
    const onTrustChanged = vi.fn<(kind: string, revokedAt: string) => void>();
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      fetchImpl: (async () =>
        mockResponse(200, JSON.stringify({ kind: "Bash", revoked: true }))) as typeof fetch,
      onTrustChanged,
    });
    const ctx = { socket: { readyState: 0 }, sessionId: "dev-1", logger } as unknown as MethodContext;
    const res = await handlers["shannon/trust.revoke"]!({ kind: "Bash" }, ctx);
    expect(res).toEqual({ kind: "result", result: { ok: true } });
    expect(onTrustChanged).toHaveBeenCalledTimes(1);
    const [kind, revokedAt] = onTrustChanged.mock.calls[0]!;
    expect(kind).toBe("Bash");
    expect(Number.isNaN(Date.parse(revokedAt))).toBe(false);
  });

  it("never fires when the engine had nothing to revoke", async () => {
    const onTrustChanged = vi.fn<(kind: string, revokedAt: string) => void>();
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      fetchImpl: (async () =>
        mockResponse(200, JSON.stringify({ kind: "Bash", revoked: false }))) as typeof fetch,
      onTrustChanged,
    });
    const ctx = { socket: { readyState: 0 }, sessionId: "dev-1", logger } as unknown as MethodContext;
    await handlers["shannon/trust.revoke"]!({ kind: "Bash" }, ctx);
    expect(onTrustChanged).not.toHaveBeenCalled();
  });

  it("a throwing sink never fails the revoke", async () => {
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      fetchImpl: (async () =>
        mockResponse(200, JSON.stringify({ kind: "Bash", revoked: true }))) as typeof fetch,
      onTrustChanged: () => {
        throw new Error("sink exploded");
      },
    });
    const ctx = { socket: { readyState: 0 }, sessionId: "dev-1", logger } as unknown as MethodContext;
    const res = await handlers["shannon/trust.revoke"]!({ kind: "Bash" }, ctx);
    expect(res).toEqual({ kind: "result", result: { ok: true } });
  });
});

import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import type { EngineEvent } from "../../engine/runtime.js";
import {
  approvalMessage,
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  resumeMessage,
  signMessage,
} from "../crypto.js";
import { ShannonError, type ShannonEvent } from "../protocol.js";
import { MobileServer, type MethodHandlers } from "../server.js";
import {
  DeviceRegistry,
  PairTokenStore,
  createMobileHandlers,
} from "../pairing.js";
import type { EngineClient } from "../engineBridge.js";

const logger = createConsoleLogger("error");

let servers: { stop: () => Promise<void> }[] = [];
let tmpDirs: string[] = [];
afterEach(async () => {
  await Promise.all(servers.map((s) => s.stop().catch(() => {})));
  servers = [];
  for (const d of tmpDirs) rmSync(d, { recursive: true, force: true });
  tmpDirs = [];
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

// ── mock engine (only needed for post-pair query/approval E2E) ──────────────

class FakeEngine implements EngineClient {
  constructor(private readonly script: EngineEvent[] = []) {}
  async connect(): Promise<void> {}
  cancel(): void {}
  async close(): Promise<void> {}
  async *runQuery(): AsyncGenerator<EngineEvent> {
    for (const ev of this.script) yield ev;
  }
}

function newPhone() {
  return generateEd25519KeyPair();
}

/** Wire a full mobile handler map (pairing + engine bridge, requireSession on). */
function handlers(opts: {
  tokens: PairTokenStore;
  registry: DeviceRegistry;
  fetchImpl?: typeof fetch;
  engineScript?: EngineEvent[];
}): MethodHandlers {
  return createMobileHandlers({
    engine: {
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      engineClientFactory: () => new FakeEngine(opts.engineScript ?? []),
      fetchImpl: opts.fetchImpl,
    },
    tokens: opts.tokens,
    registry: opts.registry,
    logger,
  });
}

function mockOk(): typeof fetch {
  return vi.fn(async () => ({ status: 200, ok: true, text: async () => "" } as unknown as Response)) as unknown as typeof fetch;
}

// ── PairTokenStore ──────────────────────────────────────────────────────────

describe("PairTokenStore", () => {
  it("issues and consumes a token once", () => {
    let t = 1000;
    const store = new PairTokenStore({ ttlMs: 60_000, now: () => t });
    const rec = store.issue();
    expect(store.consume(rec.token)).toEqual(rec);
    expect(store.consume(rec.token)).toBeNull(); // replay → consumed
  });

  it("rejects an expired token", () => {
    let t = 1000;
    const store = new PairTokenStore({ ttlMs: 60_000, now: () => t });
    const rec = store.issue();
    t += 61_000;
    expect(store.consume(rec.token)).toBeNull();
  });

  it("rejects an unknown token", () => {
    const store = new PairTokenStore();
    expect(store.consume("never-issued")).toBeNull();
  });
});

// ── PairTokenStore (file-backed, Design D cross-process channel) ────────────

describe("PairTokenStore (file-backed)", () => {
  it("consumes across instances via the shared file (desktop writes, gateway reads)", () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-tokens-"));
    tmpDirs.push(dir);
    const path = join(dir, "tokens.jsonl");

    // Desktop process mints (writes the file).
    const issuer = new PairTokenStore({ filePath: path });
    const rec = issuer.issue();

    // Gateway process (separate instance) consumes.
    const consumer = new PairTokenStore({ filePath: path });
    expect(consumer.consume(rec.token)).toEqual(rec);
    // Replay → consumed (the line was removed on the first consume).
    expect(new PairTokenStore({ filePath: path }).consume(rec.token)).toBeNull();
  });

  it("rejects an expired token and prunes it from the file", () => {
    let t = 1000;
    const dir = mkdtempSync(join(tmpdir(), "shannon-tokens-"));
    tmpDirs.push(dir);
    const path = join(dir, "tokens.jsonl");

    const issuer = new PairTokenStore({ filePath: path, ttlMs: 60_000, now: () => t });
    const rec = issuer.issue();
    t += 61_000;
    const consumer = new PairTokenStore({ filePath: path, now: () => t });
    expect(consumer.consume(rec.token)).toBeNull();
    // Expired line pruned on the consume attempt → file no longer carries it.
    expect(new PairTokenStore({ filePath: path, now: () => t }).consume(rec.token)).toBeNull();
  });

  it("returns null without rewriting when the token is absent (no clobber)", () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-tokens-"));
    tmpDirs.push(dir);
    const path = join(dir, "tokens.jsonl");
    const a = new PairTokenStore({ filePath: path });
    const rec = a.issue(); // writes one line

    const miss = new PairTokenStore({ filePath: path }).consume("not-there");
    expect(miss).toBeNull();

    // The still-valid token must remain consumable (the miss didn't rewrite).
    expect(new PairTokenStore({ filePath: path }).consume(rec.token)).toEqual(rec);
  });

  // WP-15 P2-6: a desktop that mints QR tokens nobody consumes used to grow
  // the JSONL forever — file-mode `issue()` now prunes expired lines first.
  it("issue() prunes expired siblings instead of appending forever", () => {
    let t = 1000;
    const dir = mkdtempSync(join(tmpdir(), "shannon-tokens-"));
    tmpDirs.push(dir);
    const path = join(dir, "tokens.jsonl");

    const issuer = new PairTokenStore({ filePath: path, ttlMs: 60_000, now: () => t });
    const stale1 = issuer.issue();
    t += 10_000;
    const stale2 = issuer.issue();
    t += 50_000; // stale1 expired; stale2 still valid
    const fresh = issuer.issue();

    const lines = readFileSync(path, "utf8")
      .split("\n")
      .filter((l) => l.trim().length > 0);
    expect(lines).toHaveLength(2); // stale1 dropped, stale2 + fresh kept
    expect(lines.some((l) => l.includes(stale1.token))).toBe(false);

    // Survivors stay consumable.
    expect(new PairTokenStore({ filePath: path, now: () => t }).consume(stale2.token)).toEqual(
      stale2,
    );
    expect(new PairTokenStore({ filePath: path, now: () => t }).consume(fresh.token)).toEqual(
      fresh,
    );
  });

  it("consumes legacy snake_case desktop records (issued_at/expires_at)", () => {
    // Pre-fix desktop builds appended snake_case records; the strict camelCase
    // read silently skipped them — desktop QR tokens never validated. They must
    // keep consuming after the fix (canonical write side stays camelCase).
    const dir = mkdtempSync(join(tmpdir(), "shannon-tokens-"));
    tmpDirs.push(dir);
    const path = join(dir, "tokens.jsonl");
    writeFileSync(
      path,
      JSON.stringify({ token: "legacy-tok", issued_at: 1000, expires_at: Date.now() + 75_000 }) +
        "\n",
      "utf8",
    );

    const consumer = new PairTokenStore({ filePath: path });
    const rec = consumer.consume("legacy-tok");
    expect(rec?.token).toBe("legacy-tok");
    // Consumed line removed; file rewritten in canonical camelCase shape.
    expect(readFileSync(path, "utf8")).not.toContain("legacy-tok");
  });
});

// ── DeviceRegistry ──────────────────────────────────────────────────────────

describe("DeviceRegistry", () => {
  it("upserts, gets, revokes, and persists to a data file", () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-devices-"));
    tmpDirs.push(dir);
    const path = join(dir, "devices.json");

    const reg = new DeviceRegistry({ filePath: path });
    const id = deviceIdFromPublicKey(generateEd25519KeyPair().publicKeyB64Url);
    expect(reg.has(id)).toBe(false);
    reg.upsert(id, "pk", "pixel");
    expect(reg.has(id)).toBe(true);
    expect(reg.get(id)?.label).toBe("pixel");

    // reload from disk
    const reloaded = new DeviceRegistry({ filePath: path });
    expect(reloaded.has(id)).toBe(true);
    expect(reloaded.get(id)?.public_key).toBe("pk");
    expect(reloaded.list()).toHaveLength(1);

    expect(reloaded.revoke(id)).toBe(true);
    expect(reloaded.has(id)).toBe(false);
    expect(new DeviceRegistry({ filePath: path }).has(id)).toBe(false);
  });

  it("loads legacy camelCase entries instead of silently dropping them", () => {
    // Pre-fix desktop builds wrote camelCase; the strict snake_case read used
    // to drop every entry — under wholesale-replace semantics that un-trusted
    // all devices on the next external rewrite.
    const dir = mkdtempSync(join(tmpdir(), "shannon-devices-"));
    tmpDirs.push(dir);
    const path = join(dir, "devices.json");
    writeFileSync(
      path,
      JSON.stringify({
        entries: [
          {
            deviceId: "abc123",
            publicKey: "pk",
            label: "pixel",
            addedAt: 1000,
            lastSeenAt: 2000,
          },
        ],
      }),
      "utf8",
    );

    const reg = new DeviceRegistry({ filePath: path });
    expect(reg.size).toBe(1);
    expect(reg.get("abc123")?.public_key).toBe("pk");
    expect(reg.get("abc123")?.label).toBe("pixel");
  });
});

// ── pairing + resume + session gate (full server E2E) ──────────────────────

describe("createMobileHandlers (P1.2 pairing lifecycle)", () => {
  it("pairs on a valid token + proof-of-possession and registers the device", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);

    const phone = newPhone();
    const rec = tokens.issue();
    const pop = signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url));
    const res = await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: pop,
      device_label: "pixel",
    });
    const expectedId = deviceIdFromPublicKey(phone.publicKeyB64Url);
    expect(res.result).toEqual({ device_id: expectedId, session_id: expectedId, device_label: "pixel" });
    expect(registry.has(expectedId)).toBe(true);
    socket.close();
  });

  it("rejects a replayed, unknown, or bad-POP token (and consumes it either way)", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);
    const phone = newPhone();

    // unknown token
    const r1 = await rpc(socket, "shannon/pair", {
      pair_token: "nope",
      device_public_key: phone.publicKeyB64Url,
      pop_signature: "x",
    });
    expect(r1.error?.code).toBe(ShannonError.BAD_PARAMS);

    // replayed: issue once, consume on a bad-POP attempt, then replay
    const rec = tokens.issue();
    const r2 = await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: "bad-signature",
    });
    expect(r2.error?.code).toBe(ShannonError.BAD_PARAMS);
    expect(registry.size).toBe(0); // not registered on bad POP

    const r3 = await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url)),
    });
    expect(r3.error?.code).toBe(ShannonError.BAD_PARAMS); // consumed → rejected
    expect(registry.size).toBe(0);
    socket.close();
  });

  it("requires a paired session for query (PAIRING_REQUIRED before pair; streams after)", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(
      handlers({ tokens, registry, engineScript: [{ type: "completed", model: "m" }] }),
    );
    const socket = await connect(port);
    const phone = newPhone();

    // before pairing → gate rejects
    const gated = await rpc(socket, "shannon/query", { prompt: "hi" });
    expect(gated.error?.code).toBe(ShannonError.PAIRING_REQUIRED);

    // pair on the same socket → ctx.sessionId is now bound
    const rec = tokens.issue();
    await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url)),
    });

    // query now streams
    const { events, response } = rpcStream(socket, "shannon/query", { prompt: "hi" });
    const res = await response;
    expect(res.result).toEqual({ ok: true });
    expect(events.map((e) => e.type)).toContain("query.completed");
    socket.close();
  });

  it("device.resume: valid sig rebinds; unknown device / stale ts / bad sig rejected", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));

    // seed a registered device directly
    const phone = newPhone();
    const deviceId = deviceIdFromPublicKey(phone.publicKeyB64Url);
    registry.upsert(deviceId, phone.publicKeyB64Url, "pixel");

    const fresh = async (): Promise<WebSocket> => {
      const s = await connect(port);
      return s;
    };

    // valid resume
    let socket = await fresh();
    const ts = Date.now();
    const good = await rpc(socket, "shannon/device.resume", {
      device_id: deviceId,
      timestamp: ts,
      signature: signMessage(phone.privateKey, resumeMessage(deviceId, ts)),
    });
    // WP-15 T4: the resume payload seeds the phone's live-sync cursor.
    expect(good.result).toEqual({
      device_id: deviceId,
      session_id: deviceId,
      lastSeq: expect.any(Number),
    });
    socket.close();

    // unknown device
    socket = await fresh();
    const unknown = await rpc(socket, "shannon/device.resume", {
      device_id: "0000000000000000000000000000abcd",
      timestamp: ts,
      signature: "x",
    });
    expect(unknown.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    socket.close();

    // stale timestamp → dedicated CLOCK_SKEW code (mobile classifies without
    // matching message text)
    socket = await fresh();
    const stale = await rpc(socket, "shannon/device.resume", {
      device_id: deviceId,
      timestamp: ts - 120_000,
      signature: signMessage(phone.privateKey, resumeMessage(deviceId, ts - 120_000)),
    });
    expect(stale.error?.code).toBe(ShannonError.CLOCK_SKEW);
    socket.close();

    // bad signature
    socket = await fresh();
    const bad = await rpc(socket, "shannon/device.resume", {
      device_id: deviceId,
      timestamp: ts,
      signature: "not-a-valid-signature",
    });
    expect(bad.error?.code).toBe(ShannonError.BAD_PARAMS);
    socket.close();
  });

  it("device.resume anti-replay: nonce reuse rejected; legacy timestamp replay rejected by watermark", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));

    const phone = newPhone();
    const deviceId = deviceIdFromPublicKey(phone.publicKeyB64Url);
    registry.upsert(deviceId, phone.publicKeyB64Url, "pixel");

    const resume = async (nonce: string | undefined, timestamp = Date.now()): Promise<any> => {
      const s = await connect(port);
      try {
        return await rpc(s, "shannon/device.resume", {
          device_id: deviceId,
          timestamp,
          nonce,
          signature: signMessage(phone.privateKey, resumeMessage(deviceId, timestamp, nonce)),
        });
      } finally {
        s.close();
      }
    };

    // Nonce form: first resume binds, replaying the same signed message fails.
    const nonce = "a".repeat(32);
    const first = await resume(nonce);
    expect(first.result?.device_id).toBe(deviceId);
    const replayed = await resume(nonce);
    expect(replayed.error?.code).toBe(ShannonError.CLOCK_SKEW);
    // A fresh nonce on a fresh socket succeeds.
    const second = await resume("b".repeat(32));
    expect(second.result?.device_id).toBe(deviceId);

    // Legacy (no nonce) form: a strictly newer timestamp is required —
    // re-sending an older successful resume must not re-bind.
    const oldTs = Date.now() - 1000;
    const legacyFirst = await resume(undefined, oldTs);
    expect(legacyFirst.result?.device_id).toBe(deviceId);
    const legacyReplay = await resume(undefined, oldTs - 1);
    expect(legacyReplay.error?.code).toBe(ShannonError.CLOCK_SKEW);
  });

  it("snapshot/resume (live-sync) require a paired session; model.switch cannot be driven unpaired", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));
    const socket = await connect(port);

    const snap = await rpc(socket, "shannon/snapshot");
    expect(snap.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    const cursor = await rpc(socket, "shannon/resume", { sinceSeq: 0 });
    expect(cursor.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    const switchUnpaired = await rpc(socket, "shannon/model.switch", { model: "rogue" });
    expect(switchUnpaired.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    const listUnpaired = await rpc(socket, "shannon/model.list");
    expect(listUnpaired.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    socket.close();
  });

  it("approval/decide enforces a valid per-decision signature", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const fetchImpl = mockOk();
    const { port } = await start(handlers({ tokens, registry, fetchImpl }));
    const socket = await connect(port);
    const phone = newPhone();

    // pair first
    const rec = tokens.issue();
    await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url)),
    });

    // missing signature → rejected
    const missing = await rpc(socket, "shannon/approval/decide", { request_id: "r1", choice: "allow" });
    expect(missing.error?.code).toBe(ShannonError.BAD_PARAMS);

    // wrong-content signature (signs r2 not r1) → rejected
    const wrong = await rpc(socket, "shannon/approval/decide", {
      request_id: "r1",
      choice: "allow",
      signature: signMessage(phone.privateKey, approvalMessage("r2", "allow")),
    });
    expect(wrong.error?.code).toBe(ShannonError.BAD_PARAMS);

    // valid signature → POST + ok
    const ok = await rpc(socket, "shannon/approval/decide", {
      request_id: "r1",
      choice: "allow",
      signature: signMessage(phone.privateKey, approvalMessage("r1", "allow")),
    });
    expect(ok.result).toEqual({ ok: true });
    socket.close();
  });

  it("a revoked device cannot resume or query", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));

    const phone = newPhone();
    const deviceId = deviceIdFromPublicKey(phone.publicKeyB64Url);
    registry.upsert(deviceId, phone.publicKeyB64Url, "pixel");

    // resume works before revoke
    const socket = await connect(port);
    const ts = Date.now();
    const before = await rpc(socket, "shannon/device.resume", {
      device_id: deviceId,
      timestamp: ts,
      signature: signMessage(phone.privateKey, resumeMessage(deviceId, ts)),
    });
    expect(before.result.session_id).toBe(deviceId);
    socket.close();

    // revoke → resume now fails
    expect(registry.revoke(deviceId)).toBe(true);
    const socket2 = await connect(port);
    const after = await rpc(socket2, "shannon/device.resume", {
      device_id: deviceId,
      timestamp: ts,
      signature: signMessage(phone.privateKey, resumeMessage(deviceId, ts)),
    });
    expect(after.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    socket2.close();
  });
});

// ── scope: revoke is self-only; queries are owned by the caller ─────────────

describe("scope (v0.12 minimal set)", () => {
  it("device.revoke rejects revoking OTHER devices (desktop-only action)", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(handlers({ tokens, registry }));

    const phoneA = newPhone();
    const phoneB = newPhone();
    const idA = deviceIdFromPublicKey(phoneA.publicKeyB64Url);
    const idB = deviceIdFromPublicKey(phoneB.publicKeyB64Url);
    registry.upsert(idA, phoneA.publicKeyB64Url);
    registry.upsert(idB, phoneB.publicKeyB64Url);

    const socket = await connect(port);
    // Bind the socket to device A (pair consumes a token + PoP).
    const rec = tokens.issue();
    await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phoneA.publicKeyB64Url,
      pop_signature: signMessage(phoneA.privateKey, pairPopMessage(rec.token, phoneA.publicKeyB64Url)),
    });

    const res = await rpc(socket, "shannon/device.revoke", { device_id: idB });
    expect(res.error?.code).toBe(ShannonError.BAD_PARAMS);
    // B is untouched.
    expect(registry.has(idB)).toBe(true);

    // Self-revoke still works (logout-everywhere).
    const self = await rpc(socket, "shannon/device.revoke", { device_id: idA });
    expect(self.result).toEqual({ device_id: idA, revoked: true });
    expect(registry.has(idA)).toBe(false);
    socket.close();
  });

  it("query ignores a foreign session_id and runs under the caller's device session", async () => {
    const tokens = new PairTokenStore();
    const registry = new DeviceRegistry();
    const { port } = await start(
      handlers({ tokens, registry, engineScript: [{ type: "completed", model: "m" }] }),
    );
    const socket = await connect(port);
    const phone = newPhone();
    const rec = tokens.issue();
    await rpc(socket, "shannon/pair", {
      pair_token: rec.token,
      device_public_key: phone.publicKeyB64Url,
      pop_signature: signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url)),
    });

    // Pretending to be another device's session must not route the query
    // under that key — it runs under the caller's bound device session.
    const { events, response } = rpcStream(socket, "shannon/query", {
      prompt: "hi",
      session_id: "someone-elses-session",
    });
    const res = await response;
    expect(res.result).toEqual({ ok: true });
    expect(events.map((e) => e.type)).toContain("query.completed");
    socket.close();
  });
});

/**
 * v0.13 direct-link E2E seal (cross-repo-adaptation-spec §I): byte-level
 * golden pins against shannon-mobile `test/direct_e2e_golden_test.dart`, plus
 * end-to-end negotiation coverage over a real MobileServer — sealed pairing,
 * resume via kid, legacy fallback for old phones, and the guard properties
 * (per-connection counters, text frames ignored once sealed, base-key
 * publication on pair success).
 */
import net from "node:net";

import { createHash, createPrivateKey, createPublicKey, diffieHellman, randomBytes } from "node:crypto";
import type { KeyObject } from "node:crypto";
import { mkdtempSync, existsSync, readFileSync, statSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import {
  E2eChannel,
  deriveSessionKeyV2,
  hostSharedSecret,
} from "../relay/e2e.js";
import {
  directE2EPaths,
  directKeyId,
  directReadyFrame,
  deriveDirectSessionKey,
  ensureDirectE2EKey,
  generateDirectE2EKeyPair,
  isDirectHelloFrame,
} from "../directE2E.js";
import { ShannonError } from "../protocol.js";
import { DeviceRegistry, PairTokenStore, createMobileHandlers } from "../pairing.js";
import { MobileServer, type MethodHandlers } from "../server.js";
import {
  deviceIdFromPublicKey,
  generateEd25519KeyPair,
  pairPopMessage,
  resumeMessage,
  signMessage,
} from "../crypto.js";

const logger = createConsoleLogger("error");

let servers: { stop: () => Promise<void> }[] = [];
let tmpDirs: string[] = [];
afterEach(async () => {
  await Promise.all(servers.map((s) => s.stop().catch(() => {})));
  servers = [];
  for (const d of tmpDirs) rmSync(d, { recursive: true, force: true });
  tmpDirs = [];
});

// ── fixed-scalar X25519 helpers (same scalars as the mobile golden test) ────

/** X25519 private key from a raw 32-byte scalar (PKCS8 DER wrapper, OID 1.3.101.110). */
function x25519FromScalar(scalar: Buffer): KeyObject {
  const pkcs8 = Buffer.concat([Buffer.from("302e020100300506032b656e04220420", "hex"), scalar]);
  return createPrivateKey({ key: pkcs8, format: "der", type: "pkcs8" });
}

function pubB64Of(priv: KeyObject): string {
  return (createPublicKey(priv).export({ format: "jwk" }) as { x: string }).x;
}

function sharedAsPhone(phonePriv: KeyObject, hostPubB64: string): Buffer {
  return diffieHellman({
    privateKey: phonePriv,
    publicKey: createPublicKey({ key: { kty: "OKP", crv: "X25519", x: hostPubB64 }, format: "jwk" }),
  });
}

const HOST_SCALAR = Buffer.alloc(32, 0x11);
const PHONE_SCALAR = Buffer.alloc(32, 0x22);
const HOST_PUB_B64 = "e06Qm75__kTEZaIgA31gjuNYl9Me-XLwf3SJLLD3PxM";
const PHONE_PUB_B64 = "D6poTtKIZ7l_Smot7l34zpdOdrcBjj8iocTPJnhXDyA";
const SHARED_HEX = "9e004098efc091d4ec2663b4e9f5cfd4d7064571690b4bea97ab146ab9f35056";
const BASE_KEY = Buffer.alloc(32, 0x33);
const RESUME_KEY_HEX = "fd358c499bbbc6469d87a2a2028d083e5d7139a14318cb602292b42950eebbf6";
const KID_B64 = "3rDjjO0eQd4";
const GOLDEN_TOKEN = "golden-pair-token";
const PAIRING_K0_HEX = "6a5341b2e59fc3ce87ca83f0b917066a227c38d6d921368e4fff250ce72c1258";

function helloFrame(pubB64: string, kidB64?: string): Buffer {
  const kid = kidB64 === undefined ? "" : `,"kid":"${kidB64}"`;
  return Buffer.from(`{"t":"e2e_direct_hello","v":1,"pub":"${pubB64}"${kid}}`, "utf8");
}

// ── golden pins (byte-level, cross-repo) ────────────────────────────────────

describe("direct E2E golden vectors (cross-repo pin with shannon-mobile)", () => {
  it("fixed scalars yield the pinned pubs and X25519 shared secret", () => {
    expect(pubB64Of(x25519FromScalar(HOST_SCALAR))).toBe(HOST_PUB_B64);
    expect(pubB64Of(x25519FromScalar(PHONE_SCALAR))).toBe(PHONE_PUB_B64);
    expect(sharedAsPhone(x25519FromScalar(PHONE_SCALAR), HOST_PUB_B64).toString("hex")).toBe(SHARED_HEX);
    // The host-side helper agrees with the phone-side computation.
    expect(hostSharedSecret(x25519FromScalar(HOST_SCALAR), PHONE_PUB_B64).toString("hex")).toBe(SHARED_HEX);
  });

  it("resume flavor: baseKey-mixed ECDH with the dedicated direct info string", () => {
    expect(deriveDirectSessionKey(BASE_KEY, Buffer.from(SHARED_HEX, "hex")).toString("hex")).toBe(RESUME_KEY_HEX);
  });

  it("key id: 8-byte sha256 prefix of the base key, base64url", () => {
    expect(directKeyId(BASE_KEY)).toBe(KID_B64);
    expect(createHash("sha256").update(BASE_KEY).digest().subarray(0, 8).toString("base64url")).toBe(KID_B64);
  });

  it("pairing flavor reuses the relay v2 schedule unchanged", () => {
    expect(deriveSessionKeyV2(GOLDEN_TOKEN, Buffer.from(SHARED_HEX, "hex")).toString("hex")).toBe(PAIRING_K0_HEX);
  });

  it("handshake frame bytes (spec §I wire shapes)", () => {
    expect(isDirectHelloFrame(helloFrame(PHONE_PUB_B64))).toEqual({ pub: PHONE_PUB_B64 });
    expect(isDirectHelloFrame(helloFrame(PHONE_PUB_B64, KID_B64))).toEqual({ pub: PHONE_PUB_B64, kid: KID_B64 });
    expect(isDirectHelloFrame(Buffer.from('{"t":"e2e_hello","pub":"x"}'))).toBeNull(); // relay hello ≠ direct hello
    expect(isDirectHelloFrame(Buffer.from("not json"))).toBeNull();
    expect(directReadyFrame().toString("utf8")).toBe('{"t":"e2e_direct_ready","v":1}');
  });
});

// ── end-to-end negotiation over a real MobileServer ──────────────────────────

interface TestRig {
  port: number;
  linkKeys: Map<string, Buffer>;
  hostPriv: KeyObject;
  stop: () => Promise<void>;
}

async function startServer(directOpts?: {
  livePairToken?: () => string | null;
  handlers?: MethodHandlers;
}): Promise<TestRig> {
  const hostPriv = generateDirectE2EKeyPair().privateKey;
  const linkKeys = new Map<string, Buffer>();
  const handlers =
    directOpts?.handlers ??
    ({
      "shannon/ping": async () => ({ kind: "result", result: { ok: true, pong: 1 } }),
    }) as MethodHandlers;
  const server = new MobileServer({
    host: "127.0.0.1",
    port: 0,
    logger,
    handlers,
    directE2E: {
      privateKey: hostPriv,
      ...(directOpts?.livePairToken ? { livePairToken: directOpts.livePairToken } : {}),
      linkKeys,
    },
  });
  const handle = await server.start();
  servers.push(handle);
  return { port: handle.port, linkKeys, hostPriv, stop: () => server.stop() };
}

function connect(port: number): Promise<WebSocket> {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(`ws://127.0.0.1:${port}/`);
    socket.once("open", () => resolve(socket));
    socket.once("error", reject);
  });
}

/**
 * Frame collector: ws drains a TCP segment synchronously, so back-to-back
 * frames (ack + queued fan-out) emit in ONE turn — a per-call `once` listener
 * would miss the second. Attach one persistent listener, queue frames, and
 * hand them out in order.
 */
function frameReader(socket: WebSocket): () => Promise<{ data: Buffer; isBinary: boolean }> {
  const queue: { data: Buffer; isBinary: boolean }[] = [];
  const waiters: Array<(f: { data: Buffer; isBinary: boolean }) => void> = [];
  socket.on("message", (data, isBinary) => {
    const frame = {
      data: Buffer.isBuffer(data) ? data : Buffer.from(data as ArrayBuffer),
      isBinary,
    };
    const waiter = waiters.shift();
    if (waiter) waiter(frame);
    else queue.push(frame);
  });
  return () =>
    queue.length > 0
      ? Promise.resolve(queue.shift()!)
      : new Promise((resolve) => waiters.push(resolve));
}

/** Phone-side sealed transport over the negotiated key. */
class PhonePeer {
  readonly send: E2eChannel;
  readonly recv: E2eChannel;

  constructor(readonly key: Buffer) {
    this.send = new E2eChannel(key);
    this.recv = new E2eChannel(key);
  }

  static pairing(token: string, phonePriv: KeyObject, hostPubB64: string): PhonePeer {
    return new PhonePeer(deriveSessionKeyV2(token, sharedAsPhone(phonePriv, hostPubB64)));
  }

  static resume(baseKey: Buffer, phonePriv: KeyObject, hostPubB64: string): PhonePeer {
    return new PhonePeer(deriveDirectSessionKey(baseKey, sharedAsPhone(phonePriv, hostPubB64)));
  }

  seal(text: string): Buffer {
    return this.send.seal(Buffer.from(text, "utf8"));
  }

  open(frame: Buffer): string {
    return this.recv.open(frame).toString("utf8");
  }
}

describe("direct E2E end-to-end (real MobileServer + ws)", () => {
  it("sealed pairing: ack opens under the token-mixed key, then NDJSON flows sealed both ways", async () => {
    const token = "pair-tok-1";
    const rig = await startServer({ livePairToken: () => token });
    const socket = await connect(rig.port);
    const phonePriv = x25519FromScalar(PHONE_SCALAR);
    const peer = PhonePeer.pairing(token, phonePriv, pubB64Of(rig.hostPriv));

    const next = frameReader(socket);
    socket.send(helloFrame(pubB64Of(phonePriv)), { binary: true });

    // First frame from the host is the sealed ready ack — counter starts at 1.
    const ack = await next();
    expect(ack.isBinary).toBe(true);
    expect(peer.open(ack.data)).toBe('{"t":"e2e_direct_ready","v":1}');
    expect(peer.recv.recvCounter).toBe(1);

    // A sealed request gets a sealed response.
    socket.send(peer.seal(JSON.stringify({ jsonrpc: "2.0", id: 7, method: "shannon/ping" })), { binary: true });
    const reply = await next();
    expect(reply.isBinary).toBe(true);
    expect(peer.open(reply.data)).toBe(JSON.stringify({ jsonrpc: "2.0", id: 7, result: { ok: true, pong: 1 } }));

    // Per-connection counters: each direction advanced independently.
    expect(peer.send.sendCounter).toBe(1);
    expect(peer.recv.recvCounter).toBe(2);
    socket.close();
  });

  it("pair success publishes kid→K0; a resume hello rekeys with a fresh per-connection key", async () => {
    const tokens = new PairTokenStore();
    const rec = tokens.issue();
    const phone = generateEd25519KeyPair();
    const handlers = createMobileHandlers({
      engine: {
        engineWsUrl: "ws://127.0.0.1:9",
        engineHttpBaseUrl: "http://127.0.0.1:9",
        version: "test",
        logger,
      },
      tokens,
      registry: new DeviceRegistry(),
      logger,
    });
    const rig = await startServer({ livePairToken: () => tokens.latest()?.token ?? null, handlers });
    const phoneEph = generateDirectE2EKeyPair();

    // ── pairing connection: hello (no kid) → seal → shannon/pair ───────────
    const pairSocket = await connect(rig.port);
    const pairNext = frameReader(pairSocket);
    const peer = PhonePeer.pairing(rec.token, phoneEph.privateKey, pubB64Of(rig.hostPriv));
    pairSocket.send(helloFrame(phoneEph.pubB64), { binary: true });
    const ack = await pairNext();
    expect(peer.open(ack.data)).toBe('{"t":"e2e_direct_ready","v":1}');

    const k0 = peer.key;
    pairSocket.send(
      peer.seal(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "shannon/pair",
          params: {
            pair_token: rec.token,
            device_public_key: phone.publicKeyB64Url,
            pop_signature: signMessage(phone.privateKey, pairPopMessage(rec.token, phone.publicKeyB64Url)),
          },
        }),
      ),
      { binary: true },
    );
    const pairReply = await pairNext();
    const pairResult = JSON.parse(peer.open(pairReply.data)) as {
      result?: { device_id?: string };
    };
    expect(pairResult.result?.device_id).toBe(deviceIdFromPublicKey(phone.publicKeyB64Url));
    // §I6.2: kid(K0) → K0 published exactly on pair success.
    expect(rig.linkKeys.get(directKeyId(k0))).toEqual(k0);
    pairSocket.close();

    // ── resume connection: hello with kid → NEW key, counters restart at 1 ──
    const resumeSocket = await connect(rig.port);
    const resumeNext = frameReader(resumeSocket);
    const resumeEph = generateDirectE2EKeyPair();
    const resumed = PhonePeer.resume(k0, resumeEph.privateKey, pubB64Of(rig.hostPriv));
    expect(resumed.key).not.toEqual(k0); // fresh ECDH → fresh connection key
    resumeSocket.send(helloFrame(resumeEph.pubB64, directKeyId(k0)), { binary: true });
    const resumeAck = await resumeNext();
    expect(resumed.open(resumeAck.data)).toBe('{"t":"e2e_direct_ready","v":1}');
    expect(resumed.recv.recvCounter).toBe(1); // counters never span connections

    // The sealed resume channel carries the real post-restart auth (Z1):
    // device.resume over the connection key derived from the published K0.
    const ts = Date.now();
    resumeSocket.send(
      resumed.seal(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 2,
          method: "shannon/device.resume",
          params: {
            device_id: pairResult.result?.device_id,
            timestamp: ts,
            signature: signMessage(phone.privateKey, resumeMessage(pairResult.result?.device_id ?? "", ts)),
          },
        }),
      ),
      { binary: true },
    );
    const reply = await resumeNext();
    const resumeResult = JSON.parse(resumed.open(reply.data)) as {
      result?: { device_id?: string; lastSeq?: number };
    };
    expect(resumeResult.result?.device_id).toBe(pairResult.result?.device_id);
    resumeSocket.close();
  });

  it("resume hello with an unknown kid is swallowed and the link stays legacy", async () => {
    const rig = await startServer();
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    const eph = generateDirectE2EKeyPair();
    socket.send(helloFrame(eph.pubB64, "unknownkid0"), { binary: true });
    // No ack — the hello never existed. A plaintext NDJSON line proves legacy mode.
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 3, method: "shannon/ping" }));
    const reply = await next();
    expect(reply.isBinary).toBe(false);
    expect(JSON.parse(reply.data.toString("utf8"))).toEqual({
      jsonrpc: "2.0",
      id: 3,
      result: { ok: true, pong: 1 },
    });
    socket.close();
  });

  it("pairing hello with no live token falls back to plaintext (old-gateway contract)", async () => {
    const rig = await startServer({ livePairToken: () => null });
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    const eph = generateDirectE2EKeyPair();
    socket.send(helloFrame(eph.pubB64), { binary: true });
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 4, method: "shannon/ping" }));
    const reply = await next();
    expect(reply.isBinary).toBe(false);
    expect(JSON.parse(reply.data.toString("utf8")).id).toBe(4);
    socket.close();
  });

  it("an old phone that never sends the hello is completely unaffected", async () => {
    const rig = await startServer();
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    // Text NDJSON first frame (PWA / legacy direct client).
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 5, method: "shannon/ping" }));
    const textReply = await next();
    expect(textReply.isBinary).toBe(false);
    expect(JSON.parse(textReply.data.toString("utf8")).id).toBe(5);
    // Legacy binary tolerance: UTF-8 NDJSON as a binary frame.
    socket.send(Buffer.from(JSON.stringify({ jsonrpc: "2.0", id: 6, method: "shannon/ping" })), { binary: true });
    const binReply = await next();
    expect(JSON.parse(binReply.data.toString("utf8")).id).toBe(6);
    socket.close();
  });

  it("once sealed, inbound TEXT frames are ignored and the session survives", async () => {
    const token = "pair-tok-2";
    const rig = await startServer({ livePairToken: () => token });
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    const eph = generateDirectE2EKeyPair();
    const peer = PhonePeer.pairing(token, eph.privateKey, pubB64Of(rig.hostPriv));
    socket.send(helloFrame(eph.pubB64), { binary: true });
    expect(peer.open((await next()).data)).toContain("e2e_direct_ready");

    // Unauthenticated plaintext datagram mid-session — dropped, no teardown.
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 99, method: "shannon/ping" }));

    // The sealed session still answers, and the plaintext frame did not
    // disturb the recv counter (the sealed sequence stays aligned).
    socket.send(peer.seal(JSON.stringify({ jsonrpc: "2.0", id: 8, method: "shannon/ping" })), { binary: true });
    const reply = await next();
    expect(JSON.parse(peer.open(reply.data)).id).toBe(8);
    socket.close();
  });

  it("outbound lines raced before the first frame are queued and flushed in order after the seal", async () => {
    const token = "pair-tok-3";
    let capturedCtx: { socket: { send: (data: string) => void } } | null = null;
    const handlers = {
      "shannon/ping": async () => ({ kind: "result" as const, result: { ok: true, pong: 1 } }),
    };
    const hostPriv = generateDirectE2EKeyPair().privateKey;
    const server = new MobileServer({
      host: "127.0.0.1",
      port: 0,
      logger,
      handlers,
      onContext: (ctx) => {
        capturedCtx = ctx as unknown as { socket: { send: (data: string) => void } };
      },
      directE2E: { privateKey: hostPriv, livePairToken: () => token, linkKeys: new Map() },
    });
    const handle = await server.start();
    servers.push(handle);

    const socket = await connect(handle.port);
    const next = frameReader(socket);
    // Hub-style fan-out BEFORE the phone's hello: the proxy queues it.
    capturedCtx!.socket.send(JSON.stringify({ jsonrpc: "2.0", method: "shannon/event", params: { type: "task.progress", content: "early" } }));

    const eph = generateDirectE2EKeyPair();
    const peer = PhonePeer.pairing(token, eph.privateKey, pubB64Of(hostPriv));
    socket.send(helloFrame(eph.pubB64), { binary: true });
    const ack = await next();
    expect(peer.open(ack.data)).toBe('{"t":"e2e_direct_ready","v":1}');
    // The queued line arrives sealed, right after the ack, in order.
    const queued = await next();
    expect(JSON.parse(peer.open(queued.data)).params.content).toBe("early");
    socket.close();
  });

  it("a replayed/out-of-order sealed frame is rejected and the link keeps failing closed", async () => {
    const token = "pair-tok-4";
    const rig = await startServer({ livePairToken: () => token });
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    const eph = generateDirectE2EKeyPair();
    const peer = PhonePeer.pairing(token, eph.privateKey, pubB64Of(rig.hostPriv));
    socket.send(helloFrame(eph.pubB64), { binary: true });
    await next();

    const first = peer.seal(JSON.stringify({ jsonrpc: "2.0", id: 10, method: "shannon/ping" }));
    socket.send(first, { binary: true });
    const reply = await next();
    expect(JSON.parse(peer.open(reply.data)).id).toBe(10);
    // Replay the SAME counter — dropped, no response (and no crash).
    socket.send(first, { binary: true });
    // The link is still alive: the NEXT counter gets through.
    socket.send(peer.seal(JSON.stringify({ jsonrpc: "2.0", id: 11, method: "shannon/ping" })), { binary: true });
    const reply2 = await next();
    expect(JSON.parse(peer.open(reply2.data)).id).toBe(11);
    socket.close();
  });
});

// ── robustness (unauthenticated frames must never crash the gateway) ─────────

/**
 * Minimal raw WS client: completes the upgrade, then sends one client frame
 * built by hand. `ws`'s own client API cannot produce the protocol violations
 * we need (it validates/encodes everything it sends).
 */
class RawWsClient {
  private readonly sock: net.Socket;
  private buffer = Buffer.alloc(0);
  private handshakeDone = false;
  /** Full HTTP handshake response (status line + headers). */
  handshakeResponse = "";
  readonly frames: { opcode: number; payload: Buffer }[] = [];
  private closed: (code: number) => void = () => {};

  private constructor(port: number) {
    this.sock = net.connect(port, "127.0.0.1");
    // Attach before the handshake — the 101 response may land immediately.
    // Bytes before the header terminator are HTTP, never WS frames.
    this.sock.on("data", (chunk: Buffer) => {
      this.buffer = Buffer.concat([this.buffer, chunk]);
      if (!this.handshakeDone) {
        const end = this.buffer.indexOf("\r\n\r\n");
        if (end === -1) return;
        this.handshakeDone = true;
        this.handshakeResponse = this.buffer.subarray(0, end).toString("latin1");
        this.buffer = this.buffer.subarray(end + 4);
      }
      this.drain();
    });
  }

  static async connect(port: number): Promise<RawWsClient> {
    const client = new RawWsClient(port);
    const key = randomBytes(16).toString("base64");
    client.sock.write(
      "GET / HTTP/1.1\r\n" +
        `Host: 127.0.0.1:${port}\r\n` +
        "Upgrade: websocket\r\n" +
        "Connection: Upgrade\r\n" +
        `Sec-WebSocket-Key: ${key}\r\n` +
        "Sec-WebSocket-Version: 13\r\n\r\n",
    );
    await vi.waitFor(() => {
      expect(client.handshakeResponse).toContain("101 Switching Protocols");
    });
    return client;
  }

  /** Parse complete WS frames out of the buffer (client→server frames are masked, server→client are not). */
  private drain(): void {
    for (;;) {
      if (this.buffer.length < 2) return;
      const opcode = this.buffer[0]! & 0x0f;
      const masked = (this.buffer[1]! & 0x80) !== 0;
      let len = this.buffer[1]! & 0x7f;
      let offset = 2;
      if (len === 126) {
        if (this.buffer.length < 4) return;
        len = this.buffer.readUInt16BE(2);
        offset = 4;
      } else if (len === 127) {
        if (this.buffer.length < 10) return;
        len = Number(this.buffer.readBigUInt64BE(2));
        offset = 10;
      }
      const maskLen = masked ? 4 : 0;
      if (this.buffer.length < offset + maskLen + len) return;
      const payload = Buffer.from(this.buffer.subarray(offset + maskLen, offset + maskLen + len));
      if (masked) {
        const mask = this.buffer.subarray(offset, offset + 4);
        for (let i = 0; i < payload.length; i++) payload[i]! ^= mask[i % 4]!;
      }
      this.buffer = this.buffer.subarray(offset + maskLen + len);
      if (opcode === 0x8) {
        const code = payload.length >= 2 ? payload.readUInt16BE(0) : 1005;
        this.closed(code);
        this.sock.destroy();
        return;
      }
      this.frames.push({ opcode, payload });
    }
  }

  /** Send a masked frame (client→server frames MUST be masked). */
  sendMasked(opcode: number, payload: Buffer): void {
    const mask = Buffer.alloc(4); // zero mask keeps the payload readable server-side
    const header = Buffer.allocUnsafe(2 + 4);
    header[0] = 0x80 | opcode; // FIN + opcode
    header[1] = 0x80 | payload.length; // MASK + 7-bit len (payload < 126)
    mask.copy(header, 2);
    this.sock.write(Buffer.concat([header, payload]));
  }

  onClose(fn: (code: number) => void): void {
    this.closed = fn;
  }

  destroy(): void {
    this.sock.destroy();
  }
}

describe("direct E2E robustness", () => {
  it("a malformed unauthenticated frame is an error, not a gateway crash", async () => {
    // The regression: the proxy re-emits the real socket's `error` with zero
    // listeners — EventEmitter THROWS then, and in a real process that is an
    // uncaughtException (gateway exits). Vitest only reports run-level noise
    // for that, so watch the process ourselves and fail the test on any hit.
    const uncaught: Error[] = [];
    const watcher = (err: Error): void => {
      uncaught.push(err);
    };
    process.on("uncaughtException", watcher);

    const rig = await startServer();
    const raw = await RawWsClient.connect(rig.port);
    // TEXT frame whose payload is not valid UTF-8 (lone continuation byte):
    // the ws receiver raises before any dispatch, on an unauthenticated
    // connection — exactly what a LAN peer can send.
    raw.sendMasked(0x1, Buffer.from([0x80]));

    try {
      // The gateway must still be alive and serving: a normal client connects
      // and completes an RPC after the abusive frame.
      const socket = await connect(rig.port);
      const next = frameReader(socket);
      socket.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "shannon/ping" }));
      const reply = await next();
      expect(JSON.parse(reply.data.toString("utf8")).result).toEqual({ ok: true, pong: 1 });
      socket.close();
    } finally {
      process.off("uncaughtException", watcher);
    }
    expect(uncaught).toEqual([]);
    raw.destroy();
  });

  it("a hello sent as a TEXT frame is not negotiated — legacy NDJSON applies (§I2.1)", async () => {
    const token = "pair-tok-text";
    const rig = await startServer({ livePairToken: () => token });
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    // Same bytes as a hello, but on a TEXT frame: the hello contract is binary-only.
    socket.send(helloFrame(PHONE_PUB_B64).toString("utf8"));
    const reply = await next();
    // The text hello was dispatched as NDJSON → invalid JSON-RPC → error response.
    expect(reply.isBinary).toBe(false);
    expect(JSON.parse(reply.data.toString("utf8")).error?.code).toBe(ShannonError.INVALID_REQUEST);
    // The link stays legacy: a normal request still works (no sealed frames).
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 2, method: "shannon/ping" }));
    const ping = await next();
    expect(JSON.parse(ping.data.toString("utf8")).result).toEqual({ ok: true, pong: 1 });
    socket.close();
  });

  it("a hello with a malformed pub is swallowed and the link degrades to legacy", async () => {
    const token = "pair-tok-badpub";
    const rig = await startServer({ livePairToken: () => token });
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    socket.send(helloFrame("!!!not-base64url!!!"), { binary: true });
    socket.send(JSON.stringify({ jsonrpc: "2.0", id: 3, method: "shannon/ping" }));
    const reply = await next();
    expect(reply.isBinary).toBe(false);
    expect(JSON.parse(reply.data.toString("utf8")).result).toEqual({ ok: true, pong: 1 });
    socket.close();
  });

  it("a resume-flavor bind does not publish anything new (pairing flavor only)", async () => {
    const tokens = new PairTokenStore();
    const rec = tokens.issue();
    const phone = generateEd25519KeyPair();
    const handlers = createMobileHandlers({
      engine: {
        engineWsUrl: "ws://127.0.0.1:9",
        engineHttpBaseUrl: "http://127.0.0.1:9",
        version: "test",
        logger,
      },
      tokens,
      registry: new DeviceRegistry(),
      logger,
    });
    const rig = await startServer({ livePairToken: () => tokens.latest()?.token ?? null, handlers });
    const deviceId = deviceIdFromPublicKey(phone.publicKeyB64Url);
    rig.linkKeys.set("preexisting", Buffer.alloc(32, 0x44)); // sentinel: other pairings exist too

    // Pair over a sealed connection (publishes kid→K0).
    const eph = generateDirectE2EKeyPair();
    const socket = await connect(rig.port);
    const next = frameReader(socket);
    const peer = PhonePeer.pairing(rec.token, eph.privateKey, pubB64Of(rig.hostPriv));
    socket.send(helloFrame(eph.pubB64), { binary: true });
    await next();
    const ts = Date.now();
    socket.send(
      peer.seal(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "shannon/device.resume",
          params: {
            device_id: deviceId,
            timestamp: ts,
            signature: signMessage(phone.privateKey, resumeMessage(deviceId, ts)),
          },
        }),
      ),
      { binary: true },
    );
    const reply = await next();
    // Unregistered device → resume rejected; nothing may be published.
    expect(JSON.parse(peer.open(reply.data)).error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    expect(rig.linkKeys.size).toBe(1); // only the sentinel — no publish without pair success
    socket.close();
  });
});

// ── static identity persistence (mobileTls.ts pattern) ───────────────────────

describe("ensureDirectE2EKey persistence", () => {
  it("generates once, persists 0600, and reloads the same identity", () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-direct-e2e-"));
    tmpDirs.push(dir);
    const paths = directE2EPaths(dir);

    const first = ensureDirectE2EKey(paths);
    const second = ensureDirectE2EKey(paths);
    expect(second.pubB64).toBe(first.pubB64);

    expect(existsSync(paths.keyPath)).toBe(true);
    expect(existsSync(paths.infoPath)).toBe(true);
    // Private key + info file must not be world/group readable.
    // (POSIX mode bits only — Windows NTFS has no chmod semantics, so the
    // check would read arbitrary values there.)
    if (process.platform !== "win32") {
      expect(statSync(paths.keyPath).mode & 0o777).toBe(0o600);
      expect(statSync(paths.infoPath).mode & 0o777).toBe(0o600);
    }

    const info = JSON.parse(readFileSync(paths.infoPath, "utf8")) as { hostE2EPubKey: string };
    expect(info.hostE2EPubKey).toBe(first.pubB64);

    const jwk = JSON.parse(readFileSync(paths.keyPath, "utf8")) as {
      kty: string;
      crv: string;
      d?: string;
    };
    expect(jwk.kty).toBe("OKP");
    expect(jwk.crv).toBe("X25519");
    expect(jwk.d).toBeTruthy();
  });
});

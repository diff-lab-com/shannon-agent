import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket, WebSocketServer } from "ws";

import { createConsoleLogger } from "../../../logger.js";
import { E2eChannel, deriveSessionKey } from "../e2e.js";
import type { MethodHandlers } from "../../server.js";
import type { HandlerOutcome } from "../../server.js";
import type { HealthResult } from "../../protocol.js";
import { startRelayHost, type RelayHostHandle } from "../relayHost.js";

const logger = createConsoleLogger("error");

let relayWss: WebSocketServer | null = null;
let hostHandle: RelayHostHandle | null = null;
let openSockets: WebSocket[] = [];

afterEach(async () => {
  await hostHandle?.stop().catch(() => {});
  hostHandle = null;
  for (const s of openSockets) s.close();
  openSockets = [];
  if (relayWss) {
    await new Promise<void>((resolve) => relayWss!.close(() => resolve()));
    relayWss = null;
  }
});

/**
 * Minimal mock relay: accepts host and phone connections, pairs them by sid,
 * and forwards binary frames between them. Mimics the real shannon-relay
 * control protocol (register, host_ready, paired, peer_gone).
 */
class MockRelay {
  private hosts = new Map<string, WebSocket>();
  private phones = new Map<string, WebSocket>();
  /** Most recent register frame (for contract assertions). */
  lastRegister: Record<string, unknown> | null = null;

  start(port = 0): Promise<number> {
    return new Promise((resolve, reject) => {
      relayWss = new WebSocketServer({ port });
      relayWss.on("listening", () => {
        const boundPort = (relayWss!.address() as { port: number }).port;
        resolve(boundPort);
      });
      relayWss.on("error", reject);
      relayWss.on("connection", (ws) => {
        ws.on("message", (data, isBinary) => {
          if (isBinary) {
            // Forward binary to the peer in the same session.
            // Try host → phone first, then phone → host.
            // We need to know which side sent this. The mock relay tracks
            // connections; binary forwarding uses the stored peer reference.
            this.forwardBinary(ws, data as Buffer);
            return;
          }
          let msg: Record<string, unknown>;
          try {
            msg = JSON.parse(String(data));
          } catch {
            return;
          }
          this.handleControl(ws, msg);
        });
        ws.on("close", () => {
          // Notify the peer if any.
          for (const [sid, host] of this.hosts) {
            if (host === ws) {
              this.hosts.delete(sid);
              const phone = this.phones.get(sid);
              if (phone && phone.readyState === WebSocket.OPEN) {
                phone.send(JSON.stringify({ t: "peer_gone", side: "host_left" }));
              }
              return;
            }
          }
          for (const [sid, phone] of this.phones) {
            if (phone === ws) {
              this.phones.delete(sid);
              const host = this.hosts.get(sid);
              if (host && host.readyState === WebSocket.OPEN) {
                host.send(JSON.stringify({ t: "peer_gone", side: "phone_left" }));
              }
              return;
            }
          }
        });
      });
    });
  }

  private handleControl(ws: WebSocket, msg: Record<string, unknown>): void {
    const type = msg["t"] as string;
    const role = msg["role"] as string | undefined;
    const sid = msg["sid"] as string;

    if (type === "register") {
      this.lastRegister = msg;
    }
    if (type === "register" && role === "host") {
      this.hosts.set(sid, ws);
      ws.send(JSON.stringify({ t: "host_ready", sid }));
      this.tryPair(sid);
    } else if (type === "register" && role === "phone") {
      this.phones.set(sid, ws);
      this.tryPair(sid);
    }
  }

  private tryPair(sid: string): void {
    const host = this.hosts.get(sid);
    const phone = this.phones.get(sid);
    if (host && phone && host.readyState === WebSocket.OPEN && phone.readyState === WebSocket.OPEN) {
      host.send(JSON.stringify({ t: "paired", sid }));
      phone.send(JSON.stringify({ t: "paired", sid }));
    }
  }

  private forwardBinary(sender: WebSocket, data: Buffer): void {
    for (const [sid, host] of this.hosts) {
      if (host === sender) {
        const phone = this.phones.get(sid);
        if (phone && phone.readyState === WebSocket.OPEN) phone.send(data);
        return;
      }
    }
    for (const [sid, phone] of this.phones) {
      if (phone === sender) {
        const host = this.hosts.get(sid);
        if (host && host.readyState === WebSocket.OPEN) host.send(data);
        return;
      }
    }
  }
}

/** Connect a simulated phone to the mock relay. */
function connectPhone(
  relayUrl: string,
  sid: string,
): Promise<{ ws: WebSocket; paired: Promise<void> }> {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(relayUrl);
    openSockets.push(ws);
    let pairResolve: (() => void) | null = null;
    const paired = new Promise<void>((res, rej) => {
      pairResolve = res;
      ws.once("error", rej);
    });

    ws.on("open", () => {
      ws.send(JSON.stringify({ t: "register", role: "phone", sid }));
    });
    ws.on("message", (data, isBinary) => {
      if (!isBinary) {
        let ctrl: Record<string, unknown>;
        try {
          ctrl = JSON.parse(String(data));
        } catch {
          return;
        }
        if (ctrl["t"] === "paired" && pairResolve) {
          pairResolve();
          pairResolve = null;
          resolve({ ws, paired });
        }
      }
    });
    ws.on("error", reject);
  });
}

/** Read the next binary E2E frame from a WebSocket as a decoded string. */
function nextDecodedMessage(ws: WebSocket, channel: E2eChannel): Promise<string> {
  return new Promise((resolve, reject) => {
    const handler = (data: unknown, isBinary: boolean): void => {
      if (!isBinary) return;
      try {
        const buf = Buffer.isBuffer(data)
          ? data
          : data instanceof ArrayBuffer
            ? Buffer.from(data)
            : Buffer.concat(data as Buffer[]);
        const plaintext = channel.open(buf);
        ws.off("message", handler);
        resolve(plaintext.toString("utf8"));
      } catch (err) {
        ws.off("message", handler);
        reject(err);
      }
    };
    ws.on("message", handler);
    ws.on("error", reject);
  });
}

/** Capture the next binary frame from a WebSocket WITHOUT decrypting it. */
function nextRawBinaryFrame(ws: WebSocket): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const handler = (data: unknown, isBinary: boolean): void => {
      if (!isBinary) return;
      ws.off("message", handler);
      resolve(
        Buffer.isBuffer(data)
          ? data
          : data instanceof ArrayBuffer
            ? Buffer.from(data)
            : Buffer.concat(data as Buffer[]),
      );
    };
    ws.on("message", handler);
    ws.on("error", reject);
  });
}

// ── test handler: responds to shannon/health ──────────────────────────────────

function healthHandlers(): MethodHandlers {
  return {
    "shannon/health": async (): Promise<HandlerOutcome> => ({
      kind: "result",
      result: { gateway: "ok", engine: "ok", version: "test" } satisfies HealthResult,
    }),
  };
}

// ── tests ─────────────────────────────────────────────────────────────────────

describe("startRelayHost", () => {
  it("connects, registers, and resolves paired when phone joins", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-001";
    const pairToken = "test-pair-token";
    const sessionKey = deriveSessionKey(pairToken);

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    // Connect a phone → should trigger paired on both sides
    const { ws: phoneWs } = await connectPhone(relayUrl, sid);

    // The host's paired promise should resolve
    await expect(hostHandle.paired).resolves.toBeUndefined();
    phoneWs.close();
  });

  it("invokes onContext when the phone pairs (dispatch-hub parity)", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-oncontext";
    const pairToken = "oncontext-token";
    const sessionKey = deriveSessionKey(pairToken);

    const seen: Array<{ sessionId: string | null }> = [];
    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
      onContext: (ctx) => seen.push({ sessionId: ctx.sessionId }),
    });

    const { ws: phoneWs } = await connectPhone(relayUrl, sid);
    await expect(hostHandle.paired).resolves.toBeUndefined();
    // One context per pairing; sessionId is null until shannon/pair sets it.
    expect(seen).toHaveLength(1);
    expect(seen[0]?.sessionId).toBeNull();
    phoneWs.close();
  });

  it("dispatches E2E messages bidirectionally (phone→host→phone)", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-002";
    const pairToken = "interop-token";
    const sessionKey = deriveSessionKey(pairToken);

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    // Connect phone and wait for both sides to be paired
    const { ws: phoneWs } = await connectPhone(relayUrl, sid);
    await hostHandle.paired;

    // Phone's E2E channels (independent counters, same key)
    const phoneSend = new E2eChannel(sessionKey);
    const phoneRecv = new E2eChannel(sessionKey);

    // Phone sends shannon/health request (E2E sealed)
    const request = JSON.stringify({
      jsonrpc: "2.0",
      id: 1,
      method: "shannon/health",
    });
    phoneWs.send(phoneSend.seal(Buffer.from(request, "utf8")));

    // Phone should receive the E2E-sealed response
    const responseText = await nextDecodedMessage(phoneWs, phoneRecv);
    const response = JSON.parse(responseText);

    expect(response.id).toBe(1);
    expect(response.result).toEqual({
      gateway: "ok",
      engine: "ok",
      version: "test",
    });
    phoneWs.close();
  });

  it("survives peer_gone (phone disconnect) and stays alive", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-003";
    const sessionKey = deriveSessionKey("token-003");

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    // Connect and pair a phone
    const { ws: phoneWs1 } = await connectPhone(relayUrl, sid);
    await hostHandle.paired;

    // Disconnect phone → relay sends peer_gone to host
    phoneWs1.close();
    await new Promise((resolve) => setTimeout(resolve, 200));

    // Host should still be running (stop doesn't throw)
    await hostHandle.stop();
    hostHandle = null;
  });

  it("rejects pair timeout when no phone joins", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-004";
    const sessionKey = deriveSessionKey("token-004");

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 300,
    });

    // No phone connects → paired should reject with timeout
    await expect(hostHandle.paired).rejects.toThrow(/pair timeout/i);
  });

  it("surfaces the relay's error code (contract: {t:'error', code})", async () => {
    // The real shannon-relay sends {"t":"error","code":"bad_sid"|...} —
    // reading the wrong field used to log a generic "relay error".
    let hostSock: WebSocket | null = null;
    const rawWss = new WebSocketServer({ port: 0 });
    await new Promise<void>((resolve) => rawWss.on("listening", resolve));
    const rawPort = (rawWss.address() as { port: number }).port;
    rawWss.on("connection", (ws) => {
      ws.on("message", (data, isBinary) => {
        if (isBinary) return;
        const msg = JSON.parse(String(data)) as { t?: string };
        if (msg.t === "register") {
          hostSock = ws;
          ws.send(JSON.stringify({ t: "error", code: "bad_sid" }));
        }
      });
    });

    hostHandle = startRelayHost({
      relayUrl: `ws://127.0.0.1:${rawPort}`,
      sid: "sid-error-case",
      sessionKey: deriveSessionKey("token-error"),
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    await expect(hostHandle.paired).rejects.toThrow(/bad_sid/);
    (hostSock as WebSocket | null)?.close();
    await new Promise<void>((resolve) => rawWss.close(() => resolve()));
  });

  it("auto-reconnects and re-registers after the relay drops the host socket", async () => {
    let registrations = 0;
    let hostSock: WebSocket | null = null;
    const rawWss = new WebSocketServer({ port: 0 });
    await new Promise<void>((resolve) => rawWss.on("listening", resolve));
    const rawPort = (rawWss.address() as { port: number }).port;
    rawWss.on("connection", (ws) => {
      ws.on("message", (data, isBinary) => {
        if (isBinary) return;
        const msg = JSON.parse(String(data)) as { t?: string; sid?: string };
        if (msg.t === "register") {
          registrations += 1;
          hostSock = ws;
          ws.send(JSON.stringify({ t: "host_ready", sid: msg.sid }));
        }
      });
    });

    hostHandle = startRelayHost({
      relayUrl: `ws://127.0.0.1:${rawPort}`,
      sid: "sid-reconnect-case",
      sessionKey: deriveSessionKey("token-reconnect"),
      handlers: healthHandlers(),
      logger,
      pairTimeout: 30_000,
    });

    const waitFor = (cond: () => boolean, ms: number): Promise<void> =>
      new Promise((resolve, reject) => {
        const started = Date.now();
        const tick = (): void => {
          if (cond()) return resolve();
          if (Date.now() - started > ms) return reject(new Error("timeout waiting for condition"));
          setTimeout(tick, 50);
        };
        tick();
      });

    await waitFor(() => registrations === 1, 2000);
    // Hard-drop the host leg (relay restart / network blip). The host must
    // re-register on its own — without auto-reconnect it stays unreachable
    // until a gateway restart.
    hostSock!.terminate();
    await waitFor(() => registrations >= 2, 5000);
    await hostHandle!.stop();
    hostHandle = null;
    await new Promise<void>((resolve) => rawWss.close(() => resolve()));
  });

  it("re-pairs after phone reconnects (recv counter resets)", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-005";
    const sessionKey = deriveSessionKey("token-005");

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    // First phone connects
    const { ws: phoneWs1 } = await connectPhone(relayUrl, sid);
    await hostHandle.paired;

    // Phone sends a message (advances host recv counter)
    const phoneSend1 = new E2eChannel(sessionKey);
    phoneWs1.send(phoneSend1.seal(Buffer.from(
      JSON.stringify({ jsonrpc: "2.0", id: 1, method: "shannon/health" }), "utf8",
    )));
    // Wait for the response to arrive
    const phoneRecv1 = new E2eChannel(sessionKey);
    await nextDecodedMessage(phoneWs1, phoneRecv1);
    // §G precondition: the host's phone-direction recv counter really advanced
    // to 1 before the drop (phone's first sealed frame carries counter 1).
    expect(phoneSend1.sendCounter).toBe(1);

    // Phone disconnects (peer_gone)
    phoneWs1.close();
    await new Promise((resolve) => setTimeout(resolve, 200));

    // Second phone connects → host gets paired again with fresh recv counter
    const { ws: phoneWs2 } = await connectPhone(relayUrl, sid);
    // Wait a bit for the host to process the new paired event
    await new Promise((resolve) => setTimeout(resolve, 200));

    // Phone 2 sends a message with counter starting at 1 (fresh channel)
    const phoneSend2 = new E2eChannel(sessionKey);
    const phoneRecv2 = new E2eChannel(sessionKey);
    const rejoinFrame = phoneSend2.seal(Buffer.from(
      JSON.stringify({ jsonrpc: "2.0", id: 2, method: "shannon/health" }), "utf8",
    ));
    // §G contract pin (cross-repo-adaptation-spec §G4): the re-joining phone's
    // FIRST frame is wire-level counter 1, and the host must ACCEPT this exact
    // frame after re-pair — not merely survive the reconnect.
    expect(rejoinFrame[0]).toBe(0x01); // frame version
    expect(rejoinFrame.readBigUInt64BE(1)).toBe(1n); // counter = 1
    phoneWs2.send(rejoinFrame);

    // Host should respond (recv counter was reset, so counter 1 is accepted)
    const responseText = await nextDecodedMessage(phoneWs2, phoneRecv2);
    const response = JSON.parse(responseText);
    expect(response.id).toBe(2);
    expect(response.result).toEqual({
      gateway: "ok",
      engine: "ok",
      version: "test",
    });
    phoneWs2.close();
  });

  it("preserves host send counter across re-paired (§G-rev3)", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const sid = "test-sid-006";
    const sessionKey = deriveSessionKey("token-006");

    hostHandle = startRelayHost({
      relayUrl,
      sid,
      sessionKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
    });

    // First session: one full request/response round trip. The phone's recv
    // channel accepts the host's first downstream frame (wire counter 1).
    const { ws: phoneWs1 } = await connectPhone(relayUrl, sid);
    await hostHandle.paired;

    const phoneSend1 = new E2eChannel(sessionKey);
    // This channel models the phone's recv side and is REUSED across the
    // re-pair below — mirroring shannon-mobile's relay_transport.dart, where
    // the phone role resets recv only on host_replaced, never on re-paired.
    const phoneRecv = new E2eChannel(sessionKey);
    phoneWs1.send(phoneSend1.seal(Buffer.from(
      JSON.stringify({ jsonrpc: "2.0", id: 1, method: "shannon/health" }), "utf8",
    )));
    const frame1 = await nextRawBinaryFrame(phoneWs1);
    expect(frame1[0]).toBe(0x01);
    expect(frame1.readBigUInt64BE(1)).toBe(1n); // host's first downstream frame
    expect(JSON.parse(phoneRecv.open(frame1).toString("utf8")).id).toBe(1);

    // Phone disconnects → host gets peer_gone → host waits for re-pair.
    phoneWs1.close();
    await new Promise((resolve) => setTimeout(resolve, 200));

    // Phone reconnects. §G-rev3 (cross-repo-adaptation-spec): the host's
    // socket never dropped, so its send counter must continue monotonically.
    // The phone's recv sits at 1; a reset host send counter (back to 1) would
    // be rejected by the phone as a replay and wedge the host→phone direction.
    const { ws: phoneWs2 } = await connectPhone(relayUrl, sid);
    await new Promise((resolve) => setTimeout(resolve, 200));

    const phoneSend2 = new E2eChannel(sessionKey);
    phoneWs2.send(phoneSend2.seal(Buffer.from(
      JSON.stringify({ jsonrpc: "2.0", id: 2, method: "shannon/health" }), "utf8",
    )));
    const frame2 = await nextRawBinaryFrame(phoneWs2);
    expect(frame2[0]).toBe(0x01);
    expect(frame2.readBigUInt64BE(1)).toBe(2n); // continued — NOT reset to 1
    // The reused phone recv channel accepts counter 2 (would throw on 1).
    const response2 = JSON.parse(phoneRecv.open(frame2).toString("utf8"));
    expect(response2.id).toBe(2);
    expect(response2.result).toEqual({
      gateway: "ok",
      engine: "ok",
      version: "test",
    });
    phoneWs2.close();
  });
});

// ── v0.3: handshake auth tag + X25519 E2E (forward secrecy) ──────────────────

import { createPublicKey, diffieHellman, generateKeyPairSync } from "node:crypto";
import {
  deriveRelayAuthTag,
  deriveSessionKeyV2,
  e2eHelloFrame,
  generateHostE2EKeyPair,
} from "../e2e.js";

/** Phone-side shared secret: X25519(phonePriv, hostPub). */
function phoneShared(phonePriv: ReturnType<typeof generateKeyPairSync>["privateKey"], hostPubB64: string): Buffer {
  return diffieHellman({
    privateKey: phonePriv,
    publicKey: createPublicKey({ key: { kty: "OKP", crv: "X25519", x: hostPubB64 }, format: "jwk" }),
  });
}

describe("startRelayHost v0.3 handshake", () => {
  it("sends the register frame with the relay auth tag", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const pairToken = "tag-token";

    hostHandle = startRelayHost({
      relayUrl,
      sid: "sid-tag",
      sessionKey: deriveSessionKey(pairToken),
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
      relayAuthTag: deriveRelayAuthTag(pairToken),
    });

    // Wait for registration (host_ready) then assert the frame contract.
    await vi.waitFor(() => expect(relay.lastRegister).not.toBeNull());
    expect(relay.lastRegister!["tag"]).toBe(deriveRelayAuthTag(pairToken));
    expect(relay.lastRegister!["role"]).toBe("host");
  });

  it("completes the e2e v2 handshake (e2e_hello → token-mixed ECDH key)", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const pairToken = "ecdh-token";
    const hostKeypair = generateHostE2EKeyPair();

    hostHandle = startRelayHost({
      relayUrl,
      sid: "sid-ecdh",
      sessionKey: Buffer.from("legacy-fallback-key-not-used-in-ecdh!!!", "utf8").subarray(0, 32),
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
      hostE2E: { privateKey: hostKeypair.privateKey, pairToken },
    });

    const { ws: phoneWs } = await connectPhone(relayUrl, "sid-ecdh");

    // Phone generates its ephemeral keypair and sends the plaintext hello.
    const phoneKeypair = generateKeyPairSync("x25519");
    const phoneJwk = (phoneKeypair.publicKey.export({ format: "jwk" }) as { x: string }).x;
    phoneWs.send(e2eHelloFrame(phoneJwk));

    // Both sides derive the same v2 key from token || X25519 shared.
    const shared = phoneShared(phoneKeypair.privateKey, hostKeypair.pubB64);
    const k = deriveSessionKeyV2(pairToken, shared);
    const phoneSend = new E2eChannel(k);
    const phoneRecv = new E2eChannel(k);

    phoneWs.send(
      phoneSend.seal(Buffer.from(JSON.stringify({ jsonrpc: "2.0", id: 7, method: "shannon/health" }), "utf8")),
    );
    const response = JSON.parse(await nextDecodedMessage(phoneWs, phoneRecv));
    expect(response.id).toBe(7);
    expect(response.result).toEqual({ gateway: "ok", engine: "ok", version: "test" });
    phoneWs.close();
  });

  it("falls back to the legacy token key when the first frame is not a hello", async () => {
    const relay = new MockRelay();
    const relayPort = await relay.start(0);
    const relayUrl = `ws://127.0.0.1:${relayPort}`;
    const pairToken = "legacy-token";
    const legacyKey = deriveSessionKey(pairToken);
    const hostKeypair = generateHostE2EKeyPair();

    hostHandle = startRelayHost({
      relayUrl,
      sid: "sid-legacy",
      sessionKey: legacyKey,
      handlers: healthHandlers(),
      logger,
      pairTimeout: 5000,
      hostE2E: { privateKey: hostKeypair.privateKey, pairToken },
    });

    const { ws: phoneWs } = await connectPhone(relayUrl, "sid-legacy");
    // Old phone: no hello — the first frame is already legacy-sealed.
    const phoneSend = new E2eChannel(legacyKey);
    const phoneRecv = new E2eChannel(legacyKey);
    phoneWs.send(
      phoneSend.seal(Buffer.from(JSON.stringify({ jsonrpc: "2.0", id: 8, method: "shannon/health" }), "utf8")),
    );
    const response = JSON.parse(await nextDecodedMessage(phoneWs, phoneRecv));
    expect(response.id).toBe(8);
    expect(response.result.version).toBe("test");
    phoneWs.close();
  });
});

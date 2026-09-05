import { afterEach, describe, expect, it } from "vitest";
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

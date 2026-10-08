/**
 * N3 per-kind trust face (cross-repo spec §Q): `shannon/approval/decide`
 * `scope: "kind"` and the `shannon/trust.list` / `shannon/trust.revoke`
 * methods. Covers the wire shapes (engine POST body, camelCase projection),
 * the signature binding (`:kind:<kind>` suffix, v1 + v2), the param gates
 * (kind required on kind scope; no scope on deny), the pairing gate, and the
 * honest degrade paths (broken engine payload → empty list; idempotent
 * revoke).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import { ShannonError } from "../protocol.js";
import { MobileServer, type MethodHandlers } from "../server.js";
import { createEngineHandlers } from "../engineBridge.js";
import { approvalMessage, approvalMessageV2 } from "../crypto.js";

const logger = createConsoleLogger("error");

let servers: { stop: () => Promise<void> }[] = [];
afterEach(async () => {
  await Promise.all(servers.map((s) => s.stop().catch(() => {})));
  servers = [];
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

function mockResponse(status: number, body = ""): Response {
  return {
    status,
    ok: status >= 200 && status < 300,
    text: async () => body,
    json: async () => JSON.parse(body),
  } as unknown as Response;
}

/** Open-mode bridge (requireSession off) + a fetch capture. */
function openHandlers(fetchMock: typeof fetch): MethodHandlers {
  return createEngineHandlers({
    engineWsUrl: "ws://127.0.0.1:9",
    engineHttpBaseUrl: "http://engine:33420",
    version: "test",
    logger,
    fetchImpl: fetchMock,
  });
}

/** Gated bridge over a "bound" device whose verifier the test controls. */
function openGatedHandlers(verify: (_deviceId: string, message: string) => boolean): MethodHandlers {
  return createEngineHandlers({
    engineWsUrl: "ws://127.0.0.1:9",
    engineHttpBaseUrl: "http://engine:33420",
    version: "test",
    logger,
    requireSession: true,
    verifyDeviceSignature: verify,
    fetchImpl: async () => mockResponse(200, ""),
  });
}

/** Minimal bound-device MethodContext for direct handler invocation. */
function fakeCtx(): import("../server.js").MethodContext {
  return {
    // Handlers under test never touch the socket; a inert stand-in keeps the
    // test free of live connection noise.
    socket: { readyState: 0 } as unknown as WebSocket,
    sessionId: "device-under-test",
    logger,
  };
}


// ── decide scope="kind" ──────────────────────────────────────────────────────

describe("approval/decide scope=kind", () => {
  it("POSTs the externally-tagged always_allow_kind choice to the engine", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = openHandlers(fetchMock as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/approval/decide", {
      request_id: "r1",
      choice: "allow",
      scope: "kind",
      kind: "Bash",
      signature: "sig",
    });
    expect(res.result).toEqual({ ok: true });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0]!;
    expect(String(url)).toBe("http://engine:33420/api/approval/respond");
    expect(JSON.parse((init as RequestInit).body as string)).toEqual({
      request_id: "r1",
      choice: { always_allow_kind: { kind: "Bash" } },
    });
    socket.close();
  });

  it("rejects a kind scope without a kind (or with an unsane kind)", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = openHandlers(fetchMock as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    for (const kind of [undefined, "", "  ", " Bash", `${"k".repeat(129)}`]) {
      const res = await rpc(socket, "shannon/approval/decide", {
        request_id: "r2",
        choice: "allow",
        scope: "kind",
        kind,
        signature: "sig",
      });
      expect(res.error?.code).toBe(ShannonError.BAD_PARAMS);
    }
    expect(fetchMock).not.toHaveBeenCalled();
    socket.close();
  });

  it("rejects a kind scope on deny (scope only rides allow)", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = openHandlers(fetchMock as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/approval/decide", {
      request_id: "r3",
      choice: "deny",
      scope: "kind",
      kind: "Bash",
      signature: "sig",
    });
    expect(res.error?.code).toBe(ShannonError.BAD_PARAMS);
    expect(fetchMock).not.toHaveBeenCalled();
    socket.close();
  });

  it("verifies the decision signature over the kind-bound message bytes", async () => {
    // Direct handler invocation with a bound-device context (the pairing
    // layer sets ctx.sessionId in production; the socket round-trip adds
    // nothing to this assertion).
    const seen: { message: string }[] = [];
    const handlers = openGatedHandlers((_deviceId, message) => {
      seen.push({ message });
      return message === approvalMessage("r4", "allow", "kind", "Bash");
    });
    const decide = handlers["shannon/approval/decide"]!;
    const ctx = fakeCtx();
    const ok = await decide(
      { request_id: "r4", choice: "allow", scope: "kind", kind: "Bash", signature: "sig" },
      ctx,
    );
    expect(ok).toEqual({ kind: "result", result: { ok: true } });
    expect(seen.map((s) => s.message)).toEqual(["r4:allow:kind:Bash"]);

    // The kind is PART of the verified bytes: a signature over the bare
    // once-message must not verify as a kind grant.
    seen.length = 0;
    const forged = await decide(
      { request_id: "r5", choice: "allow", scope: "kind", kind: "Bash", signature: "sig" },
      ctx,
    );
    expect(forged).toMatchObject({ kind: "error", code: ShannonError.BAD_PARAMS });
    expect(seen[0]?.message).toBe("r5:allow:kind:Bash");

    // v2: the timestamp rides between choice and the scope suffix.
    const v2 = await decide(
      {
        request_id: "r6",
        choice: "allow",
        scope: "kind",
        kind: "Write",
        timestamp: Date.now(),
        signature: "sig",
      },
      ctx,
    );
    void v2;
    expect(seen.at(-1)?.message).toMatch(/^r6:allow:\d+:kind:Write$/);
  });

  it("kind trust rides the same pairing gate as every session-scoped call", async () => {
    const handlers = createEngineHandlers({
      engineWsUrl: "ws://127.0.0.1:9",
      engineHttpBaseUrl: "http://engine:33420",
      version: "test",
      logger,
      requireSession: true,
      fetchImpl: async () => mockResponse(200, ""),
    });
    const { port } = await start(handlers);
    const socket = await connect(port);
    const listed = await rpc(socket, "shannon/trust.list", {});
    expect(listed.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    const revoked = await rpc(socket, "shannon/trust.revoke", { kind: "Bash" });
    expect(revoked.error?.code).toBe(ShannonError.PAIRING_REQUIRED);
    socket.close();
  });
});

// ── trust.list / trust.revoke ────────────────────────────────────────────────

describe("shannon/trust.list", () => {
  it("proxies the engine face and projects to the camelCase wire shape", async () => {
    const calls: { url: string; method?: string }[] = [];
    const fetchMock = vi.fn<typeof fetch>(async (url, init) => {
      calls.push({ url: String(url), method: (init as RequestInit | undefined)?.method });
      return mockResponse(200, JSON.stringify({
        kinds: [
          { kind: "Write", granted_at: 1760000000100 },
          { kind: "Bash", granted_at: 1760000000000 },
        ],
      }));
    });
    const handlers = openHandlers(fetchMock as unknown as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/trust.list", {});
    expect(res.error).toBeUndefined();
    expect(res.result).toEqual({
      kinds: [
        { kind: "Write", grantedAt: new Date(1760000000100).toISOString() },
        { kind: "Bash", grantedAt: new Date(1760000000000).toISOString() },
      ],
    });
    expect(calls).toEqual([{ url: "http://engine:33420/api/trust/kinds", method: "GET" }]);
    socket.close();
  });

  it("degrades a broken engine payload to an honest empty list", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () =>
      mockResponse(200, JSON.stringify({ kinds: [{ kind: "Bash" }, null, 42, { kind: "", granted_at: 1 }, { kind: "Edit", granted_at: "soon" }] })),
    );
    const handlers = openHandlers(fetchMock as unknown as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/trust.list", {});
    expect(res.result).toEqual({ kinds: [] });
    socket.close();
  });

  it("surfaces an engine failure as ENGINE_ERROR (never a fake empty ok)", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(500, "boom"));
    const handlers = openHandlers(fetchMock as unknown as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/trust.list", {});
    expect(res.error?.code).toBe(ShannonError.ENGINE_ERROR);
    socket.close();
  });
});

describe("shannon/trust.revoke", () => {
  it("forwards the kind and answers ok even when the engine had nothing to revoke", async () => {
    const calls: { url: string; method?: string; body?: string }[] = [];
    const fetchMock = vi.fn<typeof fetch>(async (url, init) => {
      calls.push({
        url: String(url),
        method: (init as RequestInit | undefined)?.method,
        body: (init as RequestInit | undefined)?.body as string | undefined,
      });
      return mockResponse(200, JSON.stringify({ kind: "Bash", revoked: false }));
    });
    const handlers = openHandlers(fetchMock as unknown as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/trust.revoke", { kind: "Bash" });
    expect(res.result).toEqual({ ok: true });
    expect(calls).toEqual([{
      url: "http://engine:33420/api/trust/revoke",
      method: "POST",
      body: JSON.stringify({ kind: "Bash" }),
    }]);
    socket.close();
  });

  it("rejects a missing kind with BAD_PARAMS before touching the engine", async () => {
    const fetchMock = vi.fn<typeof fetch>(async () => mockResponse(200, ""));
    const handlers = openHandlers(fetchMock as unknown as typeof fetch);
    const { port } = await start(handlers);
    const socket = await connect(port);
    const res = await rpc(socket, "shannon/trust.revoke", {});
    expect(res.error?.code).toBe(ShannonError.BAD_PARAMS);
    expect(fetchMock).not.toHaveBeenCalled();
    socket.close();
  });
});

// ── capability surface ───────────────────────────────────────────────────────

describe("trust capability surface", () => {
  it("exposes both methods in the runtime method list (schema pins the enum)", async () => {
    const { SHANNON_METHODS } = await import("../protocol.js");
    expect(SHANNON_METHODS).toContain("shannon/trust.list");
    expect(SHANNON_METHODS).toContain("shannon/trust.revoke");
  });

  it("binds the kind into the v2 signature bytes", () => {
    expect(approvalMessage("req-1", "allow", "kind", "Bash")).toBe("req-1:allow:kind:Bash");
    expect(approvalMessageV2("req-1", "allow", 1767225600000, "kind", "Bash")).toBe(
      "req-1:allow:1767225600000:kind:Bash",
    );
    // No kind param → no suffix (and a session scope keeps its P3-3 shape).
    expect(approvalMessage("req-1", "allow", "kind", undefined)).toBe("req-1:allow");
    expect(approvalMessage("req-1", "allow", "session")).toBe("req-1:allow:session");
    expect(approvalMessage("req-1", "allow")).toBe("req-1:allow");
  });
});

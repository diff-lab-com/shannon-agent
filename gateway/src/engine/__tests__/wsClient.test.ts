import { afterEach, describe, expect, it, vi } from "vitest";
import net from "node:net";
import { type AddressInfo, WebSocketServer, type WebSocket } from "ws";

import { EngineWsClient } from "../wsClient.js";
import { type EngineEvent } from "../runtime.js";
import { PROTOCOL_VERSION } from "../types.gen.js";

/**
 * The client is exercised against a real `ws` server speaking the engine's
 * `{ "type": "..." }` protocol — the same wire shape `api_server.rs` emits.
 * No LLM, no network egress.
 */

interface MockServer {
  url: string;
  close: () => Promise<void>;
}

const openServers: MockServer[] = [];

async function startMockServer(
  handler: (ws: WebSocket) => void,
): Promise<MockServer> {
  const server = new WebSocketServer({ port: 0 });
  await new Promise<void>((resolve) => server.on("listening", resolve));
  server.on("connection", handler);
  const addr = server.address() as AddressInfo;
  const mock: MockServer = {
    url: `ws://127.0.0.1:${addr.port}`,
    close: () =>
      new Promise<void>((resolve) => server.close(() => resolve())),
  };
  openServers.push(mock);
  return mock;
}

afterEach(async () => {
  while (openServers.length > 0) {
    const s = openServers.pop();
    if (s) await s.close();
  }
});

function send(ws: WebSocket, frame: object): void {
  ws.send(JSON.stringify(frame));
}

function onQuery(ws: WebSocket, cb: () => void): void {
  ws.on("message", (data) => {
    const msg = JSON.parse(data.toString("utf8"));
    if (msg?.type === "query") cb();
  });
}

describe("EngineWsClient", () => {
  it("yields text → usage → completed then ends", async () => {
    const server = await startMockServer((ws) => {
      onQuery(ws, () => {
        send(ws, { type: "text", content: "Hello" });
        send(ws, {
          type: "usage",
          input_tokens: 5,
          output_tokens: 3,
          cost_usd: 0.0001,
        });
        send(ws, { type: "completed", model: "gpt-test" });
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    const events: EngineEvent[] = [];
    for await (const ev of client.runQuery("hi")) events.push(ev);

    expect(events.map((e) => e.type)).toEqual([
      "text",
      "usage",
      "completed",
    ]);
    const last = events[2];
    expect(last && last.type === "completed" && last.model).toBe("gpt-test");

    await client.close();
  });

  it("yields failed and ends", async () => {
    const server = await startMockServer((ws) => {
      onQuery(ws, () => send(ws, { type: "failed", error: "boom" }));
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    const types: string[] = [];
    for await (const ev of client.runQuery("hi")) types.push(ev.type);

    expect(types).toEqual(["failed"]);
    await client.close();
  });

  it("cancel() sends a cancel frame and yields cancelled", async () => {
    const server = await startMockServer((ws) => {
      ws.on("message", (data) => {
        const msg = JSON.parse(data.toString("utf8"));
        if (msg?.type === "query") {
          send(ws, { type: "text", content: "partial" });
        } else if (msg?.type === "cancel") {
          send(ws, { type: "cancelled" });
        }
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    const types: string[] = [];
    for await (const ev of client.runQuery("hi")) {
      types.push(ev.type);
      if (ev.type === "text") client.cancel();
    }

    expect(types).toEqual(["text", "cancelled"]);
    await client.close();
  });

  it("runQuery throws if not connected", async () => {
    const client = new EngineWsClient({ url: "ws://127.0.0.1:1" });
    await expect(async () => {
      for await (const _ of client.runQuery("hi")) {
        void _;
      }
    }).rejects.toThrow(/not connected/);
  });

  it("rejects a second concurrent query on the same client", async () => {
    const server = await startMockServer(() => {
      // Never reply — keeps the first query pending.
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    const firstIter = client.runQuery("first");
    // Starts the generator body: sends the query frame, then awaits a frame.
    const firstNext = firstIter.next().catch(() => {});
    await new Promise((r) => setImmediate(r));

    const secondIter = client.runQuery("second");
    await expect(secondIter.next()).rejects.toThrow(/already in flight/);

    await client.close();
    await firstNext;
  });

  it("connect() is idempotent", async () => {
    const server = await startMockServer(() => {
      /* unused */
    });
    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    await client.connect();
    expect(client.isConnected).toBe(true);
    await client.close();
  });

  // review §P2-23: a wedged engine accept (TCP connects, upgrade never
  // completes) must surface as a normal error so reconnect logic takes over,
  // not a forever-pending connect().
  it("connect() rejects after the handshake timeout against a silent server", async () => {
    // Raw TCP server: accepts connections but never speaks WebSocket.
    const server = net.createServer((socket) => {
      // Swallow everything; never respond.
      socket.on("data", () => {});
    });
    await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
    const addr = server.address() as AddressInfo;

    const client = new EngineWsClient({
      url: `ws://127.0.0.1:${addr.port}`,
      handshakeTimeoutMs: 100,
    });
    await expect(client.connect()).rejects.toThrow(/handshake timed out/);
    expect(client.isConnected).toBe(false);

    await new Promise<void>((resolve) => server.close(() => resolve()));
  });

  it("connect() succeeds well within the default handshake budget", async () => {
    const server = await startMockServer(() => {
      /* unused */
    });
    const client = new EngineWsClient({ url: server.url });
    await expect(client.connect()).resolves.toBeUndefined();
    await client.close();
  });
});

// ── §P2-24: greeting / protocol-version negotiation ─────────────────────

/**
 * Wait until `probe()` stops returning `null` (the greeting arrives
 * asynchronously after the WS handshake completes).
 */
async function waitForVersion(client: EngineWsClient): Promise<string | null> {
  for (let i = 0; i < 100; i++) {
    const version = client.protocolVersion;
    if (version !== null) return version;
    await new Promise((r) => setImmediate(r));
  }
  return client.protocolVersion;
}

/** Wait until `mock` has observed at least one call (bounded). */
async function waitForWarnCall(mock: {
  mock: { calls: unknown[][] };
}): Promise<void> {
  for (let i = 0; i < 100 && mock.mock.calls.length === 0; i++) {
    await new Promise((r) => setImmediate(r));
  }
}

describe("EngineWsClient greeting consumption (§P2-24)", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("captures protocol_version from the unsolicited greeting frame", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    const server = await startMockServer((ws) => {
      send(ws, {
        type: "session_info",
        message_count: 0,
        model: null,
        protocol_version: PROTOCOL_VERSION,
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    expect(await waitForVersion(client)).toBe(PROTOCOL_VERSION);
    await client.close();
  });

  it("does not warn when the engine's major version matches", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    const server = await startMockServer((ws) => {
      // Same major (0), different minor/patch — additive, no warning.
      send(ws, {
        type: "session_info",
        message_count: 0,
        model: null,
        protocol_version: "0.999.0",
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    expect(await waitForVersion(client)).toBe("0.999.0");
    expect(warn).not.toHaveBeenCalled();
    await client.close();
  });

  it("warns on a major-version mismatch without dropping the connection", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    const server = await startMockServer((ws) => {
      send(ws, {
        type: "session_info",
        message_count: 0,
        model: null,
        protocol_version: "1.0.0",
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    expect(await waitForVersion(client)).toBe("1.0.0");
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0] ?? "")).toMatch(/major version mismatch/);
    // Policy: warn, never hard-fail (minor-cycle compatibility).
    expect(client.isConnected).toBe(true);
    await client.close();
  });

  it("warns when a legacy engine omits protocol_version from the greeting", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    const server = await startMockServer((ws) => {
      send(ws, { type: "session_info", message_count: 0, model: null });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    await waitForWarnCall(warn);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(String(warn.mock.calls[0]?.[0] ?? "")).toMatch(/no protocol_version/);
    await client.close();
  });

  it("routes a mid-query session_info response to the consumer AND updates the version", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    vi.spyOn(console, "info").mockImplementation(() => {});
    const server = await startMockServer((ws) => {
      onQuery(ws, () => {
        send(ws, {
          type: "session_info",
          message_count: 3,
          model: "gpt-test",
          protocol_version: "1.0.0",
        });
        send(ws, { type: "completed", model: "gpt-test" });
      });
    });

    const client = new EngineWsClient({ url: server.url });
    await client.connect();
    const events: EngineEvent[] = [];
    for await (const ev of client.runQuery("hi")) events.push(ev);

    expect(events.map((e) => e.type)).toEqual(["session_info", "completed"]);
    expect(client.protocolVersion).toBe("1.0.0");
    expect(warn).toHaveBeenCalledTimes(1);
    await client.close();
  });
});

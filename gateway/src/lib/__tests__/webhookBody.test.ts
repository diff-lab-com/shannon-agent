import { afterEach, describe, expect, it } from "vitest";
import net from "node:net";
import { createServer, type Server, type IncomingMessage, type ServerResponse } from "node:http";
import { type AddressInfo } from "node:net";

import {
  MAX_WEBHOOK_BODY_BYTES,
  WebhookBodyTooLargeError,
  readWebhookBody,
  readWebhookBodyOr413,
} from "../webhookBody.js";

/**
 * review §P2-23: webhook entry points must bound the raw body *before*
 * signature verification. These tests drive a real `node:http` server whose
 * handler uses the shared reader, so chunked bodies, oversized
 * Content-Lengths and the 413 path are all exercised over actual sockets.
 */

let server: Server | null = null;

function listen(
  handler: (req: IncomingMessage, res: ServerResponse) => void,
): Promise<string> {
  return new Promise((resolve) => {
    server = createServer(handler);
    server.listen(0, "127.0.0.1", () => {
      const addr = server!.address() as AddressInfo;
      resolve(`http://127.0.0.1:${addr.port}`);
    });
  });
}

async function stop(): Promise<void> {
  if (!server) return;
  await new Promise<void>((resolve) => server!.close(() => resolve()));
  server = null;
}

/** Send a raw HTTP/1.1 request over a socket (so Content-Length can lie). */
async function postRaw(
  base: string,
  headers: string,
  body: string,
): Promise<string> {
  const addr = new URL(base);
  return await new Promise<string>((resolve) => {
    const socket = net.connect(Number(addr.port), "127.0.0.1", () => {
      socket.write(`POST / HTTP/1.1\r\nHost: x\r\n${headers}\r\n${body}`);
      socket.end();
    });
    let response = "";
    socket.on("data", (d: Buffer) => {
      response += d.toString("utf8");
    });
    socket.on("close", () => resolve(response));
    setTimeout(() => socket.destroy(), 2000);
  });
}

describe("readWebhookBody (via real server)", () => {
  afterEach(stop);

  it("accepts a body under the cap", async () => {
    let received: string | null = null;
    const base = await listen((req, res) => {
      void readWebhookBody(req).then((raw) => {
        received = raw;
        res.end("ok");
      });
    });
    const payload = JSON.stringify({ hello: "world" });
    const res = await fetch(base, { method: "POST", body: payload });
    expect(res.status).toBe(200);
    expect(received).toBe(payload);
  });

  it("rejects a body that grows past the cap mid-stream", async () => {
    let error: unknown = null;
    const base = await listen((req, res) => {
      void readWebhookBody(req, 64).then(
        () => {
          res.end("ok");
        },
        (err: unknown) => {
          error = err;
          res.statusCode = 413;
          res.end("too large");
        },
      );
    });

    // Proper chunked framing: two 64-byte chunks = 128 bytes total > cap 64.
    const chunk = "z".repeat(64);
    const response = await postRaw(
      base,
      "Content-Type: text/plain\r\nTransfer-Encoding: chunked\r\n",
      `${chunk.length.toString(16)}\r\n${chunk}\r\n${chunk.length.toString(16)}\r\n${chunk}\r\n0\r\n\r\n`,
    );
    expect(error).toBeInstanceOf(WebhookBodyTooLargeError);
    expect(response).toContain("413");
  });

  it("rejects up-front on a Content-Length that already exceeds the cap", async () => {
    let error: unknown = null;
    const base = await listen((req, res) => {
      void readWebhookBody(req, 64).then(
        () => {
          res.end("ok");
        },
        (err: unknown) => {
          error = err;
          res.statusCode = 413;
          res.end("too large");
        },
      );
    });
    // A lying/oversized Content-Length is rejected before any body bytes.
    const response = await postRaw(
      base,
      "Content-Type: text/plain\r\nContent-Length: 99999\r\n",
      "only a little",
    );
    expect(error).toBeInstanceOf(WebhookBodyTooLargeError);
    expect(response).toContain("413");
  });
});

describe("readWebhookBodyOr413 (adapter entry shape)", () => {
  afterEach(stop);

  it("answers 413 and lets the handler bail when the body is over the cap", async () => {
    const base = await listen((req, res) => {
      void readWebhookBodyOr413(req, res, 64).then((raw) => {
        if (raw === null) return; // handler's own early return after 413
        res.end("ok");
      });
    });
    const response = await postRaw(
      base,
      "Content-Type: text/plain\r\nContent-Length: 200\r\n",
      "y".repeat(200),
    );
    expect(response).toContain("413");
  });

  it("passes normal bodies through", async () => {
    const base = await listen((req, res) => {
      void readWebhookBodyOr413(req, res).then((raw) => {
        if (raw === null) return;
        res.end(`got:${raw}`);
      });
    });
    const res = await fetch(base, { method: "POST", body: "hello" });
    expect(res.status).toBe(200);
    expect(await res.text()).toBe("got:hello");
  });

  it("caps at 1 MiB by default", () => {
    expect(MAX_WEBHOOK_BODY_BYTES).toBe(1024 * 1024);
  });
});

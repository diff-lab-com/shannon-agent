import { afterAll, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { WebSocket } from "ws";

import { createConsoleLogger } from "../../logger.js";
import { ensureTlsMaterial, mobileTlsPaths } from "../mobileTls.js";
import { MobileServer, type MethodHandlers } from "../server.js";
import { JSONRPC_VERSION } from "../protocol.js";

const logger = createConsoleLogger("error");
let servers: { stop: () => Promise<void> }[] = [];
let dirs: string[] = [];
afterAll(async () => {
  await Promise.all(servers.map((s) => s.stop().catch(() => {})));
  for (const d of dirs) rmSync(d, { recursive: true, force: true });
});

describe("mobileTls (v0.12 LAN hardening)", () => {
  it("generates, persists, and reloads self-signed material with matching fingerprint", () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-tls-"));
    dirs.push(dir);
    const paths = mobileTlsPaths(dir);

    const first = ensureTlsMaterial(paths);
    expect(first.key).toContain("PRIVATE KEY");
    expect(first.cert).toContain("BEGIN CERTIFICATE");
    // fingerprint = lowercase colon-less hex of the DER cert's SHA-256.
    expect(first.fingerprint).toMatch(/^[0-9a-f]{64}$/);

    // Reload: same material (no regeneration churn → phones' pins stay valid).
    const second = ensureTlsMaterial(paths);
    expect(second.cert).toBe(first.cert);
    expect(second.fingerprint).toBe(first.fingerprint);

    // The info file the desktop QR composer reads is present and consistent.
    const info = JSON.parse(
      require("node:fs").readFileSync(paths.infoPath, "utf8"),
    ) as { fingerprint: string };
    expect(info.fingerprint).toBe(first.fingerprint);
  });

  it("serves wss: a TLS (self-signed-tolerant) client completes an RPC", async () => {
    const dir = mkdtempSync(join(tmpdir(), "shannon-tls-"));
    dirs.push(dir);
    const tls = ensureTlsMaterial(mobileTlsPaths(dir));

    const handlers: MethodHandlers = {
      "shannon/health": async () => ({
        kind: "result",
        result: { gateway: "ok", engine: "ok", version: "test" },
      }),
    };
    const server = new MobileServer({
      host: "127.0.0.1",
      port: 0,
      logger,
      handlers,
      tls: { key: tls.key, cert: tls.cert },
    });
    const handle = await server.start();
    servers.push(handle);

    // Self-signed cert → the test client must skip chain validation (the
    // phone instead pins `tls.fingerprint` via onBadCertificate).
    const socket = new WebSocket(`wss://127.0.0.1:${handle.port}/`, {
      rejectUnauthorized: false,
    });
    await new Promise<void>((resolve, reject) => {
      socket.once("open", () => resolve());
      socket.once("error", reject);
    });
    const reply = await new Promise<any>((resolve) => {
      const onMsg = (d: unknown): void => {
        const msg = JSON.parse(String(d)) as any;
        if (msg.id === 1) socket.off("message", onMsg), resolve(msg);
      };
      socket.on("message", onMsg);
      socket.send(
        JSON.stringify({
          jsonrpc: JSONRPC_VERSION,
          id: 1,
          method: "shannon/health",
        }),
      );
    });
    expect(reply.result.gateway).toBe("ok");
    socket.close();
  });
});

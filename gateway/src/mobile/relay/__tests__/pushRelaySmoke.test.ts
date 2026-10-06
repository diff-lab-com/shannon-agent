/**
 * §O3 cross-repo live smoke: mono's REAL desktop push stack (startRelayHost +
 * PushRelayBinding, wired exactly like bootstrap.ts) driven against the REAL
 * shannon-relay binary. This is the executable counterpart of the relay repo's
 * `docs/push-integration-plan.md` §0 verification claims — it runs whenever a
 * relay binary is present and skips silently otherwise (CI-safe):
 *
 *   SHANNON_RELAY_BIN=/path/to/shannon-relay pnpm vitest run \
 *     src/mobile/relay/__tests__/pushRelaySmoke.test.ts
 *
 * Contract reference: docs/protocol/relay-push-wake-frames.md (v1.1).
 */
import { spawn, type ChildProcess } from "node:child_process";
import { createServer } from "node:net";
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { createConsoleLogger } from "../../../logger.js";
import type { MethodHandlers } from "../../server.js";
import { startRelayHost, type RelayHostHandle } from "../relayHost.js";
import { PushRelayBinding, type PushRelayError } from "../pushRelayBinding.js";

const logger = createConsoleLogger("error");

/** Locate the relay binary: env override, then the sibling workspace checkout. */
function findRelayBinary(): string | null {
  const candidates = [
    process.env.SHANNON_RELAY_BIN,
    // Sibling checkout in the shannon workspace (resolved from gateway/ cwd).
    // NOTE: keep the sibling build fresh (cargo build / cargo build --release)
    // — a stale binary silently degrades the smoke (e.g. no caps in
    // host_ready from a pre-v1.1 build).
    "../../shannon-service/shannon-relay/target/release/shannon-relay",
    "../../shannon-service/shannon-relay/target/debug/shannon-relay",
  ].filter((p): p is string => typeof p === "string" && p.length > 0);
  for (const p of candidates) {
    if (existsSync(p)) return p;
  }
  return null;
}

const relayBin = findRelayBinary();

describe.skipIf(!relayBin)("push cross-repo smoke (real shannon-relay binary)", () => {
  const dir = mkdtempSync(join(tmpdir(), "push-smoke-"));
  // Assigned in beforeAll: ask the OS for a genuinely free port — a fixed or
  // random range collides on dev boxes (other suites' listeners, services).
  let port = 0;
  let relayUrl = "";
  const sid = `smoke-${Date.now()}`;

  let relay: ChildProcess;
  let relayErrTail = "";
  let host: RelayHostHandle;
  let binding: PushRelayBinding;
  /** push.* frames seen on the wire (wake.ack capture). */
  const seenPushFrames: Array<Record<string, unknown>> = [];

  beforeAll(async () => {
    port = await new Promise<number>((res) => {
      const probe0 = createServer();
      probe0.listen(0, "127.0.0.1", () => {
        const p = (probe0.address() as { port: number }).port;
        probe0.close(() => res(p));
      });
    });

    // Fake FCM service account — ServiceAccount::parse only checks the three
    // fields; the 10s merge-window flush would fail at Google's endpoint, but
    // no test lives long enough to reach it (and the relay is killed after).
    writeFileSync(
      join(dir, "sa.json"),
      JSON.stringify({
        project_id: "push-smoke-project",
        client_email: "smoke@push-smoke-project.iam.gserviceaccount.com",
        private_key: "-----BEGIN PRIVATE KEY-----\nsmoke\n-----END PRIVATE KEY-----\n",
      }),
    );
    relay = spawn(relayBin!, {
      env: {
        ...process.env,
        LISTEN: `127.0.0.1:${port}`,
        HEALTH_LISTEN: "127.0.0.1:0",
        SHANNON_RELAY_BINDINGS_FILE: join(dir, "bindings.json"),
        SHANNON_FCM_SERVICE_ACCOUNT: join(dir, "sa.json"),
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    const tap = (d: Buffer) => {
      relayErrTail = (relayErrTail + String(d)).slice(-2000);
    };
    relay.stderr?.on("data", tap);
    relay.stdout?.on("data", tap);
    relay.on("error", (e) => {
      relayErrTail += `\n[relay spawn error: ${e.message}]`;
    });
    relay.once("exit", (code, signal) => {
      relayErrTail += `\n[relay exited code=${code} signal=${signal}]`;
    });
    relayUrl = `ws://127.0.0.1:${port}`;

    // Wait for the WS endpoint to accept.
    const { WebSocket } = await import("ws");
    const deadline = Date.now() + 10_000;
    for (;;) {
      try {
        await new Promise<void>((res, rej) => {
          const probe = new WebSocket(relayUrl);
          probe.once("open", () => {
            probe.close();
            res();
          });
          probe.once("error", rej);
        });
        break;
      } catch {
        if (Date.now() > deadline) {
          throw new Error(
            `relay did not come up on ${relayUrl} (bin=${relayBin}, alive=${!relay.killed}, ` +
              `exit=${relay.exitCode}/${relay.signalCode})\nrelay stderr tail:\n${relayErrTail}`,
          );
        }
        await new Promise((r) => setTimeout(r, 200));
      }
    }

    // The REAL desktop stack: startRelayHost + PushRelayBinding, wired exactly
    // like bootstrap.ts does (transport via the handle, capable from the
    // host_ready caps parse).
    host = startRelayHost({
      relayUrl,
      sid,
      sessionKey: Buffer.alloc(32, 7),
      handlers: {} as MethodHandlers,
      logger,
      pairTimeout: 60_000,
      relayAuthTag: "c21va2UtYXV0aC10YWctMTIzNDU2",
      onRegistered: () => {
        /* asserted in test 1 */
      },
    });
    host.paired.catch(() => {}); // no phone joins — the pair timeout is expected
    binding = new PushRelayBinding(
      {
        send: (frame) => host.sendControl(frame),
        onFrame: (handler) =>
          host.onControl((frame) => {
            seenPushFrames.push(frame);
            handler(frame);
          }),
      },
      { logger, capable: () => host.pushCapable() },
    );
  }, 30_000);

  afterAll(async () => {
    if (relayErrTail) console.log("[relay stderr tail]\n" + relayErrTail);
    await host.stop().catch(() => {});
    relay?.kill("SIGKILL");
  });

  it("advertises caps:[\"push\"] in host_ready and the desktop consumes it", async () => {
    // onRegistered → reconcile hook fires on host_ready; poll pushCapable.
    const deadline = Date.now() + 5_000;
    while (!host.pushCapable() && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 50));
    }
    expect(host.pushCapable()).toBe(true);
  });

  it("bind → 22ch handle; rotation keeps it; unbind → re-bind mints a new one", async () => {
    const first = await binding.bind("smoke-dev-a", { platform: "fcm", token: "tok-a1" });
    expect(first.handle).toMatch(/^[\w-]{22}$/);

    const rotated = await binding.bind("smoke-dev-a", { platform: "fcm", token: "tok-a2" });
    expect(rotated.handle).toBe(first.handle);

    await binding.unbind("smoke-dev-a");
    const fresh = await binding.bind("smoke-dev-a", { platform: "fcm", token: "tok-a3" });
    expect(fresh.handle).not.toBe(first.handle);

    // At-rest discipline: the store holds ciphertext, never the plaintext token.
    const raw = readFileSync(join(dir, "bindings.json"), "utf8");
    expect(raw).not.toContain("tok-a3");
  });

  it("wake acks echo their own seq; unbound device answers accepted:false", async () => {
    binding.wake("smoke-dev-a", 3);
    binding.wake("smoke-dev-a", 9);
    const deadline = Date.now() + 5_000;
    while (seenPushFrames.filter((f) => f["t"] === "push.wake.ack").length < 2 && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 50));
    }
    const acks = seenPushFrames.filter((f) => f["t"] === "push.wake.ack");
    expect(acks).toHaveLength(2);
    expect(acks[0]).toMatchObject({ accepted: true, seq: 3 });
    expect(acks[1]).toMatchObject({ accepted: true, seq: 9 });

    binding.wake("smoke-never-bound", 1);
    while (!seenPushFrames.some((f) => f["t"] === "push.wake.ack" && f["accepted"] === false) && Date.now() < deadline) {
      await new Promise((r) => setTimeout(r, 50));
    }
    expect(seenPushFrames.some((f) => f["t"] === "push.wake.ack" && f["accepted"] === false)).toBe(true);
  });

  it("bind rate limit: the 11th bind within 60s is rate_limited", async () => {
    const rateLimited: PushRelayError[] = [];
    for (let i = 0; i < 11 && rateLimited.length === 0; i += 1) {
      await binding.bind(`smoke-rate-${i}`, { platform: "fcm", token: `tok-r${i}` }).catch(
        (err: PushRelayError) => {
          if (err.relayCode === "rate_limited") rateLimited.push(err);
        },
      );
    }
    expect(rateLimited[0]?.relayCode).toBe("rate_limited");
  });
});

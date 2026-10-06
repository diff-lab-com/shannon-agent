/**
 * 修正1 (frame contract review 2026-10-05 §2.1) acceptance — the push
 * EXPECTED STATE store, the link-reconnect reconciliation, and the bootstrap
 * wiring factory. Contract source: docs/protocol/relay-push-wake-frames.md
 * §3.1/§3.2 (trigger table + expected-state reconciliation, 末态制胜, no
 * action queue).
 */
import { mkdtempSync, rmSync, statSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import { createConsoleLogger } from "../../../logger.js";
import { PushRelayBinding, type RelayPushTransport } from "../pushRelayBinding.js";
import {
  PushExpectedStateStore,
  createPushWiring,
  reconcileExpectedPushState,
} from "../pushExpectedState.js";

const logger = createConsoleLogger("error");

const dirs: string[] = [];
afterEach(() => {
  while (dirs.length) rmSync(dirs.pop()!, { recursive: true, force: true });
});

function tmpPaths(): { statePath: string; keyPath: string } {
  const dir = mkdtempSync(join(tmpdir(), "push-expected-"));
  dirs.push(dir);
  return { statePath: join(dir, "expected.json"), keyPath: join(dir, "key") };
}

/** File-mode store opts from the tmp paths (the store's option is `filePath`). */
function fileStoreOpts(paths: { statePath: string; keyPath: string }): {
  filePath: string;
  keyPath: string;
} {
  return { filePath: paths.statePath, keyPath: paths.keyPath };
}

/**
 * Fake transport with scripted acks: push.bind/push.unbind frames are acked
 * ok:true on a microtask (so a sequential reconcile drains without waiting
 * the 5s timeout). `hold` lets a test park a specific device's bind mid-round
 * (the 末态制胜 scenario).
 */
function autoAckTransport(hold?: (frame: Record<string, unknown>) => boolean) {
  const sent: Record<string, unknown>[] = [];
  const held: Array<{ frame: Record<string, unknown>; ack: () => void }> = [];
  const handlers: Array<(frame: Record<string, unknown>) => void> = [];
  const receive = (frame: Record<string, unknown>): void => {
    for (const h of handlers) h(frame);
  };
  const transport = {
    send: (frame: Record<string, unknown>): boolean => {
      sent.push(frame);
      if (hold?.(frame)) {
        held.push({ frame, ack: () => receive(ackFor(frame)) });
        return true;
      }
      queueMicrotask(() => receive(ackFor(frame)));
      return true;
    },
    onFrame: (handler: (frame: Record<string, unknown>) => void): (() => void) => {
      handlers.push(handler);
      return () => {
        const i = handlers.indexOf(handler);
        if (i >= 0) handlers.splice(i, 1);
      };
    },
    frames: sent,
    held,
  } satisfies RelayPushTransport & { frames: typeof sent; held: typeof held };
  return transport;
}

function ackFor(frame: Record<string, unknown>): Record<string, unknown> {
  if (frame["t"] === "push.bind") {
    return { t: "push.bind.ack", id: frame["id"], ok: true, handle: "h-relay" };
  }
  return { t: "push.unbind.ack", id: frame["id"], ok: true };
}

// ── the store ───────────────────────────────────────────────────────────────

describe("PushExpectedStateStore", () => {
  it("records intents and round-trips them through the encrypted file", () => {
    const paths = tmpPaths();
    const store = new PushExpectedStateStore({ logger, ...fileStoreOpts(paths) });
    store.recordEnabled("devA", "fcm", "tok-a");
    store.recordEnabled("devB", "apns", "tok-b");
    store.recordDisabled("devB");

    // Fresh instance over the same files = the reconcile source of truth
    // survives a gateway restart.
    const reloaded = new PushExpectedStateStore({ logger, ...fileStoreOpts(paths) });
    expect(reloaded.get("devA")).toEqual({ enabled: true, platform: "fcm", token: "tok-a" });
    expect(reloaded.get("devB")?.enabled).toBe(false);
    // token stays as the last registered value (the reconcile re-bind needs it).
    expect(reloaded.get("devB")?.token).toBe("tok-b");
    expect(reloaded.list()).toHaveLength(2);
  });

  it("the at-rest file never contains the token plaintext and lands 0600", () => {
    const paths = tmpPaths();
    const store = new PushExpectedStateStore({ logger, ...fileStoreOpts(paths) });
    store.recordEnabled("devA", "fcm", "tok-PLAINTEXT-SECRET");

    const raw = readFileSync(paths.statePath, "utf8");
    expect(raw).not.toContain("tok-PLAINTEXT-SECRET");
    expect(raw).not.toContain("devA");
    expect(statSync(paths.statePath).mode & 0o777).toBe(0o600);
    expect(statSync(paths.keyPath).mode & 0o777).toBe(0o600);
  });

  it("a lost key degrades to a fresh store (评审修正1的自愈路径)", () => {
    const paths = tmpPaths();
    const store = new PushExpectedStateStore({ logger, ...fileStoreOpts(paths) });
    store.recordEnabled("devA", "fcm", "tok-a");
    rmSync(paths.keyPath);

    const reloaded = new PushExpectedStateStore({ logger, ...fileStoreOpts(paths) });
    expect(reloaded.list()).toEqual([]);
    // And the store still works — the first new intent re-keys transparently.
    reloaded.recordEnabled("devB", "apns", "tok-b");
    expect(reloaded.get("devB")).toMatchObject({ enabled: true });
  });

  it("forget removes the entry entirely (§M2 revoke cascade)", () => {
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    store.forget("devA");
    expect(store.get("devA")).toBeUndefined();
    // Forgetting an unknown id is an honest no-op (no spurious persist).
    expect(() => store.forget("dev-unknown")).not.toThrow();
  });
});

// ── reconciliation (评审 3.2) ────────────────────────────────────────────────

describe("reconcileExpectedPushState (修正1 §3.2)", () => {
  it("expected on → push.bind with the recorded platform/token; expected off → push.unbind", async () => {
    const t = autoAckTransport();
    const binding = new PushRelayBinding(t, { logger });
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devOn", "fcm", "tok-on");
    store.recordDisabled("devOff");

    await reconcileExpectedPushState(store, binding, logger);

    const bind = t.frames.find((f) => f["t"] === "push.bind");
    expect(bind).toMatchObject({ deviceId: "devOn", platform: "fcm", token: "tok-on" });
    const unbind = t.frames.find((f) => f["t"] === "push.unbind");
    expect(unbind).toMatchObject({ deviceId: "devOff" });
  });

  it("an enabled entry without platform/token asserts unbind (corrupt-intent fallback)", async () => {
    const t = autoAckTransport();
    const binding = new PushRelayBinding(t, { logger });
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    store.recordDisabled("devA");

    await reconcileExpectedPushState(store, binding, logger);

    expect(t.frames.map((f) => f["t"])).toEqual(["push.unbind"]);
    expect(t.frames[0]).toMatchObject({ deviceId: "devA" });
  });

  it("one device's refusal never starves the rest (per-device best-effort)", async () => {
    const t = autoAckTransport();
    // Script devA's bind ack as ok:false (relay refuses).
    const handlers: Array<(f: Record<string, unknown>) => void> = [];
    const raw = new PushRelayBinding(
      {
        send: (frame) => {
          t.frames.push(frame);
          queueMicrotask(() => {
            for (const h of handlers) {
              h(
                frame["deviceId"] === "devA"
                  ? { t: "push.bind.ack", id: frame["id"], ok: false, error: { code: "bad_request", message: "no" } }
                  : ackFor(frame),
              );
            }
          });
          return true;
        },
        onFrame: (h) => {
          handlers.push(h);
          return () => {
            const i = handlers.indexOf(h);
            if (i >= 0) handlers.splice(i, 1);
          };
        },
      },
      { logger },
    );
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    store.recordEnabled("devB", "apns", "tok-b");

    await reconcileExpectedPushState(store, raw, logger);

    expect(t.frames.filter((f) => f["t"] === "push.bind").map((f) => f["deviceId"])).toEqual([
      "devA",
      "devB",
    ]);
  });

  it("末态制胜: an intent flipped mid-reconcile asserts the FLIPPED state", async () => {
    // Hold devA's bind mid-round; while it is parked, the user disables devB.
    const t = autoAckTransport((frame) => frame["deviceId"] === "devA");
    const binding = new PushRelayBinding(t, { logger });
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    store.recordEnabled("devB", "fcm", "tok-b");

    const done = reconcileExpectedPushState(store, binding, logger);
    await vi_waitFor(() => t.frames.some((f) => f["t"] === "push.bind" && f["deviceId"] === "devA"));
    store.recordDisabled("devB"); // user toggle lands while devA is in flight
    t.held[0]!.ack(); // release devA
    await done;

    expect(t.frames.filter((f) => f["t"] === "push.unbind").map((f) => f["deviceId"])).toEqual([
      "devB",
    ]);
  });
});

/** Tiny condition poll (vi.waitFor is available; keep the deps explicit). */
async function vi_waitFor(cond: () => boolean, timeoutMs = 1000): Promise<void> {
  const start = Date.now();
  while (!cond()) {
    if (Date.now() - start > timeoutMs) throw new Error("waitFor timeout");
    await new Promise((r) => setTimeout(r, 5));
  }
}

// ── wiring factory (bootstrap assembly seam) ────────────────────────────────

describe("createPushWiring (修正1 bootstrap seam)", () => {
  it("pushBindingSink records the intent FIRST, then binds through the ref cell", async () => {
    const t = autoAckTransport();
    const ref = { current: new PushRelayBinding(t, { logger }) };
    const store = new PushExpectedStateStore({ logger });
    const wiring = createPushWiring({ pushRelayRef: ref, store, logger });

    const { handle } = await wiring.pushBindingSink("devA", { platform: "fcm", token: "tok-a" });
    expect(handle).toBe("h-relay");
    expect(store.get("devA")).toEqual({ enabled: true, platform: "fcm", token: "tok-a" });
    expect(t.frames[0]).toMatchObject({ t: "push.bind", deviceId: "devA" });
  });

  it("pushBindingSink with no relay: intent recorded, §O2 NOT_IMPLEMENTED still thrown", async () => {
    const ref = { current: null };
    const store = new PushExpectedStateStore({ logger });
    const wiring = createPushWiring({ pushRelayRef: ref, store, logger });

    await expect(
      wiring.pushBindingSink("devA", { platform: "fcm", token: "tok-a" }),
    ).rejects.toMatchObject({ code: -32603 });
    // The intent survived the refusal — the link-reconnect reconcile will
    // bind it (the 自愈 half of 修正1).
    expect(store.get("devA")).toEqual({ enabled: true, platform: "fcm", token: "tok-a" });
  });

  it("pushUnbindSink records the disabled intent; unbinds when the relay is up", async () => {
    const t = autoAckTransport();
    const ref = { current: new PushRelayBinding(t, { logger }) };
    const store = new PushExpectedStateStore({ logger });
    const wiring = createPushWiring({ pushRelayRef: ref, store, logger });

    await wiring.pushUnbindSink("devA");
    expect(store.get("devA")?.enabled).toBe(false);
    expect(t.frames[0]).toMatchObject({ t: "push.unbind", deviceId: "devA" });

    // No relay cell → resolves anyway (the honest-ok posture; reconcile backs it).
    const down = createPushWiring({ pushRelayRef: { current: null }, store, logger });
    await expect(down.pushUnbindSink("devA")).resolves.toBeUndefined();
  });

  it("onDeviceRevoked forgets the intent and fires a best-effort unbind", async () => {
    const t = autoAckTransport();
    const ref = { current: new PushRelayBinding(t, { logger }) };
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    const wiring = createPushWiring({ pushRelayRef: ref, store, logger });

    wiring.onDeviceRevoked("devA");
    expect(store.get("devA")).toBeUndefined();
    await vi_waitFor(() => t.frames.some((f) => f["t"] === "push.unbind" && f["deviceId"] === "devA"));

    // A rejecting unbind must not escape as an unhandled rejection.
    const badWiring = createPushWiring({
      pushRelayRef: {
        current: new PushRelayBinding(
          {
            send: () => false, // link down → request rejects "unreachable"
            onFrame: () => () => {},
          },
          { logger },
        ),
      },
      store: new PushExpectedStateStore({ logger }),
      logger,
    });
    badWiring.onDeviceRevoked("devX");
    await new Promise((r) => setTimeout(r, 10)); // would crash the test on unhandled rejection
  });

  it("reconcile no-ops without a relay and drives the store when one exists", async () => {
    const store = new PushExpectedStateStore({ logger });
    store.recordEnabled("devA", "fcm", "tok-a");
    const idle = createPushWiring({ pushRelayRef: { current: null }, store, logger });
    expect(() => idle.reconcile()).not.toThrow();

    const t = autoAckTransport();
    const live = createPushWiring({
      pushRelayRef: { current: new PushRelayBinding(t, { logger }) },
      store,
      logger,
    });
    live.reconcile();
    await vi_waitFor(() => t.frames.some((f) => f["t"] === "push.bind" && f["deviceId"] === "devA"));
  });
});

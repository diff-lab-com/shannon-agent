/**
 * §O3/§T6 Push-to-Wake binding acceptance — the executable spec of
 * docs/protocol/relay-push-wake-frames.md, run against a fake transport.
 */
import { describe, expect, it, vi } from "vitest";

import { createConsoleLogger } from "../../../logger.js";
import { PushRelayBinding, PushRelayError, type RelayPushTransport } from "../pushRelayBinding.js";

const logger = createConsoleLogger("error");

/** Fake transport: records outbound frames, lets tests script inbound acks. */
function fakeTransport() {
  const sent: Record<string, unknown>[] = [];
  const handlers: Array<(frame: Record<string, unknown>) => void> = [];
  let linkUp = true;
  const transport: RelayPushTransport & { frames: typeof sent; setUp: (up: boolean) => void; receive: (f: Record<string, unknown>) => void } = {
    send: (frame) => {
      if (!linkUp) return false;
      sent.push(frame);
      return true;
    },
    onFrame: (handler) => {
      handlers.push(handler);
      return () => {
        const i = handlers.indexOf(handler);
        if (i >= 0) handlers.splice(i, 1);
      };
    },
    frames: sent,
    setUp: (up) => {
      linkUp = up;
    },
    receive: (frame) => {
      for (const h of handlers) h(frame);
    },
  };
  return transport;
}

describe("PushRelayBinding (§O3/§T6 frames)", () => {
  it("bind round-trips the §T6 frame and resolves with the relay handle", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    const promise = binding.bind("devA", { platform: "fcm", token: "tok-1" });

    // Outbound frame per the contract: t + deviceId + platform + token + id + v.
    const frame = t.frames[0]! as Record<string, unknown>;
    expect(frame["t"]).toBe("push.bind");
    expect(frame["deviceId"]).toBe("devA");
    expect(frame["platform"]).toBe("fcm");
    expect(frame["token"]).toBe("tok-1");
    expect(typeof frame["id"]).toBe("string");
    expect(frame["v"]).toBe(1);

    t.receive({ t: "push.bind.ack", id: frame["id"], ok: true, handle: "h-relay" });
    await expect(promise).resolves.toEqual({ handle: "h-relay" });
  });

  it("bind ack ok:false maps relay not_configured to the phone's NOT_IMPLEMENTED", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    const promise = binding.bind("devA", { platform: "apns", token: "tok" });
    const frame = t.frames[0]!;
    t.receive({ t: "push.bind.ack", id: frame["id"], ok: false, error: { code: "not_configured", message: "no APNs key" } });
    const err = await promise.catch((e) => e);
    expect(err).toBeInstanceOf(PushRelayError);
    expect((err as PushRelayError).relayCode).toBe("not_configured");
    expect((err as PushRelayError).code).toBe(-32603);
  });

  it("other relay refusals map to the generic structured code", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    const promise = binding.bind("devA", { platform: "fcm", token: "tok" });
    const frame = t.frames[0]!;
    t.receive({ t: "push.bind.ack", id: frame["id"], ok: false, error: { code: "vendor_rejected", message: "stale token" } });
    const err = await promise.catch((e) => e);
    expect((err as PushRelayError).code).toBe(-32002);
    expect((err as PushRelayError).message).toBe("stale token");
  });

  it("bind times out into a structured error when the relay never acks", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger, timeoutMs: 20 });
    await expect(binding.bind("devA", { platform: "fcm", token: "tok" })).rejects.toMatchObject({
      relayCode: "timeout",
    });
  });

  it("bind with a down link rejects immediately instead of hanging", async () => {
    const t = fakeTransport();
    t.setUp(false);
    const binding = new PushRelayBinding(t, { logger });
    await expect(binding.bind("devA", { platform: "fcm", token: "tok" })).rejects.toMatchObject({
      relayCode: "unreachable",
    });
  });

  it("unbind round-trips; unknown-id ok stays a success (honest no-op)", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    const promise = binding.unbind("devA");
    t.receive({ t: "push.unbind.ack", id: t.frames[0]!["id"], ok: true });
    await expect(promise).resolves.toBeUndefined();
    expect(t.frames[0]!["t"]).toBe("push.unbind");
  });

  it("wake is fire-and-forget: frame out, no ack wait, dropped quietly when down", () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    binding.wake("devA", 7);
    expect(t.frames[0]).toMatchObject({ t: "push.wake", deviceId: "devA", seq: 7 });

    const warn = vi.fn();
    const downBinding = new PushRelayBinding(t, { logger: { warn } });
    t.setUp(false);
    expect(() => downBinding.wake("devA", 8)).not.toThrow();
    expect(warn).toHaveBeenCalledOnce();
  });

  it("dispose rejects in-flight requests and stops listening", async () => {
    const t = fakeTransport();
    const binding = new PushRelayBinding(t, { logger });
    const promise = binding.bind("devA", { platform: "fcm", token: "tok" });
    binding.dispose();
    await expect(promise).rejects.toMatchObject({ relayCode: "disposed" });
    const before = t.frames.length;
    t.receive({ t: "push.bind.ack", id: "late", ok: true, handle: "h" });
    expect(t.frames.length).toBe(before);
  });
});

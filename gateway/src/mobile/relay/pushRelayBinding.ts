/**
 * §O3 desktop→relay Push-to-Wake binding — the executable spec of the frames
 * pinned in `docs/protocol/relay-push-wake-frames.md` (push.bind / push.unbind
 * / push.wake + acks over the desktop's relay control-plane WSS).
 *
 * Transport is injected (`RelayPushTransport`) so the frames can ride any
 * carrier: production wires it to the relay host connection's control-frame
 * side channel (`relayHost.ts` `sendControl`/`onControl`); tests use a fake.
 * Request/response correlation, the ack timeout, and the relay-error →
 * phone-visible code mapping live HERE; `relayHost.ts` stays a dumb pipe.
 *
 * Error mapping (docs §5): `not_configured` maps to NOT_IMPLEMENTED — the
 * SAME phone-visible code as an unwired sink — so the phone's tri-state
 * "推送不可用" has exactly one code regardless of which hop lacks the vendor
 * credentials; every other relay refusal maps to ENGINE_ERROR (§O2's
 * structured-error contract, never a mocked success).
 */

import { ShannonError } from "../protocol.js";

/** The T6 contract's transport face (see relayHost.ts `sendControl`/`onControl`). */
export interface RelayPushTransport {
  /** Best-effort send; false = link down (frame dropped). */
  send(frame: Record<string, unknown>): boolean;
  /** Subscribe to relay→desktop control frames. Returns the unsubscribe fn. */
  onFrame(handler: (frame: Record<string, unknown>) => void): () => void;
}

/** One vendor push binding the phone registered (§O2 shape, forwarded). */
export interface PushBindingRequest {
  platform: "fcm" | "apns";
  token: string;
}

/**
 * A relay-side refusal. `relayCode` is the contract's string code (§5);
 * `code` is the phone-visible ShannonError the push.register handler surfaces.
 */
export class PushRelayError extends Error {
  constructor(
    readonly relayCode: string,
    readonly code: number,
    message: string,
  ) {
    super(message);
  }
}

const PHONE_CODE_BY_RELAY_CODE: Record<string, number> = {
  not_configured: ShannonError.NOT_IMPLEMENTED,
};

export interface PushRelayBindingOptions {
  logger: { warn(message: string): void };
  /** Ack wait per bind/unbind round-trip (default 5000ms). */
  timeoutMs?: number;
}

interface PendingRequest {
  resolve: (ack: Record<string, unknown>) => void;
  reject: (err: PushRelayError) => void;
  timer: ReturnType<typeof setTimeout>;
}

export class PushRelayBinding {
  private readonly pending = new Map<string, PendingRequest>();
  private readonly unsubscribe: () => void;

  constructor(
    private readonly transport: RelayPushTransport,
    private readonly opts: PushRelayBindingOptions,
  ) {
    this.unsubscribe = transport.onFrame((frame) => this.onFrame(frame));
  }

  /** Stop listening (relay host shutdown). In-flight requests reject. */
  dispose(): void {
    this.unsubscribe();
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(new PushRelayError("disposed", ShannonError.ENGINE_ERROR, "push relay binding disposed"));
      this.pending.delete(id);
    }
  }

  /**
   * `push.bind` — register (or rotate) the device's vendor token; resolves
   * with the relay-assigned random handle. Refusals throw PushRelayError.
   */
  async bind(deviceId: string, binding: PushBindingRequest): Promise<{ handle: string }> {
    const ack = await this.request({
      t: "push.bind",
      deviceId,
      platform: binding.platform,
      token: binding.token,
    });
    if (ack["ok"] !== true) throw this.asError(ack, "push.bind");
    const handle = ack["handle"];
    if (typeof handle !== "string" || handle.length === 0) {
      throw new PushRelayError("bad_ack", ShannonError.ENGINE_ERROR, "push.bind.ack missing handle");
    }
    return { handle };
  }

  /** `push.unbind` — unregister (honest no-op on the relay for unknown ids). */
  async unbind(deviceId: string): Promise<void> {
    const ack = await this.request({ t: "push.unbind", deviceId });
    if (ack["ok"] !== true) throw this.asError(ack, "push.unbind");
  }

  /**
   * `push.wake` — fire-and-forget per §O3 (the relay merges per handle with
   * the 10s window; no ack wait). Dropped silently when the link is down —
   * a desktop whose relay link is down cannot be woken through it anyway.
   */
  wake(deviceId: string, seq: number): void {
    const sent = this.transport.send({ t: "push.wake", deviceId, seq });
    if (!sent) this.opts.logger.warn(`push wake dropped (relay link down): ${deviceId} seq=${seq}`);
  }

  private request(frame: Record<string, unknown>): Promise<Record<string, unknown>> {
    const id = crypto.randomUUID();
    const timeoutMs = this.opts.timeoutMs ?? 5_000;
    return new Promise<Record<string, unknown>>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new PushRelayError("timeout", ShannonError.ENGINE_ERROR, `push relay ack timeout (${timeoutMs}ms)`));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      const sent = this.transport.send({ ...frame, id, v: 1 });
      if (!sent) {
        clearTimeout(timer);
        this.pending.delete(id);
        reject(new PushRelayError("unreachable", ShannonError.ENGINE_ERROR, "push relay link is down"));
      }
    });
  }

  private onFrame(frame: Record<string, unknown>): void {
    const type = frame["t"];
    if (type !== "push.bind.ack" && type !== "push.unbind.ack") return;
    const id = frame["id"];
    if (typeof id !== "string") return;
    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id);
    clearTimeout(pending.timer);
    pending.resolve(frame);
  }

  private asError(ack: Record<string, unknown>, op: string): PushRelayError {
    const err = ack["error"] as { code?: unknown; message?: unknown } | undefined;
    const relayCode = typeof err?.code === "string" ? err.code : "unknown";
    const message = typeof err?.message === "string" ? err.message : `${op} refused by relay`;
    const phoneCode = PHONE_CODE_BY_RELAY_CODE[relayCode] ?? ShannonError.ENGINE_ERROR;
    return new PushRelayError(relayCode, phoneCode, message);
  }
}

/**
 * v0.13 negotiated direct-link E2E seal (cross-repo-adaptation-spec §I) — the
 * LAN/direct counterpart of the relay E2E channel. The phone's FIRST frame on
 * a direct WebSocket is a plaintext (binary) `e2e_direct_hello` carrying a
 * fresh ephemeral X25519 pubkey (plus the base-key id on resume connections);
 * a seal-capable host answers with a sealed `e2e_direct_ready` ack and every
 * later byte both ways is the relay's C6 AEAD frame format. Anything else
 * keeps the exact legacy behavior (text = NDJSON, binary = UTF-8 tolerance) —
 * an old phone that never sends the hello is unaffected, and a hello this
 * host cannot honor is dropped silently so the phone times out and downgrades
 * (its fallback contract, §I5).
 *
 * Key schedule (byte-pinned against shannon-mobile `test/direct_e2e_golden_test.dart`):
 *   pairing flavor (token still live): K0 = HKDF-SHA256(ikm = UTF8(pairToken) ||
 *       X25519(phoneEph, hostStatic), salt = "shannon-relay", info = "shannon-e2e-v2")
 *       — the SAME derivation as the relay v2 schedule, no new surface.
 *   resume flavor (token consumed):    K = HKDF-SHA256(ikm = K0 ||
 *       X25519(phoneEph', hostStatic), salt = "shannon-relay",
 *       info = "shannon-e2e-direct-v1") — dedicated info domain-separates the
 *       resume schedule so a resume IKM never equals a pairing/relay IKM.
 * Counters start at zero per connection in BOTH directions: every connection
 * contributes a fresh phone ephemeral, so no key (and no counter) ever spans
 * connections — relay-track counter persistence does not apply here.
 */

import { createHash, generateKeyPairSync } from "node:crypto";
import { createPrivateKey, createPublicKey, type KeyObject } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import { EventEmitter } from "node:events";
import { WebSocket } from "ws";

import type { Logger } from "../adapters/types.js";
import {
  E2eChannel,
  deriveSessionKeyV2,
  hostSharedSecret,
  hkdfExpand,
  hkdfExtract,
} from "./relay/e2e.js";

/** `~/.shannon/mobile-direct-e2e/{x25519.json,direct-e2e-info.json}`. */
export function directE2EPaths(baseDir?: string): { keyPath: string; infoPath: string } {
  const dir = baseDir ?? join(homedir(), ".shannon", "mobile-direct-e2e");
  return { keyPath: join(dir, "x25519.json"), infoPath: join(dir, "direct-e2e-info.json") };
}

export interface DirectE2EIdentity {
  privateKey: KeyObject;
  /** base64url raw 32-byte X25519 public key — the QR `hostE2EPubKey` value. */
  pubB64: string;
}

interface PersistedDirectKey {
  kty: "OKP";
  crv: "X25519";
  x: string;
  d: string;
  /** Index signature so the object satisfies node's `JsonWebKey` input type. */
  [key: string]: unknown;
}

/**
 * Load the persisted static X25519 E2E identity, generating it on first use
 * (mobileTls.ts pattern: trust travels out-of-band in the QR, so the private
 * key lives only here, 0600; rotation = re-pairing). `direct-e2e-info.json`
 * carries the public key for the desktop's direct-QR composer — the same
 * Design-D file channel the TLS fingerprint uses.
 */
export function ensureDirectE2EKey(paths = directE2EPaths()): DirectE2EIdentity {
  if (existsSync(paths.keyPath)) {
    const jwk = JSON.parse(readFileSync(paths.keyPath, "utf8")) as PersistedDirectKey;
    const privateKey = createPrivateKey({ key: jwk, format: "jwk" });
    // Derive the pub from the key material rather than trusting the stored
    // `x` (a hand-edited/corrupt file must not advertise a foreign identity).
    const pubB64 = (createPublicKey(privateKey).export({ format: "jwk" }) as { x: string }).x;
    return { privateKey, pubB64 };
  }

  const { privateKey, pubB64 } = generateDirectE2EKeyPair();
  const jwk = privateKey.export({ format: "jwk" }) as PersistedDirectKey;
  mkdirSync(dirname(paths.keyPath), { recursive: true });
  writeFileSync(paths.keyPath, JSON.stringify(jwk, null, 2), { encoding: "utf8", mode: 0o600 });
  writeFileSync(paths.infoPath, JSON.stringify({ hostE2EPubKey: pubB64 }, null, 2), {
    encoding: "utf8",
    mode: 0o600,
  });
  return { privateKey, pubB64 };
}

/** Generate a fresh X25519 identity (tests / first boot). */
export function generateDirectE2EKeyPair(): DirectE2EIdentity {
  const { publicKey, privateKey } = generateKeyPairSync("x25519");
  return { privateKey, pubB64: (publicKey.export({ format: "jwk" }) as { x: string }).x };
}

// ── handshake primitives (byte-pinned cross-repo) ───────────────────────────

/**
 * Direct-link connection key, resume flavor:
 * `HKDF-SHA256(ikm = baseKey || shared, salt = "shannon-relay",
 * info = "shannon-e2e-direct-v1")` → 32 bytes. `baseKey` is the pairing
 * connection's key (persisted on both peers after a sealed pair); every later
 * connection contributes a fresh phone ephemeral — one key per connection.
 */
export function deriveDirectSessionKey(baseKey: Buffer, shared: Buffer): Buffer {
  const ikm = Buffer.concat([baseKey, shared]);
  const salt = Buffer.from("shannon-relay", "utf8");
  const info = Buffer.from("shannon-e2e-direct-v1", "utf8");
  return hkdfExpand(hkdfExtract(ikm, salt), info, 32);
}

/**
 * Public, non-secret id of a base key: `base64url(SHA-256(baseKey)[0..8])`.
 * The resume hello carries it so the host can pick WHICH pairing's base key
 * to rekey from without exposing the deviceId on a plaintext LAN frame.
 */
export function directKeyId(baseKey: Buffer): string {
  return createHash("sha256").update(baseKey).digest().subarray(0, 8).toString("base64url");
}

/** Plaintext first frame a direct-mode phone sends (parsed form). */
export interface DirectHello {
  pub: string;
  /** Present only on resume connections (§I3-②). */
  kid?: string;
}

/** True when `frame` parses as an `e2e_direct_hello` first frame. */
export function isDirectHelloFrame(frame: Buffer): DirectHello | null {
  if (frame.length > 512) return null;
  try {
    const parsed = JSON.parse(frame.toString("utf8")) as {
      t?: unknown;
      pub?: unknown;
      kid?: unknown;
    };
    if (parsed.t !== "e2e_direct_hello" || typeof parsed.pub !== "string" || parsed.pub.length === 0) {
      return null;
    }
    return parsed.kid === undefined
      ? { pub: parsed.pub }
      : typeof parsed.kid === "string" && parsed.kid.length > 0
        ? { pub: parsed.pub, kid: parsed.kid }
        : null;
  } catch {
    return null;
  }
}

/** The sealed host ack payload — proves key possession by being encrypted. */
export function directReadyFrame(): Buffer {
  return Buffer.from(JSON.stringify({ t: "e2e_direct_ready", v: 1 }), "utf8");
}

// ── per-connection negotiating transport (server side) ──────────────────────

export interface DirectE2EOptions {
  /** Host static X25519 private key (persisted, `ensureDirectE2EKey`). */
  privateKey: KeyObject;
  /**
   * Freshest live (unconsumed) one-time pair token, for the pairing flavor.
   * The token is NOT consumed here — `shannon/pair` does that later on the
   * sealed channel. Return null (or omit) to disable the pairing flavor.
   */
  livePairToken?: () => string | null;
  /**
   * kid → K0 registry for the resume flavor, shared across the server's
   * connections. Entries are published when a pairing-flavor link's
   * `shannon/pair` succeeds (§I6.2) and survive process-lifetime (mobile
   * re-pairs after a gateway restart).
   */
  linkKeys?: Map<string, Buffer>;
}

/**
 * A virtual socket satisfying the `MethodContext.socket` surface (handlers
 * check `readyState`, call `send`/`close`; the hub listens for `close`) that
 * routes outbound text through the direct link — queued before the first
 * frame decides, plaintext in legacy mode, C6-sealed once negotiated.
 */
class DirectSocketProxy extends EventEmitter {
  static readonly OPEN = WebSocket.OPEN;

  constructor(
    private readonly real: WebSocket,
    private readonly sendThrough: (text: string) => void,
    onError: (err: Error) => void,
  ) {
    super();
    // The dispatch hub registers `close` (and handlers may register `error`)
    // on ctx.socket — forward the real socket's lifecycle so cleanup fires.
    this.real.on("close", (code, reason) => this.emit("close", code, reason));
    this.real.on("error", (err) => this.emit("error", err));
    // EventEmitter THROWS on `error` with zero listeners — and no handler in
    // the server/hub listens on ctx.socket by default. A malformed frame on
    // an unauthenticated connection would then crash the whole gateway, so
    // seed a warn-and-carry-on listener that additional listeners stack onto.
    this.on("error", onError);
  }

  get readyState(): number {
    return this.real.readyState;
  }

  send(data: string): void {
    this.sendThrough(data);
  }

  close(): void {
    this.real.close();
  }
}

/**
 * One direct connection's negotiation + sealed transport. Owns the decision
 * (first frame), the two E2eChannel directions, the pre-decision outbound
 * queue, and the §I6.2 kid publication on pair success. Installed per
 * connection by `MobileServer` when `directE2E` is configured.
 */
export class DirectLink {
  /** Passed to `MethodContext.socket` — routes handler/hub sends through us. */
  readonly socket: DirectSocketProxy;

  private readonly queuedOut: string[] = [];
  private decided = false;
  private sealed = false;
  /** Set when a hello was recognized but cannot be honored → swallow it and stay legacy. */
  private droppedHello = false;
  private sendChannel: E2eChannel | null = null;
  private recvChannel: E2eChannel | null = null;
  /** Pairing-flavor base key — published under its kid once this connection pairs. */
  private pairingKey: Buffer | null = null;

  constructor(
    private readonly opts: DirectE2EOptions,
    private readonly real: WebSocket,
    private readonly logger: Logger,
  ) {
    this.socket = new DirectSocketProxy(real, (text) => this.outbound(text), (err) =>
      this.logger.warn(`direct e2e socket error: ${(err as Error).message}`),
    );
  }

  /**
   * §I6.2: publish `kid(K0) → K0` when `shannon/pair` succeeds on THIS
   * connection. Called after `onContext` so the hub's binder (if any) stays
   * ahead of us in the chain. Armed for every pairing-flavor-possible link —
   * the hello may arrive after this call — but publishes only if that link
   * actually sealed the pairing flavor (K0 known) at bind time.
   */
  armPublishOnBind(ctx: { onSessionBound?: (deviceId: string) => void }): void {
    const prev = ctx.onSessionBound;
    ctx.onSessionBound = (deviceId: string) => {
      prev?.(deviceId);
      const key = this.pairingKey;
      if (!key) return; // resume flavor / never sealed → nothing to publish
      this.opts.linkKeys?.set(directKeyId(key), key);
      this.logger.info(`direct e2e: base key published (kid=${directKeyId(key)}) after pair of ${deviceId}`);
    };
  }

  /**
   * Take over the connection's inbound frames. `dispatchLine` receives each
   * cleartext NDJSON line (sealed mode: after `open`; legacy: as-is).
   */
  listen(dispatchLine: (text: string) => void): void {
    this.real.on("message", (data: unknown, isBinary: boolean) => {
      const frame = toBuffer(data);
      if (!this.decided) {
        this.decided = true;
        // The hello travels as a BINARY frame (spec §I2.1) — a text frame is
        // just the start of a legacy NDJSON session.
        if (isBinary && this.tryNegotiate(frame)) return;
        if (this.droppedHello) return; // recognized but unhonorable — swallow
      }
      if (this.sealed) {
        // Sealed mode: text frames are IGNORED (an authenticated session is
        // not torn down by unauthenticated datagrams, §I4).
        if (!isBinary) return;
        let plaintext: Buffer;
        try {
          plaintext = this.recvChannel!.open(frame);
        } catch (err) {
          // Same posture as the relay host: drop the frame, keep the session
          // (the recv counter did not advance, so the channel stays fail-closed).
          this.logger.warn(`direct e2e: open failed: ${(err as Error).message}`);
          return;
        }
        dispatchLine(plaintext.toString("utf8"));
        return;
      }
      // Legacy tolerance (pre-seal direct behavior): binary frames are UTF-8 NDJSON.
      dispatchLine(frame.toString("utf8"));
    });
  }

  /** First-frame negotiation. True when the link sealed. Never throws. */
  private tryNegotiate(frame: Buffer): boolean {
    const hello = isDirectHelloFrame(frame);
    if (!hello) return false;
    try {
      const shared = hostSharedSecret(this.opts.privateKey, hello.pub);
      let key: Buffer;
      if (hello.kid !== undefined) {
        const base = this.opts.linkKeys?.get(hello.kid);
        if (!base) {
          this.logger.info(`direct e2e: resume hello with unknown kid — ignoring (phone downgrades)`);
          this.droppedHello = true;
          return false;
        }
        key = deriveDirectSessionKey(base, shared);
        this.logger.info("direct e2e: resume handshake accepted (fresh per-connection key)");
      } else {
        const token = this.opts.livePairToken?.() ?? null;
        if (!token) {
          this.logger.info("direct e2e: pairing hello with no live pair token — ignoring");
          this.droppedHello = true;
          return false;
        }
        key = deriveSessionKeyV2(token, shared);
        this.logger.info("direct e2e: pairing handshake accepted (token-mixed, relay v2 schedule)");
      }
      this.sendChannel = new E2eChannel(key);
      this.recvChannel = new E2eChannel(key);
      // The ack IS the first sealed frame — it proves key possession.
      this.real.send(this.sendChannel.seal(directReadyFrame()), { binary: true });
      this.sealed = true;
      // Only a link that actually sealed may publish its base key (§I6.2) —
      // set AFTER the ack is out, so a failed send degrades to legacy without
      // leaving a phantom K0 behind.
      if (hello.kid === undefined) this.pairingKey = key;
      // Flush anything the hub fanned out while the handshake was in flight.
      for (const line of this.queuedOut) this.sealOut(line);
      this.queuedOut.length = 0;
      return true;
    } catch (err) {
      // Malformed pubkey or KDF failure → legacy mode, never break the session.
      this.logger.warn(`direct e2e: handshake failed (${(err as Error).message}) — legacy plaintext`);
      this.droppedHello = true;
      return false;
    }
  }

  private outbound(text: string): void {
    if (!this.decided) {
      this.queuedOut.push(text);
      return;
    }
    if (!this.sealed) {
      if (this.real.readyState === DirectSocketProxy.OPEN) this.real.send(text);
      return;
    }
    this.sealOut(text);
    // Note: lines queued before a LEGACY decision are deliberately never
    // flushed — matching _NegotiatingHostTransport, and safe because hub
    // pushes target session-bound contexts (which require an inbound frame,
    // i.e. a decided link). Do NOT "fix" this into a plaintext flush: it
    // would leak pre-seal traffic to a client that never sealed.
  }

  private sealOut(text: string): void {
    if (!this.sendChannel || this.real.readyState !== DirectSocketProxy.OPEN) return;
    this.real.send(this.sendChannel.seal(Buffer.from(text, "utf8")), { binary: true });
  }
}

function toBuffer(data: unknown): Buffer {
  if (Buffer.isBuffer(data)) return data;
  if (data instanceof ArrayBuffer) return Buffer.from(data);
  if (Array.isArray(data)) return Buffer.concat(data as Buffer[]);
  if (ArrayBuffer.isView(data)) return Buffer.from(data as Uint8Array);
  return Buffer.from(String(data), "utf8");
}

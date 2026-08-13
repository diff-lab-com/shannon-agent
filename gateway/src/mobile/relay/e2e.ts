/**
 * E2E encrypted channel for relay-mode communication between the gateway and
 * the phone. This is a direct TypeScript port of the Dart `E2eChannel` in
 * shannon-mobile, and MUST produce byte-for-byte identical frames.
 *
 * Wire format: `[1B ver=0x01][8B counter big-endian][AES-256-GCM ciphertext + 16B tag]`
 *
 * - Version byte: `0x01`
 * - Counter: 64-bit big-endian, starts at 1 (0 = no messages yet), monotonic per sender
 * - AES-256-GCM nonce: 12 bytes = `[0x00 × 4] ++ counter_be(8)`
 * - Ciphertext + tag: Node `cipher.getAuthTag()` appended (matches Rust `aes-gcm` and
 *   Dart `cryptography` package with `nonce: false`)
 * - Replay protection: receiver rejects `counter <= last_accepted_counter`
 * - Counter reset: `reset()` sets counter to 0 (used on host replacement)
 *
 * Uses only `node:crypto` — no external dependencies.
 */

import { createCipheriv, createDecipheriv, createHmac } from "node:crypto";

export const FRAME_VERSION = 0x01;
const HEADER_LEN = 9; // ver(1) + counter(8)
const TAG_LEN = 16;
const NONCE_LEN = 12;

export class E2eChannel {
  private counter = 0;
  private lastAcceptedCounter = 0;
  private readonly key: Buffer; // 32 bytes

  constructor(key: Buffer) {
    if (key.length !== 32) throw new Error("session key must be 32 bytes");
    this.key = key;
  }

  /**
   * Encrypt a plaintext message into an E2E frame.
   * Increments the counter before encrypting (counter starts at 1 for the first message).
   */
  seal(plaintext: Buffer): Buffer {
    this.counter += 1;
    const nonce = counterToNonce(this.counter);

    const cipher = createCipheriv("aes-256-gcm", this.key, nonce);
    const ciphertext = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    const tag = cipher.getAuthTag();

    // Frame: [ver(1)][counter_be(8)][ciphertext][tag(16)]
    const header = Buffer.allocUnsafe(HEADER_LEN);
    header.writeUInt8(FRAME_VERSION, 0);
    header.writeBigUInt64BE(BigInt(this.counter), 1);
    return Buffer.concat([header, ciphertext, tag]);
  }

  /**
   * Decrypt an E2E frame and return the plaintext.
   * Throws on version mismatch, replay (counter <= last accepted), or auth failure.
   */
  open(frame: Buffer): Buffer {
    if (frame.length < HEADER_LEN + TAG_LEN) {
      throw new Error("frame too short");
    }
    const version = frame.readUInt8(0);
    if (version !== FRAME_VERSION) {
      throw new Error(`unsupported frame version: ${version}`);
    }
    const counter = Number(frame.readBigUInt64BE(1));

    // Replay protection: reject counters at or below the last accepted one.
    if (counter <= this.lastAcceptedCounter) {
      throw new Error(`replay detected: counter ${counter} <= last ${this.lastAcceptedCounter}`);
    }

    const ciphertext = frame.subarray(HEADER_LEN, frame.length - TAG_LEN);
    const tag = frame.subarray(frame.length - TAG_LEN);
    const nonce = counterToNonce(counter);

    const decipher = createDecipheriv("aes-256-gcm", this.key, nonce);
    decipher.setAuthTag(tag);
    const plaintext = Buffer.concat([decipher.update(ciphertext), decipher.final()]);

    this.lastAcceptedCounter = counter;
    return plaintext;
  }

  /** Reset both send and receive counters (used on host replacement / re-pair). */
  reset(): void {
    this.counter = 0;
    this.lastAcceptedCounter = 0;
  }

  /** Current send counter (0 = no messages sent yet). */
  get sendCounter(): number {
    return this.counter;
  }

  /** Last accepted receive counter (0 = no messages received yet). */
  get recvCounter(): number {
    return this.lastAcceptedCounter;
  }
}

/**
 * Build the 12-byte AES-256-GCM nonce from a counter:
 * first 4 bytes zero, last 8 bytes = counter in big-endian.
 */
function counterToNonce(counter: number): Buffer {
  const nonce = Buffer.allocUnsafe(NONCE_LEN);
  nonce.writeUInt32BE(0, 0); // first 4 bytes = 0
  nonce.writeBigUInt64BE(BigInt(counter), 4);
  return nonce;
}

// ── HKDF-SHA256 key derivation ──────────────────────────────────────────────

/**
 * HKDF-Extract: HMAC-SHA256(salt, ikm) → PRK.
 */
function hkdfExtract(ikm: Buffer, salt: Buffer): Buffer {
  return createHmac("sha256", salt).update(ikm).digest();
}

/**
 * HKDF-Expand: expand PRK to the desired length using info.
 */
function hkdfExpand(prk: Buffer, info: Buffer, length: number): Buffer {
  const blocks: Buffer[] = [];
  let prev = Buffer.alloc(0);
  while (Buffer.concat(blocks).length < length) {
    const hmac = createHmac("sha256", prk);
    hmac.update(Buffer.concat([prev, info, Buffer.from([blocks.length + 1])]));
    prev = hmac.digest();
    blocks.push(prev);
  }
  return Buffer.concat(blocks).subarray(0, length);
}

/**
 * Derive the 32-byte E2E session key from a pair token.
 *
 * Matches the Dart/Flutter implementation:
 * `HKDF-SHA256(salt="shannon-relay", info="shannon-e2e-v1", ikm=UTF8(pairToken))` → 32 bytes.
 *
 * Both sides (gateway host and phone) share the same pair token from the QR code,
 * so both derive the same key.
 */
export function deriveSessionKey(pairToken: string): Buffer {
  const ikm = Buffer.from(pairToken, "utf8");
  const salt = Buffer.from("shannon-relay", "utf8");
  const info = Buffer.from("shannon-e2e-v1", "utf8");
  const prk = hkdfExtract(ikm, salt);
  return hkdfExpand(prk, info, 32);
}

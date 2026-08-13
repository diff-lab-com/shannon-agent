import { describe, expect, it } from "vitest";

import {
  E2eChannel,
  FRAME_VERSION,
  deriveSessionKey,
} from "../e2e.js";

const TEST_KEY = Buffer.alloc(32, 0x42);

describe("E2eChannel", () => {
  describe("seal/open round-trip", () => {
    it("round-trips a plaintext message", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);
      const plaintext = Buffer.from("hello relay world", "utf8");

      const frame = sender.seal(plaintext);
      const decoded = receiver.open(frame);

      expect(decoded.equals(plaintext)).toBe(true);
    });

    it("round-trips multiple messages in sequence", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      for (let i = 0; i < 10; i++) {
        const pt = Buffer.from(`message-${i}`, "utf8");
        const frame = sender.seal(pt);
        const decoded = receiver.open(frame);
        expect(decoded.equals(pt)).toBe(true);
      }
    });

    it("round-trips an empty plaintext", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      const frame = sender.seal(Buffer.alloc(0));
      const decoded = receiver.open(frame);
      expect(decoded.length).toBe(0);
    });
  });

  describe("frame format", () => {
    it("starts with version byte 0x01", () => {
      const ch = new E2eChannel(TEST_KEY);
      const frame = ch.seal(Buffer.from("x", "utf8"));
      expect(frame.readUInt8(0)).toBe(FRAME_VERSION);
    });

    it("counter starts at 1 for the first message", () => {
      const ch = new E2eChannel(TEST_KEY);
      const frame = ch.seal(Buffer.from("x", "utf8"));
      const counter = Number(frame.readBigUInt64BE(1));
      expect(counter).toBe(1);
    });

    it("counter increments monotonically", () => {
      const ch = new E2eChannel(TEST_KEY);
      ch.seal(Buffer.from("a", "utf8"));
      const frame2 = ch.seal(Buffer.from("b", "utf8"));
      expect(Number(frame2.readBigUInt64BE(1))).toBe(2);

      const frame3 = ch.seal(Buffer.from("c", "utf8"));
      expect(Number(frame3.readBigUInt64BE(1))).toBe(3);
    });

    it("frame length = header(9) + plaintext + tag(16)", () => {
      const ch = new E2eChannel(TEST_KEY);
      const pt = Buffer.from("payload", "utf8");
      const frame = ch.seal(pt);
      expect(frame.length).toBe(9 + pt.length + 16);
    });
  });

  describe("replay protection", () => {
    it("rejects a replayed frame (same counter twice)", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      const frame = sender.seal(Buffer.from("secret", "utf8"));
      // First open succeeds
      expect(receiver.open(frame).toString("utf8")).toBe("secret");
      // Replay → rejected
      expect(() => receiver.open(frame)).toThrow(/replay/i);
    });

    it("rejects a frame with counter lower than last accepted", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      // Send two messages
      const f1 = sender.seal(Buffer.from("a", "utf8"));
      const f2 = sender.seal(Buffer.from("b", "utf8"));

      // Accept both in order
      receiver.open(f1);
      receiver.open(f2);

      // Now try to open f1 again (counter 1 < last accepted 2)
      expect(() => receiver.open(f1)).toThrow(/replay/i);
    });
  });

  describe("counter monotonic enforcement", () => {
    it("receiver rejects out-of-order frames (counter 2 before 1)", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      // Seal two frames
      const f1 = sender.seal(Buffer.from("a", "utf8"));
      const f2 = sender.seal(Buffer.from("b", "utf8"));

      // Open f2 first (counter 2)
      receiver.open(f2);

      // Now f1 (counter 1) is rejected because 1 <= 2
      expect(() => receiver.open(f1)).toThrow(/replay/i);
    });
  });

  describe("reset()", () => {
    it("allows counter 1 after reset", () => {
      const sender = new E2eChannel(TEST_KEY);
      const receiver = new E2eChannel(TEST_KEY);

      // Exchange several messages
      for (let i = 0; i < 5; i++) {
        const f = sender.seal(Buffer.from(`m${i}`, "utf8"));
        receiver.open(f);
      }
      expect(receiver.recvCounter).toBe(5);
      expect(sender.sendCounter).toBe(5);

      // Reset both
      sender.reset();
      receiver.reset();
      expect(sender.sendCounter).toBe(0);
      expect(receiver.recvCounter).toBe(0);

      // Now counter 1 is accepted again
      const frame = sender.seal(Buffer.from("after-reset", "utf8"));
      expect(Number(frame.readBigUInt64BE(1))).toBe(1);
      const decoded = receiver.open(frame);
      expect(decoded.toString("utf8")).toBe("after-reset");
    });
  });

  describe("interop", () => {
    it("a channel opened with the same key can decrypt frames from another", () => {
      // Two independent channels with the same key behave as sender/receiver
      const alice = new E2eChannel(TEST_KEY);
      const bob = new E2eChannel(TEST_KEY);

      const msg = Buffer.from("cross-channel", "utf8");
      const frame = alice.seal(msg);
      const decoded = bob.open(frame);
      expect(decoded.equals(msg)).toBe(true);
    });

    it("fails to decrypt with a different key (auth tag mismatch)", () => {
      const sender = new E2eChannel(TEST_KEY);
      const wrongKey = Buffer.alloc(32, 0x99);
      const receiver = new E2eChannel(wrongKey);

      const frame = sender.seal(Buffer.from("mismatch", "utf8"));
      expect(() => receiver.open(frame)).toThrow();
    });
  });

  describe("error handling", () => {
    it("rejects a frame too short", () => {
      const ch = new E2eChannel(TEST_KEY);
      expect(() => ch.open(Buffer.alloc(5))).toThrow(/too short/i);
    });

    it("rejects an unsupported version", () => {
      const ch = new E2eChannel(TEST_KEY);
      // Version 0x02 with otherwise valid layout
      const frame = Buffer.alloc(9 + 16);
      frame.writeUInt8(0x02, 0);
      frame.writeBigUInt64BE(1n, 1);
      expect(() => ch.open(frame)).toThrow(/unsupported frame version/i);
    });

    it("constructor rejects a key that is not 32 bytes", () => {
      expect(() => new E2eChannel(Buffer.alloc(16))).toThrow("session key must be 32 bytes");
      expect(() => new E2eChannel(Buffer.alloc(48))).toThrow("session key must be 32 bytes");
    });
  });
});

describe("deriveSessionKey", () => {
  it("produces a 32-byte key", () => {
    const key = deriveSessionKey("test-pair-token");
    expect(key.length).toBe(32);
  });

  it("is deterministic for the same input", () => {
    const k1 = deriveSessionKey("my-token-123");
    const k2 = deriveSessionKey("my-token-123");
    expect(k1.equals(k2)).toBe(true);
  });

  it("differs for different tokens", () => {
    const k1 = deriveSessionKey("token-a");
    const k2 = deriveSessionKey("token-b");
    expect(k1.equals(k2)).toBe(false);
  });

  it("produces keys that work with E2eChannel round-trip", () => {
    const key = deriveSessionKey("interop-token");
    const sender = new E2eChannel(key);
    const receiver = new E2eChannel(key);
    const msg = Buffer.from("derived key works", "utf8");
    const frame = sender.seal(msg);
    expect(receiver.open(frame).equals(msg)).toBe(true);
  });
});

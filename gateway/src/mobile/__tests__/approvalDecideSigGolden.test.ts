/**
 * approval.decide signature golden vectors — the mono-side mirror of
 * shannon-mobile `test/approval_decide_sig_golden_test.dart` (contract doc:
 * docs/approval-decide-signing.md, mobile-side copy §6.5).
 *
 * Pins, from FIXED inputs (requestId = "req-golden-1", choice = "allow",
 * timestampMs = 1767225600000 = 2026-01-01T00:00:00.000Z, Ed25519 seed =
 * 0x11 × 32):
 *   v1 message  = "req-golden-1:allow"               (`approvalMessage`)
 *   v2 message  = "req-golden-1:allow:1767225600000" (`approvalMessageV2`)
 *   signature   = base64url(raw 64 bytes), PureEdDSA, NO pre-hash — same
 *                 posture as pair PoP / device.resume.
 * If mobile's golden test and this file ever disagree, the cross-repo contract
 * broke — fix the drift, never re-pin one side to the other blindly.
 */
import { createHash, createPrivateKey, createPublicKey, sign } from "node:crypto";
import type { KeyObject } from "node:crypto";

import { describe, expect, it } from "vitest";

import {
  approvalDecideTimestampWindowMs,
  approvalMessage,
  approvalMessageV2,
  deviceIdFromPublicKey,
  verifyMessage,
} from "../crypto.js";

const SEED = Buffer.alloc(32, 0x11);
const REQUEST_ID = "req-golden-1";
const CHOICE = "allow" as const;
const TIMESTAMP_MS = 1767225600000;

// Transcribed from test/approval_decide_sig_golden_test.dart — do not edit.
const PUB_B64U = "0EqyMnQrtKs6E2i9RhXk5tAiSrcaAWuvhSCjMsl3hzc";
const DEVICE_ID = "d1eb19fce56c9daa631a22110b0f90f8";
const V1_SIG_HEX =
  "9c98c3deedf5fa74105b20817f0e9ce57b6827530b1a61333e3f0982d12a9516478338f778542e8b9c893858fc33ed69c24e08adf254e38aecce038bc1f65c03";
const V2_SIG_HEX =
  "2eddd9e76f186488b36882c920d5c0da4f27bea614a51a05d5626fbd4dbd95db427ed784b1bfa3d4dc660bd85acb8230cd36e0af7eeae02931dfbacaf33e9405";

/** Ed25519 private key from a raw 32-byte seed (PKCS8 DER wrapper). */
function privateKeyFromSeed(seed: Buffer): KeyObject {
  const pkcs8Prefix = Buffer.from("302e020100300506032b657004220420", "hex");
  return createPrivateKey({ key: Buffer.concat([pkcs8Prefix, seed]), format: "der", type: "pkcs8" });
}

describe("approval.decide signature golden (cross-repo pin with shannon-mobile)", () => {
  it("seed 0x11×32 yields the pinned public key and deviceId", () => {
    const priv = privateKeyFromSeed(SEED);
    const jwk = createPublicKey(priv).export({ format: "jwk" }) as { x: string };
    expect(jwk.x).toBe(PUB_B64U);
    expect(deviceIdFromPublicKey(jwk.x)).toBe(DEVICE_ID);
  });

  it("v1 message shape signs to the pinned legacy signature", () => {
    const sig = sign(null, Buffer.from(approvalMessage(REQUEST_ID, CHOICE), "utf8"), privateKeyFromSeed(SEED));
    expect(sig.toString("base64url")).toBe(Buffer.from(V1_SIG_HEX, "hex").toString("base64url"));
    expect(sig.toString("hex")).toBe(V1_SIG_HEX);
  });

  it("v2 message shape signs to the pinned anti-replay signature", () => {
    const sig = sign(
      null,
      Buffer.from(approvalMessageV2(REQUEST_ID, CHOICE, TIMESTAMP_MS), "utf8"),
      privateKeyFromSeed(SEED),
    );
    expect(sig.toString("base64url")).toBe(Buffer.from(V2_SIG_HEX, "hex").toString("base64url"));
    expect(sig.toString("hex")).toBe(V2_SIG_HEX);
  });

  it("the gateway verifies both pinned signatures and rejects cross-shape", () => {
    const v1Sig = Buffer.from(V1_SIG_HEX, "hex").toString("base64url");
    const v2Sig = Buffer.from(V2_SIG_HEX, "hex").toString("base64url");
    expect(verifyMessage(PUB_B64U, approvalMessage(REQUEST_ID, CHOICE), v1Sig)).toBe(true);
    expect(verifyMessage(PUB_B64U, approvalMessageV2(REQUEST_ID, CHOICE, TIMESTAMP_MS), v2Sig)).toBe(true);
    // Cross-shape verification must fail — the message IS the version.
    expect(verifyMessage(PUB_B64U, approvalMessage(REQUEST_ID, CHOICE), v2Sig)).toBe(false);
    expect(verifyMessage(PUB_B64U, approvalMessageV2(REQUEST_ID, CHOICE, TIMESTAMP_MS), v1Sig)).toBe(false);
  });

  it("the pinned window constant matches the phone (±5 minutes)", () => {
    expect(approvalDecideTimestampWindowMs).toBe(300_000);
  });

  it("deviceId derivation helper agrees with the mobile-pinned value", () => {
    expect(createHash("sha256").update(PUB_B64U).digest("hex").slice(0, 32)).toBe(DEVICE_ID);
  });
});

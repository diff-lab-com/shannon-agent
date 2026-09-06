import { randomBytes } from "node:crypto";
import { createPrivateKey, createPublicKey, sign as cryptoSign, verify as cryptoVerify } from "node:crypto";

import { describe, expect, it } from "vitest";

import { MOBILE_PAGE_HTML } from "../web/page.js";
import { NACL_SOURCE } from "../web/naclSource.js";

/**
 * The PWA page (browser-as-phone) signs pair/resume/approval payloads with the
 * vendored TweetNaCl because WebCrypto's SubtleCrypto is unavailable in secure-
 * context-less plain-http LAN pages. These tests pin the contract that actually
 * matters: byte-exact interop with the gateway's node:crypto Ed25519 verifier,
 * and the page embedding a working client.
 */

function loadNacl(): any {
  const module = { exports: {} as any };
  // Same-realm evaluation (not runInNewContext) so the Uint8Array instances we
  // pass match the constructor TweetNaCl's checkArrayTypes expects.
  new Function("module", NACL_SOURCE)(module);
  return module.exports;
}

/** Derive the node Ed25519 keypair from the same 32-byte seed TweetNaCl uses. */
function nodeKeyPairFromSeed(seed: Uint8Array): { publicJwkX: string; privateJwk: any } {
  const d = Buffer.from(seed).toString("base64url");
  const priv = createPrivateKey({
    key: { kty: "OKP", crv: "Ed25519", x: "", d },
    format: "jwk",
  });
  const pub = createPublicKey(priv);
  const x = (pub.export({ format: "jwk" }) as { x: string }).x;
  return { publicJwkX: x, privateJwk: { kty: "OKP", crv: "Ed25519", x, d } };
}

describe("vendored TweetNaCl ↔ node:crypto Ed25519 interop", () => {
  it("derives the same public key as node for the same seed", () => {
    const nacl = loadNacl();
    for (let i = 0; i < 8; i++) {
      const seed = randomBytes(32);
      const kp = nacl.sign.keyPair.fromSeed(new Uint8Array(seed));
      expect(kp.secretKey).toHaveLength(64); // seed||pub, the nacl secretKey form
      const node = nodeKeyPairFromSeed(seed);
      expect(Buffer.from(kp.publicKey).toString("base64url")).toBe(node.publicJwkX);
    }
  });

  it("signatures verify under the gateway's verifier (the shannon/pair path)", () => {
    const nacl = loadNacl();
    const pubFromB64Url = (x: string): any =>
      createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x }, format: "jwk" });

    for (let i = 0; i < 8; i++) {
      const seed = randomBytes(32);
      const kp = nacl.sign.keyPair.fromSeed(new Uint8Array(seed));
      const x = (createPublicKey(
        createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", x: "", d: Buffer.from(seed).toString("base64url") }, format: "jwk" }),
      ).export({ format: "jwk" }) as { x: string }).x;

      // The canonical pair message: `${pair_token}:${device_public_key}`.
      const pubB64Url = Buffer.from(kp.publicKey).toString("base64url");
      const message = `${randomBytes(24).toString("base64url")}:${pubB64Url}`;
      const sig = nacl.sign.detached(new TextEncoder().encode(message), kp.secretKey);

      expect(cryptoVerify(null, Buffer.from(message, "utf8"), pubFromB64Url(x), Buffer.from(sig))).toBe(true);
      // A flipped bit must fail.
      const bad = Buffer.from(sig);
      bad[0] = Number(bad[0]) ^ 1;
      expect(cryptoVerify(null, Buffer.from(message, "utf8"), pubFromB64Url(x), bad)).toBe(false);
    }
  });

  it("node-signed messages verify under nacl (round trip)", () => {
    const nacl = loadNacl();
    const seed = randomBytes(32);
    const d = Buffer.from(seed).toString("base64url");
    const priv = createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", x: "", d }, format: "jwk" });
    const msg = new TextEncoder().encode("1746000000000:device-1");
    const sig = cryptoSign(null, Buffer.from(msg), priv);
    const kp = nacl.sign.keyPair.fromSeed(new Uint8Array(seed));
    expect(nacl.sign.detached.verify(msg, new Uint8Array(sig), kp.publicKey)).toBe(true);
  });
});

describe("PWA page", () => {
  it("embeds the protocol surface and the vendored signer", () => {
    expect(MOBILE_PAGE_HTML.startsWith("<!doctype html>")).toBe(true);
    expect(MOBILE_PAGE_HTML).toContain('name="viewport"');
    expect(MOBILE_PAGE_HTML).toContain("shannon/pair");
    expect(MOBILE_PAGE_HTML).toContain("shannon/device.resume");
    expect(MOBILE_PAGE_HTML).toContain("shannon/task.dispatch");
    expect(MOBILE_PAGE_HTML).toContain("shannon/task.list");
    expect(MOBILE_PAGE_HTML).toContain("nacl.sign.keyPair.fromSeed");
  });

  it("does not rely on SubtleCrypto (works on plain-http LAN pages)", () => {
    expect(MOBILE_PAGE_HTML).not.toContain("crypto.subtle");
    expect(MOBILE_PAGE_HTML).toContain("getRandomValues");
  });
});

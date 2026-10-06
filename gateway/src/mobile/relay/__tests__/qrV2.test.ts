import { describe, expect, it } from "vitest";

import { generateQrV2Payload, generateRelaySessionId } from "../qrV2.js";

describe("generateQrV2Payload", () => {
  it("produces the correct shape with all relay-mode fields", () => {
    const payload = generateQrV2Payload({
      scheme: "wss",
      host: "192.168.1.100",
      port: 33430,
      pairToken: "abc123token",
      expiresAt: 1700000000000,
      relayUrl: "wss://relay.shannon.example",
      relaySessionId: "deadbeefcafe",
    });

    expect(payload).toEqual({
      v: 2,
      scheme: "wss",
      host: "192.168.1.100",
      port: 33430,
      token: "abc123token",
      exp: 1700000000000,
      mode: "relay",
      relayEndpoint: "wss://relay.shannon.example",
      relaySessionId: "deadbeefcafe",
    });
  });

  it("includes hostE2EPubKey when provided", () => {
    const payload = generateQrV2Payload({
      scheme: "ws",
      host: "10.0.0.5",
      port: 33430,
      pairToken: "tok",
      expiresAt: 123,
      relayUrl: "wss://relay.example",
      relaySessionId: "sid",
      hostE2EPubKey: "base64urlkey",
    });

    expect(payload["hostE2EPubKey"]).toBe("base64urlkey");
  });

  it("omits hostE2EPubKey when not provided", () => {
    const payload = generateQrV2Payload({
      scheme: "ws",
      host: "10.0.0.5",
      port: 33430,
      pairToken: "tok",
      expiresAt: 123,
      relayUrl: "wss://relay.example",
      relaySessionId: "sid",
    });

    expect(payload["hostE2EPubKey"]).toBeUndefined();
  });

  it("omits hostE2EPubKey when null", () => {
    const payload = generateQrV2Payload({
      scheme: "ws",
      host: "10.0.0.5",
      port: 33430,
      pairToken: "tok",
      expiresAt: 123,
      relayUrl: "wss://relay.example",
      relaySessionId: "sid",
      hostE2EPubKey: null,
    });

    expect(payload["hostE2EPubKey"]).toBeUndefined();
  });

  it("always sets v=2 and mode=relay", () => {
    const payload = generateQrV2Payload({
      scheme: "ws",
      host: "h",
      port: 1,
      pairToken: "t",
      expiresAt: 0,
      relayUrl: "url",
      relaySessionId: "s",
    });

    expect(payload["v"]).toBe(2);
    expect(payload["mode"]).toBe("relay");
  });

  // Cross-repo contract (shannon-mobile pairing_controller M13 pre-check):
  // the phone rejects relay payloads missing relayEndpoint/relaySessionId;
  // hostE2EPubKey is the v0.3 X25519 handshake key (present on new gateways,
  // absent on legacy ones — the phone tolerates both). This pin keeps the
  // gateway's QR shape and the phone's validator from drifting apart.
  it("emits exactly the field set the phone's relay validator expects", () => {
    const payload = generateQrV2Payload({
      scheme: "wss",
      host: "192.168.1.100",
      port: 33430,
      pairToken: "tok",
      expiresAt: 123,
      relayUrl: "wss://relay.shannon.example",
      relaySessionId: "sid",
      hostE2EPubKey: "x25519-pub-b64url",
    });

    const keys = Object.keys(payload).sort();
    expect(keys).toEqual(
      [
        "exp",
        "host",
        "hostE2EPubKey",
        "mode",
        "port",
        "relayEndpoint",
        "relaySessionId",
        "scheme",
        "token",
        "v",
      ].sort(),
    );
    // The two fields the phone hard-requires are present and non-empty.
    expect(String(payload["relayEndpoint"]).length).toBeGreaterThan(0);
    expect(String(payload["relaySessionId"]).length).toBeGreaterThan(0);
  });
});

describe("generateRelaySessionId", () => {
  it("produces a 32-char hex string (UUID without dashes)", () => {
    const sid = generateRelaySessionId();
    expect(sid).toMatch(/^[0-9a-f]{32}$/);
  });

  it("produces unique IDs", () => {
    const ids = new Set<string>();
    for (let i = 0; i < 100; i++) {
      ids.add(generateRelaySessionId());
    }
    expect(ids.size).toBe(100);
  });

  it("contains no dashes", () => {
    const sid = generateRelaySessionId();
    expect(sid).not.toContain("-");
  });
});

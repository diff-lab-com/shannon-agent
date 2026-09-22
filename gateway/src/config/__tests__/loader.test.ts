import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { loadConfig, resolveConfigPath, validateConfig } from "../loader.js";

describe("resolveConfigPath", () => {
  it("uses the explicit arg first", () => {
    expect(resolveConfigPath("/explicit/path.json")).toBe("/explicit/path.json");
  });

  it("falls back to $SHANNON_GATEWAY_CONFIG when no arg", () => {
    const old = process.env.SHANNON_GATEWAY_CONFIG;
    process.env.SHANNON_GATEWAY_CONFIG = "/from/env.json";
    try {
      expect(resolveConfigPath()).toBe("/from/env.json");
    } finally {
      if (old === undefined) delete process.env.SHANNON_GATEWAY_CONFIG;
      else process.env.SHANNON_GATEWAY_CONFIG = old;
    }
  });
});

describe("validateConfig", () => {
  const ok = {
    engine: { wsUrl: "ws://127.0.0.1:33420/api/ws", httpBaseUrl: "http://127.0.0.1:33420" },
    adapters: [{ platform: "slack", enabled: true }],
  };

  it("accepts a minimal valid config", () => {
    const cfg = validateConfig(ok);
    expect(cfg.engine.wsUrl).toBe("ws://127.0.0.1:33420/api/ws");
    expect(cfg.adapters).toHaveLength(1);
    expect(cfg.adapters[0]?.platform).toBe("slack");
  });

  it("accepts optional model / options / secrets / logLevel", () => {
    const cfg = validateConfig({
      engine: { wsUrl: "ws://e/ws", httpBaseUrl: "http://e", model: "gpt-x" },
      adapters: [
        { platform: "telegram", enabled: true, options: { foo: 1 }, secrets: { botToken: "tg/bot" } },
      ],
      logLevel: "debug",
    });
    expect(cfg.engine.model).toBe("gpt-x");
    expect(cfg.adapters[0]?.options?.foo).toBe(1);
    expect(cfg.adapters[0]?.secrets?.botToken).toBe("tg/bot");
    expect(cfg.logLevel).toBe("debug");
  });

  it("rejects a missing engine object", () => {
    expect(() => validateConfig({ adapters: [] })).toThrow(/engine/);
  });

  it("rejects an empty wsUrl", () => {
    expect(() =>
      validateConfig({ engine: { wsUrl: "", httpBaseUrl: "http://e" }, adapters: [] }),
    ).toThrow(/wsUrl/);
  });

  it("rejects a non-array adapters field", () => {
    expect(() =>
      validateConfig({ engine: { wsUrl: "ws://e/ws", httpBaseUrl: "http://e" }, adapters: {} }),
    ).toThrow(/adapters/);
  });

  it("rejects an adapter missing platform", () => {
    expect(() =>
      validateConfig({
        engine: { wsUrl: "ws://e/ws", httpBaseUrl: "http://e" },
        adapters: [{ enabled: true }],
      }),
    ).toThrow(/platform/);
  });

  it("rejects an invalid logLevel", () => {
    expect(() =>
      validateConfig({
        engine: { wsUrl: "ws://e/ws", httpBaseUrl: "http://e" },
        adapters: [],
        logLevel: "loud",
      }),
    ).toThrow(/logLevel/);
  });

  it("accepts a valid optional mobile section", () => {
    const cfg = validateConfig({
      ...ok,
      mobile: {
        enabled: true,
        host: "127.0.0.1",
        port: 33430,
        tokensFile: "/tmp/tokens.jsonl",
        devicesFile: "/tmp/devices.json",
      },
    });
    expect(cfg.mobile).toEqual({
      enabled: true,
      host: "127.0.0.1",
      port: 33430,
      tokensFile: "/tmp/tokens.jsonl",
      devicesFile: "/tmp/devices.json",
    });
  });

  it("rejects mobile that is not an object", () => {
    expect(() => validateConfig({ ...ok, mobile: "on" })).toThrow(/mobile.*object/);
  });

  it("rejects a non-boolean mobile.enabled", () => {
    expect(() => validateConfig({ ...ok, mobile: { enabled: "yes" } })).toThrow(/mobile\.enabled/);
  });

  it("rejects an out-of-range mobile.port", () => {
    expect(() => validateConfig({ ...ok, mobile: { port: 99999 } })).toThrow(/mobile\.port/);
  });

  // ── P1-4 IM-channel block ──────────────────────────────────────────────

  it("accepts a valid optional im section and round-trips trigger options", () => {
    const cfg = validateConfig({
      ...ok,
      im: { taskLifecycle: false },
      adapters: [
        {
          platform: "telegram",
          enabled: true,
          options: { trigger: { groupMode: "any", dmDirect: false, prefix: "/shy" } },
        },
      ],
    });
    expect(cfg.im).toEqual({ taskLifecycle: false });
    expect(cfg.adapters[0]?.options).toEqual({
      trigger: { groupMode: "any", dmDirect: false, prefix: "/shy" },
    });
  });

  it("rejects im that is not an object", () => {
    expect(() => validateConfig({ ...ok, im: "off" })).toThrow(/im.*object/);
  });

  it("rejects a non-boolean im.taskLifecycle", () => {
    expect(() => validateConfig({ ...ok, im: { taskLifecycle: "off" } })).toThrow(
      /im\.taskLifecycle/,
    );
  });
});

describe("loadConfig", () => {
  it("reads and validates a file from disk", () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-cfg-"));
    const path = join(dir, "config.json");
    writeFileSync(
      path,
      JSON.stringify({
        engine: { wsUrl: "ws://e/ws", httpBaseUrl: "http://e" },
        adapters: [{ platform: "slack", enabled: true }],
      }),
    );
    try {
      const cfg = loadConfig(path);
      expect(cfg.adapters[0]?.platform).toBe("slack");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("throws on a missing file", () => {
    expect(() => loadConfig("/nonexistent/gateway/config.json")).toThrow(/cannot read/);
  });

  it("throws on invalid JSON", () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-cfg-"));
    const path = join(dir, "config.json");
    writeFileSync(path, "{ not json");
    try {
      expect(() => loadConfig(path)).toThrow(/invalid JSON/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  // ---- review §P0-6: config round-trip ----

  it("preserves engine.authTokenKey (review §P0-6)", () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-cfg-"));
    const path = join(dir, "config.json");
    writeFileSync(
      path,
      JSON.stringify({
        engine: {
          wsUrl: "ws://localhost:1/ws",
          httpBaseUrl: "http://localhost:1",
          authTokenKey: "my-secret-bearer",
        },
        adapters: [],
      }),
    );
    try {
      const cfg = loadConfig(path);
      expect(cfg.engine.authTokenKey).toBe("my-secret-bearer");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("preserves mobile.relay / mobile.tls / mobile.qrPayloadFile (review §P0-6)", () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-cfg-"));
    const path = join(dir, "config.json");
    writeFileSync(
      path,
      JSON.stringify({
        engine: { wsUrl: "ws://localhost:1/ws", httpBaseUrl: "http://localhost:1" },
        adapters: [],
        mobile: {
          enabled: true,
          relay: { url: "wss://relay.example", enabled: true },
          tls: { enabled: true },
          qrPayloadFile: "/tmp/qr.json",
        },
      }),
    );
    try {
      const cfg = loadConfig(path);
      expect(cfg.mobile?.relay).toEqual({ url: "wss://relay.example", enabled: true });
      expect(cfg.mobile?.tls).toEqual({ enabled: true });
      expect(cfg.mobile?.qrPayloadFile).toBe("/tmp/qr.json");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it("rejects unknown top-level field (review §P0-6)", () => {
    const dir = mkdtempSync(join(tmpdir(), "gw-cfg-"));
    const path = join(dir, "config.json");
    writeFileSync(
      path,
      JSON.stringify({
        engine: { wsUrl: "ws://localhost:1/ws", httpBaseUrl: "http://localhost:1" },
        adapters: [],
        mysteryField: true,
      }),
    );
    try {
      expect(() => loadConfig(path)).toThrow(/unknown top-level field 'mysteryField'/);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

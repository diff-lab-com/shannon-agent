import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import { type GatewayConfig, type LogLevel } from "./types.js";

/**
 * Resolve the config path:
 * explicit arg > `$SHANNON_GATEWAY_CONFIG` > `~/.shannon/gateway/config.json`.
 */
export function resolveConfigPath(explicit?: string): string {
  if (explicit) return explicit;
  const fromEnv = process.env.SHANNON_GATEWAY_CONFIG;
  if (fromEnv) return fromEnv;
  return join(homedir(), ".shannon", "gateway", "config.json");
}

/** Load and validate a GatewayConfig from disk. Throws on missing/invalid. */
export function loadConfig(path?: string): GatewayConfig {
  const resolved = resolveConfigPath(path);
  let raw: string;
  try {
    raw = readFileSync(resolved, "utf8");
  } catch (err) {
    throw new Error(
      `gateway config: cannot read ${resolved}: ${(err as Error).message}`,
    );
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (err) {
    throw new Error(
      `gateway config: invalid JSON in ${resolved}: ${(err as Error).message}`,
    );
  }
  return validateConfig(parsed, resolved);
}

const LOG_LEVELS: readonly LogLevel[] = ["debug", "info", "warn", "error"];

/**
 * Validate an already-parsed config blob. Pure — no I/O. Exported so tests can
 * exercise validation without touching disk.
 */
export function validateConfig(parsed: unknown, path = "<inline>"): GatewayConfig {
  if (typeof parsed !== "object" || parsed === null) {
    throw new Error(`gateway config ${path}: expected an object`);
  }
  const obj = parsed as Record<string, unknown>;

  const engine = obj.engine;
  if (typeof engine !== "object" || engine === null) {
    throw new Error(`gateway config ${path}: missing "engine" object`);
  }
  const e = engine as Record<string, unknown>;
  if (typeof e.wsUrl !== "string" || e.wsUrl.length === 0) {
    throw new Error(`gateway config ${path}: engine.wsUrl must be a non-empty string`);
  }
  if (typeof e.httpBaseUrl !== "string" || e.httpBaseUrl.length === 0) {
    throw new Error(`gateway config ${path}: engine.httpBaseUrl must be a non-empty string`);
  }
  if (e.model !== undefined && (typeof e.model !== "string" || e.model.length === 0)) {
    throw new Error(`gateway config ${path}: engine.model must be a non-empty string if present`);
  }

  if (!Array.isArray(obj.adapters)) {
    throw new Error(`gateway config ${path}: "adapters" must be an array`);
  }
  const adapters = (obj.adapters as unknown[]).map((a, i) => {
    if (typeof a !== "object" || a === null) {
      throw new Error(`gateway config ${path}: adapters[${i}] must be an object`);
    }
    const ac = a as Record<string, unknown>;
    if (typeof ac.platform !== "string" || ac.platform.length === 0) {
      throw new Error(`gateway config ${path}: adapters[${i}].platform must be a non-empty string`);
    }
    if (typeof ac.enabled !== "boolean") {
      throw new Error(`gateway config ${path}: adapters[${i}].enabled must be boolean`);
    }
    if (ac.options !== undefined && (typeof ac.options !== "object" || ac.options === null)) {
      throw new Error(`gateway config ${path}: adapters[${i}].options must be an object`);
    }
    if (ac.secrets !== undefined && (typeof ac.secrets !== "object" || ac.secrets === null)) {
      throw new Error(`gateway config ${path}: adapters[${i}].secrets must be an object`);
    }
    const out: GatewayConfig["adapters"][number] = {
      platform: ac.platform,
      enabled: ac.enabled,
    };
    if (ac.options) out.options = ac.options as Record<string, unknown>;
    if (ac.secrets) out.secrets = ac.secrets as Record<string, string>;
    return out;
  });

  const logLevel = obj.logLevel;
  if (
    logLevel !== undefined &&
    (typeof logLevel !== "string" || !LOG_LEVELS.includes(logLevel as LogLevel))
  ) {
    throw new Error(`gateway config ${path}: logLevel must be one of debug|info|warn|error`);
  }

  const result: GatewayConfig = { engine: { wsUrl: e.wsUrl, httpBaseUrl: e.httpBaseUrl }, adapters };
  if (typeof e.model === "string" && e.model.length > 0) result.engine.model = e.model;
  // review §P0-6: authTokenKey must round-trip; without it, every call to a
  // non-loopback-bound engine returns 401 even when configured.
  if (e.authTokenKey !== undefined) {
    if (typeof e.authTokenKey !== "string" || e.authTokenKey.length === 0) {
      throw new Error(
        `gateway config ${path}: engine.authTokenKey must be a non-empty string if present`,
      );
    }
    result.engine.authTokenKey = e.authTokenKey;
  }
  if (typeof logLevel === "string") result.logLevel = logLevel as LogLevel;

  // Optional P1-4 IM-channel block. Validates shape only; defaults (lifecycle
  // push on, mentionOrPrefix groups) resolve in bootstrap/trigger.ts.
  const im = obj.im;
  if (im !== undefined) {
    if (typeof im !== "object" || im === null) {
      throw new Error(`gateway config ${path}: "im" must be an object`);
    }
    const m = im as Record<string, unknown>;
    const parsedIm: GatewayConfig["im"] = {};
    if (m.taskLifecycle !== undefined) {
      if (typeof m.taskLifecycle !== "boolean") {
        throw new Error(`gateway config ${path}: im.taskLifecycle must be boolean`);
      }
      parsedIm.taskLifecycle = m.taskLifecycle;
    }
    result.im = parsedIm;
  }

  // Optional inbound mobile `shannon/*` server (P1.3). Validates shape only;
  // defaults (host/port/file paths) are resolved in bootstrap.
  const mobile = obj.mobile;
  if (mobile !== undefined) {
    if (typeof mobile !== "object" || mobile === null) {
      throw new Error(`gateway config ${path}: "mobile" must be an object`);
    }
    const m = mobile as Record<string, unknown>;
    const parsedMobile: GatewayConfig["mobile"] = {};
    if (m.enabled !== undefined) {
      if (typeof m.enabled !== "boolean") {
        throw new Error(`gateway config ${path}: mobile.enabled must be boolean`);
      }
      parsedMobile.enabled = m.enabled;
    }
    if (m.host !== undefined) {
      if (typeof m.host !== "string" || m.host.length === 0) {
        throw new Error(`gateway config ${path}: mobile.host must be a non-empty string`);
      }
      parsedMobile.host = m.host;
    }
    if (m.port !== undefined) {
      if (typeof m.port !== "number" || !Number.isInteger(m.port) || m.port < 0 || m.port > 65535) {
        throw new Error(`gateway config ${path}: mobile.port must be an integer in 0..65535`);
      }
      parsedMobile.port = m.port;
    }
    if (m.tokensFile !== undefined) {
      if (typeof m.tokensFile !== "string" || m.tokensFile.length === 0) {
        throw new Error(`gateway config ${path}: mobile.tokensFile must be a non-empty string`);
      }
      parsedMobile.tokensFile = m.tokensFile;
    }
    if (m.devicesFile !== undefined) {
      if (typeof m.devicesFile !== "string" || m.devicesFile.length === 0) {
        throw new Error(`gateway config ${path}: mobile.devicesFile must be a non-empty string`);
      }
      parsedMobile.devicesFile = m.devicesFile;
    }
    // review §P0-6: relay/tls/qrPayloadFile were silently dropped by the old
    // loader. Round-trip them so v0.12 TLS pinning + relay mode actually work.
    if (m.relay !== undefined) {
      if (typeof m.relay !== "object" || m.relay === null) {
        throw new Error(`gateway config ${path}: mobile.relay must be an object`);
      }
      const r = m.relay as Record<string, unknown>;
      if (typeof r.url !== "string" || r.url.length === 0) {
        throw new Error(`gateway config ${path}: mobile.relay.url must be a non-empty string`);
      }
      if (typeof r.enabled !== "boolean") {
        throw new Error(`gateway config ${path}: mobile.relay.enabled must be boolean`);
      }
      parsedMobile.relay = { url: r.url, enabled: r.enabled };
    }
    if (m.tls !== undefined) {
      if (typeof m.tls !== "object" || m.tls === null) {
        throw new Error(`gateway config ${path}: mobile.tls must be an object`);
      }
      const t = m.tls as Record<string, unknown>;
      const tls: { enabled?: boolean } = {};
      if (t.enabled !== undefined) {
        if (typeof t.enabled !== "boolean") {
          throw new Error(`gateway config ${path}: mobile.tls.enabled must be boolean`);
        }
        tls.enabled = t.enabled;
      }
      parsedMobile.tls = tls;
    }
    if (m.qrPayloadFile !== undefined) {
      if (typeof m.qrPayloadFile !== "string" || m.qrPayloadFile.length === 0) {
        throw new Error(
          `gateway config ${path}: mobile.qrPayloadFile must be a non-empty string`,
        );
      }
      parsedMobile.qrPayloadFile = m.qrPayloadFile;
    }
    result.mobile = parsedMobile;
  }

  // review §P0-6: surface unknown top-level keys as errors instead of silently
  // ignoring them — this prevents the next bug where a new field is added to
  // GatewayConfig but not yet plumbed through the loader.
  const knownTopLevel = new Set(["engine", "adapters", "logLevel", "im", "mobile"]);
  for (const k of Object.keys(obj)) {
    if (!knownTopLevel.has(k)) {
      throw new Error(
        `gateway config ${path}: unknown top-level field '${k}' (expected one of: ${[...knownTopLevel].join(", ")})`,
      );
    }
  }

  return result;
}

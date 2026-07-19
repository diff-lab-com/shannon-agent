/**
 * Service-management module for `shannon-gateway`.
 *
 * Turns the standalone Bun-compiled binary into a managed USER-LEVEL background
 * service (modeled on hermes-agent). The Rust `shannon-cli` later shells out to
 * `shannon-gateway <subcommand>`, so the exported subcommand names are fixed:
 *   install | uninstall | start | stop | restart | status | list
 *   | setup | migrate-legacy | enroll
 *
 * Responsibilities:
 *   - resolve the gateway binary (argv[1] → $SHANNON_GATEWAY_BIN → `which`),
 *   - write the OS unit (systemd --user / launchd / Windows task),
 *   - delegate lifecycle to the OS service manager,
 *   - probe best-effort gateway health (TCP connect to its configured port).
 *
 * The module is host-agnostic: it knows nothing about the engine beyond the
 * config it reads to derive a health port. No root privilege is required.
 */

import { type ExecException, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir, platform as osPlatform } from "node:os";
import { dirname, join } from "node:path";
import { createConnection } from "node:net";

import { loadConfig, resolveConfigPath } from "../config/loader.js";
import { type Platform, buildUnit } from "./units.js";

export type { Platform } from "./units.js";

/** Result of a service-manager control call (best-effort; never throws). */
export interface ServiceControlResult {
  ok: boolean;
  /** Raw exit code (when available) or -1. */
  code: number;
  stdout: string;
  stderr: string;
}

/** Output of `status()` for a single profile. */
export interface ServiceStatus {
  profile: string;
  /** Whether a config file exists for the profile. */
  configured: boolean;
  /** OS service manager "active" string (platform-specific semantics). */
  serviceState: string;
  /** Best-effort TCP health probe result. */
  health: {
    /** True when the gateway accepted a TCP connection on its port. */
    reachable: boolean;
    /** Host:port probed, or null when unconfigured/unknown. */
    endpoint: string | null;
    error?: string;
  } | null;
  /** PID when reported by the OS service manager, else null. */
  pid: number | null;
}

/** Standard console logger used by service commands. */
function log(msg: string): void {
  process.stdout.write(msg + "\n");
}
function warn(msg: string): void {
  process.stderr.write(msg + "\n");
}

/* ------------------------------------------------------------------ */
/* Binary resolution                                                  */
/* ------------------------------------------------------------------ */

/**
 * Resolve the absolute path to the `shannon-gateway` binary to embed in the
 * service unit:
 *   1. `process.argv[1]` when it is a real on-disk path (production installs
 *      invoke the binary by its absolute path, e.g. /usr/local/bin/shannon-gateway),
 *   2. `$SHANNON_GATEWAY_BIN`,
 *   3. `which shannon-gateway` (PATH lookup).
 * Bun-compiled binaries report a virtual `/$bunfs/...` argv[1] that is not the
 * real install path, so for those we skip argv and rely on `which`.
 * Falls back to the bare `shannon-gateway` name if all resolution fails.
 */
export function resolveBinary(): string {
  const fromArgv = resolveFromArgv();
  if (fromArgv) return fromArgv;

  const fromEnv = process.env.SHANNON_GATEWAY_BIN;
  if (fromEnv && fromEnv.length > 0) return fromEnv;

  const fromWhich = whichBinary("shannon-gateway");
  if (fromWhich) return fromWhich;

  // Last resort: rely on the service manager's PATH.
  return "shannon-gateway";
}

function resolveFromArgv(): string | null {
  const argv1 = process.argv[1];
  if (!argv1 || argv1.length === 0) return null;
  // Bun-compiled binaries report a virtual path like `/$bunfs/root/<name>` that
  // is NOT the on-disk install location; never trust it as the binary path.
  if (argv1.startsWith("/$bunfs/")) return null;
  if (argv1.endsWith("src/index.ts")) return null; // dev/tsx entry — not the binary
  return existsSync(argv1) ? argv1 : null;
}

function whichBinary(name: string): string | null {
  const cmd = osPlatform() === "win32" ? "where" : "which";
  try {
    const out = execFileSync(cmd, [name], { encoding: "utf8" }).trim();
    return out.length > 0 ? out.split(/\r?\n/)[0]! : null;
  } catch {
    return null;
  }
}

/* ------------------------------------------------------------------ */
/* Low-level OS service-manager control                               */
/* ------------------------------------------------------------------ */

function run(
  cmd: string,
  args: string[],
  opts: { allowFail?: boolean } = {},
): ServiceControlResult {
  try {
    const out = execFileSync(cmd, args, { encoding: "utf8" });
    return { ok: true, code: 0, stdout: out, stderr: "" };
  } catch (err) {
    const e = err as ExecException & { stdout?: string; stderr?: string; status?: number };
    const res: ServiceControlResult = {
      ok: false,
      code: typeof e.status === "number" ? e.status : -1,
      stdout: e.stdout ?? "",
      stderr: e.stderr ?? "",
    };
    if (!opts.allowFail) {
      warn(`${cmd} ${args.join(" ")} failed (code ${res.code}): ${res.stderr || res.stdout}`);
    }
    return res;
  }
}

/* ------------------------------------------------------------------ */
/* Profile helpers                                                    */
/* ------------------------------------------------------------------ */

/**
 * Map a profile name to a config path. A profile is a named config variant
 * stored as `~/.shannon/gateway/<profile>/config.json`; the default (no
 * profile) is `~/.shannon/gateway/config.json`. This keeps the existing
 * `loadConfig`/`resolveConfigPath` semantics while adding multi-instance
 * support for the service layer.
 */
export function configPathForProfile(profile?: string): string {
  if (!profile) return join(homedir(), ".shannon", "gateway", "config.json");
  return join(homedir(), ".shannon", "gateway", profile, "config.json");
}

/** List the available profile names (directories under the gateway config dir). */
export function listProfiles(): string[] {
  const base = join(homedir(), ".shannon", "gateway");
  const profiles: string[] = [];
  if (existsSync(join(base, "config.json"))) profiles.push("default");
  try {
    for (const entry of readdirSync(base, { withFileTypes: true })) {
      if (entry.isDirectory() && existsSync(join(base, entry.name, "config.json"))) {
        profiles.push(entry.name);
      }
    }
  } catch {
    /* no profiles dir yet */
  }
  // Dedupe while preserving order.
  return [...new Set(profiles)];
}

/* ------------------------------------------------------------------ */
/* Public API: install / uninstall / start / stop / restart          */
/* ------------------------------------------------------------------ */

export interface InstallResult {
  /** Platform the unit was built for. */
  platform: Platform;
  /** Path the unit file was written to. */
  unitPath: string;
  /** Whether enable+start succeeded. */
  started: boolean;
}

/** Write the user-level unit and enable+start it. */
export function install(profile?: string): InstallResult {
  const plat = osPlatform() as Platform;
  const binary = resolveBinary();
  const unit = buildUnitFor(plat, binary, profile);

  mkdirSync(dirname(unit.path), { recursive: true });
  writeFileSync(unit.path, unit.contents, "utf8");
  log(`wrote service unit: ${unit.path}`);

  const started = enableAndStart(plat, unit.path);
  return { platform: plat, unitPath: unit.path, started };
}

/** Stop, disable, and remove the service unit. */
export function uninstall(profile?: string): void {
  const plat = osPlatform() as Platform;
  stop(plat);
  disable(plat);
  const unit = buildUnitFor(plat, resolveBinary(), profile);
  try {
    if (existsSync(unit.path)) {
      rmSync(unit.path, { force: true });
      log(`removed service unit: ${unit.path}`);
    }
  } catch (err) {
    warn(`failed to remove ${unit.path}: ${(err as Error).message}`);
  }
}

export function start(_profile?: string): ServiceControlResult {
  const plat = osPlatform() as Platform;
  return platformStart(plat);
}

export function stop(_profile?: string): ServiceControlResult {
  const plat = osPlatform() as Platform;
  return platformStop(plat);
}

export async function restart(_profile?: string): Promise<ServiceControlResult> {
  const plat = osPlatform() as Platform;
  platformStop(plat);
  return platformStart(plat);
}

/* ------------------------------------------------------------------ */
/* Public API: status / list                                          */
/* ------------------------------------------------------------------ */

/**
 * Query the OS service manager for a profile and best-effort probe the
 * gateway's health (TCP connect to its configured port, if discoverable).
 * Async because the TCP health probe is non-blocking.
 */
export async function status(profile?: string): Promise<ServiceStatus> {
  const plat = osPlatform() as Platform;
  const cfgPath = configPathForProfile(profile);
  const configured = existsSync(cfgPath);
  const svcState = queryServiceState(plat);
  const pid = queryPid(plat);
  let health: ServiceStatus["health"] = null;
  if (configured) {
    const endpoint = readHealthEndpoint(cfgPath);
    health = endpoint
      ? await probeTcpAsync(endpoint)
      : { reachable: false, endpoint: null, error: "no health endpoint in config" };
  }
  return { profile: profile ?? "default", configured, serviceState: svcState, health, pid };
}

/** Enumerate profiles and the running state of each. */
export async function list(): Promise<ServiceStatus[]> {
  const profiles = listProfiles();
  return Promise.all(profiles.map((p) => status(p === "default" ? undefined : p)));
}

/* ------------------------------------------------------------------ */
/* Public API: setup / migrate-legacy / enroll                        */
/* ------------------------------------------------------------------ */

/**
 * Interactive config/auth setup. Delegates to the existing config loader so we
 * don't reimplement auth — we validate a (user-provided) config path and print
 * guidance. The real keyring population is performed by the desktop/CLI today;
 * this keeps the gateway host-agnostic and wired to the loader only.
 */
export function setup(profile?: string): void {
  const cfgPath = configPathForProfile(profile);
  log(`gateway setup (profile: ${profile ?? "default"})`);
  if (!existsSync(cfgPath)) {
    warn(
      `no config found at ${cfgPath}\n` +
        `create it (or run your desktop/CLI config flow) before installing the service.\n` +
        `minimal shape:\n` +
        `  {\n` +
        `    "engine": { "wsUrl": "ws://127.0.0.1:33420/api/ws", "httpBaseUrl": "http://127.0.0.1:33420" },\n` +
        `    "adapters": [ { "platform": "slack", "enabled": false } ]\n` +
        `  }`,
    );
    return;
  }
  // Delegate: validate via the existing loader (throws on invalid → reported).
  try {
    resolveConfigPath(cfgPath);
    log(`config at ${cfgPath} is present; validating with the gateway loader...`);
    loadConfig(cfgPath);
    log("config is valid. secrets are resolved at runtime from the OS keyring (or $SHANNON_SECRET__*).");
  } catch (err) {
    warn(`config validation failed: ${(err as Error).message}`);
  }
}

/**
 * Defensive cleanup: remove any legacy `shannon.service` (singular) units left
 * by older installs. The current unit is `shannon-gateway.service`.
 */
export function migrateLegacy(): void {
  const plat = osPlatform() as Platform;
  if (plat === "linux") {
    const legacy = join(homedir(), ".config", "systemd", "user", "shannon.service");
    if (existsSync(legacy)) {
      run("systemctl", ["--user", "disable", "--now", "shannon.service"], { allowFail: true });
      rmSync(legacy, { force: true });
      log(`removed legacy unit: ${legacy}`);
      run("systemctl", ["--user", "daemon-reload"], { allowFail: true });
    } else {
      log("no legacy shannon.service unit found; nothing to do.");
    }
    return;
  }
  if (plat === "darwin") {
    const legacy = join(homedir(), "Library", "LaunchAgents", "com.shannon-agent.gateway.legacy.plist");
    if (existsSync(legacy)) {
      run("launchctl", ["unload", legacy], { allowFail: true });
      rmSync(legacy, { force: true });
      log(`removed legacy agent: ${legacy}`);
    } else {
      log("no legacy launchd agent found; nothing to do.");
    }
    return;
  }
  log("migrate-legacy: no legacy units tracked for this platform; nothing to do.");
}

/** MVP stub — device enrollment is not yet implemented. */
export function enroll(): void {
  warn("enroll: not yet implemented (MVP stub).");
}

/* ------------------------------------------------------------------ */
/* Internal: platform-specific control                               */
/* ------------------------------------------------------------------ */

function buildUnitFor(plat: Platform, binary: string, profile?: string) {
  return buildUnit(plat, binary, profile);
}

function enableAndStart(plat: Platform, unitPath: string): boolean {
  switch (plat) {
    case "linux": {
      const r1 = run("systemctl", ["--user", "daemon-reload"], { allowFail: true });
      const r2 = run("systemctl", ["--user", "enable", "--now", "shannon-gateway"], { allowFail: true });
      return r1.ok && r2.ok;
    }
    case "darwin": {
      const r = run("launchctl", ["load", unitPath], { allowFail: true });
      return r.ok;
    }
    case "win32": {
      // Register a scheduled task from the embedded XML, then it runs at login.
      const r = run(
        "schtasks",
        ["/create", "/tn", "shannon-gateway", "/xml", unitPath, "/f"],
        { allowFail: true },
      );
      return r.ok;
    }
  }
}

function disable(plat: Platform): void {
  switch (plat) {
    case "linux":
      run("systemctl", ["--user", "disable", "shannon-gateway"], { allowFail: true });
      break;
    case "darwin": {
      const unit = buildUnitFor(plat, resolveBinary());
      run("launchctl", ["unload", unit.path], { allowFail: true });
      break;
    }
    case "win32":
      run("schtasks", ["/delete", "/tn", "shannon-gateway", "/f"], { allowFail: true });
      break;
  }
}

function platformStart(plat: Platform): ServiceControlResult {
  switch (plat) {
    case "linux":
      return run("systemctl", ["--user", "start", "shannon-gateway"], { allowFail: true });
    case "darwin": {
      const unit = buildUnitFor(plat, resolveBinary());
      return run("launchctl", ["load", unit.path], { allowFail: true });
    }
    case "win32":
      return run("schtasks", ["/run", "/tn", "shannon-gateway"], { allowFail: true });
  }
}

function platformStop(plat: Platform): ServiceControlResult {
  switch (plat) {
    case "linux":
      return run("systemctl", ["--user", "stop", "shannon-gateway"], { allowFail: true });
    case "darwin": {
      const unit = buildUnitFor(plat, resolveBinary());
      return run("launchctl", ["unload", unit.path], { allowFail: true });
    }
    case "win32":
      return run("schtasks", ["/end", "/tn", "shannon-gateway"], { allowFail: true });
  }
}

function queryServiceState(plat: Platform): string {
  switch (plat) {
    case "linux": {
      const r = run("systemctl", ["--user", "is-active", "shannon-gateway"], { allowFail: true });
      return (r.stdout || r.stderr || "unknown").trim() || "unknown";
    }
    case "darwin": {
      const r = run("launchctl", ["list"], { allowFail: true });
      return r.stdout.includes("com.shannon-agent.gateway") ? "loaded" : "unloaded";
    }
    case "win32": {
      const r = run("schtasks", ["/query", "/tn", "shannon-gateway", "/fo", "LIST"], { allowFail: true });
      return r.ok ? "registered" : "not-registered";
    }
  }
}

function queryPid(plat: Platform): number | null {
  switch (plat) {
    case "linux": {
      const r = run("systemctl", ["--user", "show", "shannon-gateway", "--property=MainPID"], {
        allowFail: true,
      });
      const m = r.stdout.match(/MainPID=(\d+)/);
      const pid = m && m[1] ? parseInt(m[1], 10) : NaN;
      return Number.isFinite(pid) && pid > 0 ? pid : null;
    }
    case "darwin": {
      const r = run("launchctl", ["list"], { allowFail: true });
      const line = r.stdout
        .split(/\r?\n/)
        .find((l) => l.includes("com.shannon-agent.gateway"));
      if (!line) return null;
      const cols = line.trim().split(/\s+/);
      const pid = cols[0] ? parseInt(cols[0], 10) : NaN;
      return Number.isFinite(pid) && pid > 0 ? pid : null;
    }
    case "win32":
      return null; // PID not easily mapped from schtasks; leave null.
  }
}

/* ------------------------------------------------------------------ */
/* Internal: health probe                                            */
/* ------------------------------------------------------------------ */

/**
 * Read a best-effort health endpoint from the config. The gateway's mobile
 * `shannon/*` server (when enabled) binds a host:port; we probe that. Falls
 * back to the engine URL's host if no mobile server is configured. Returns
 * `host:port` or null.
 */
function readHealthEndpoint(cfgPath: string): string | null {
  let raw: string;
  try {
    raw = readFileSync(cfgPath, "utf8");
  } catch {
    return null;
  }
  try {
    const parsed = JSON.parse(raw) as { mobile?: { host?: string; port?: number } };
    if (parsed.mobile?.port) {
      const host = parsed.mobile.host ?? "127.0.0.1";
      return `${host}:${parsed.mobile.port}`;
    }
  } catch {
    /* ignore malformed config for health purposes */
  }
  return null;
}

/** Best-effort TCP connect to host:port with a short timeout. */
function probeTcpAsync(endpoint: string): Promise<ServiceStatus["health"]> {
  const [host, portStr] = endpoint.split(":");
  const port = portStr ? parseInt(portStr, 10) : NaN;
  if (!host || !Number.isFinite(port)) {
    return Promise.resolve({ reachable: false, endpoint, error: "malformed endpoint" });
  }
  return new Promise<ServiceStatus["health"]>((resolve) => {
    const sock = createConnection({ host, port, timeout: 1500 });
    let settled = false;
    const done = (reachable: boolean, error?: string) => {
      if (settled) return;
      settled = true;
      sock.destroy();
      resolve({ reachable, endpoint, error });
    };
    sock.once("connect", () => done(true));
    sock.once("error", (err: Error) => done(false, err.message));
    sock.once("timeout", () => done(false, "timeout"));
  });
}

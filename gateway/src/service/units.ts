/**
 * OS service-unit builders for `shannon-gateway`.
 *
 * Each builder returns the unit file contents (and its on-disk path) for the
 * current platform. The service manager commands (enable/start/stop/...) are
 * issued by the caller (`service.ts`) via the OS CLI. Units are USER-LEVEL
 * only (no root): systemd --user, ~/Library/LaunchAgents, and a Windows
 * scheduled task at login.
 *
 * The gateway binary is launched as `run [--profile <p>]` so the standalone
 * entry point and the service entry point share one code path.
 */

import { homedir } from "node:os";
import { join } from "node:path";

/** One of the supported OS families (mirrors `process.platform` values used). */
export type Platform = "linux" | "darwin" | "win32";

/** Where the per-platform unit file lives. */
export interface ServiceUnit {
  /** Absolute path the unit file should be written to. */
  path: string;
  /** Rendered unit-file contents. */
  contents: string;
}

/**
 * Build the user-level service unit for the given platform.
 *
 * @param platform  `linux` | `darwin` | `win32`.
 * @param binary    Absolute path to the resolved `shannon-gateway` binary.
 * @param profile   Optional profile name (mapped to `--profile <p>`).
 */
export function buildUnit(
  platform: Platform,
  binary: string,
  profile?: string,
): ServiceUnit {
  switch (platform) {
    case "linux":
      return buildSystemdUnit(binary, profile);
    case "darwin":
      return buildLaunchdUnit(binary, profile);
    case "win32":
      return buildWindowsUnit(binary, profile);
  }
}

/**
 * Build the user-level service unit for the ENGINE (`shannon serve`).
 *
 * The gateway is a WebSocket *client* — it connects to a running engine and
 * never spawns one. Registering the engine alongside the gateway closes the
 * "installed the gateway, nothing to talk to" gap: `install` writes both
 * units, and on systemd the gateway unit additionally orders itself after
 * the engine (After=/Wants=). Platforms without inter-unit ordering
 * (launchd, schtasks) rely on both units' keep-alive plus the gateway's
 * reconnect loop.
 *
 * @param platform      `linux` | `darwin` | `win32`.
 * @param engineBinary  Absolute path to the `shannon` CLI binary.
 */
export function buildEngineUnit(
  platform: Platform,
  engineBinary: string,
): ServiceUnit {
  switch (platform) {
    case "linux":
      return buildSystemdEngineUnit(engineBinary);
    case "darwin":
      return buildLaunchdEngineUnit(engineBinary);
    case "win32":
      return buildWindowsEngineUnit(engineBinary);
  }
}

function runArgs(profile?: string): string[] {
  return profile ? ["run", "--profile", profile] : ["run"];
}

/** systemd user service: ~/.config/systemd/user/shannon-gateway.service */
function buildSystemdUnit(binary: string, profile?: string): ServiceUnit {
  const args = runArgs(profile);
  const execStart = [binary, ...args]
    .map((a) => (a.includes(" ") ? `"${a}"` : a))
    .join(" ");
  const contents = [
    "[Unit]",
    "Description=Shannon Gateway (chat platform bridge to the Shannon engine)",
    // Order after the engine unit so `systemctl --user start shannon-gateway`
    // (or a login-session start) brings the engine up first; Wants= pulls it
    // in even when only the gateway is started explicitly.
    "After=network-online.target shannon-serve.service",
    "Wants=network-online.target shannon-serve.service",
    "",
    "[Service]",
    "Type=simple",
    `ExecStart=${execStart}`,
    "Restart=on-failure",
    "RestartSec=2",
    "",
    "[Install]",
    "WantedBy=default.target",
    "",
  ].join("\n");
  const path = join(
    homedir(),
    ".config",
    "systemd",
    "user",
    "shannon-gateway.service",
  );
  return { path, contents };
}

/** launchd agent: ~/Library/LaunchAgents/com.shannon-agent.gateway.plist */
function buildLaunchdUnit(binary: string, profile?: string): ServiceUnit {
  const args = runArgs(profile);
  const programArgs = ["<string>" + escapeXml(binary) + "</string>"]
    .concat(
      args.map((a) => `<string>${escapeXml(a)}</string>`),
    )
    .join("\n      ");
  const contents = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.shannon-agent.gateway</string>
  <key>ProgramArguments</key>
  <array>
      ${programArgs}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>${escapeXml(join(homedir(), ".shannon", "gateway", "gateway.log"))}</string>
  <key>StandardErrorPath</key>
  <string>${escapeXml(join(homedir(), ".shannon", "gateway", "gateway.err.log"))}</string>
</dict>
</plist>
`;
  const path = join(
    homedir(),
    "Library",
    "LaunchAgents",
    "com.shannon-agent.gateway.plist",
  );
  return { path, contents };
}

/**
 * Windows scheduled task. Units on Windows are registered imperatively via
 * `schtasks /create` rather than a static file, so this returns the `.xml` task
 * definition for documentation/inspection plus the `schtasks` arguments the
 * caller should run. We still expose a path (the XML is written next to config)
 * so `uninstall`/`status` can reference a stable file.
 */
function buildWindowsUnit(binary: string, profile?: string): ServiceUnit {
  const args = runArgs(profile);
  const command = [binary, ...args]
    .map((a) => a.includes(" ") ? `"${a}"` : a)
    .join(" ");
  const commandXml = escapeXml(command);
  const contents = `<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Shannon Gateway (chat platform bridge to the Shannon engine)</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT2M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions>
    <Exec>
      <Command>${commandXml}</Command>
    </Exec>
  </Actions>
</Task>
`;
  const path = join(homedir(), ".shannon", "gateway", "shannon-gateway.task.xml");
  return { path, contents };
}

/** systemd user service: ~/.config/systemd/user/shannon-serve.service */
function buildSystemdEngineUnit(engineBinary: string): ServiceUnit {
  const execStart = engineBinary.includes(" ")
    ? `"${engineBinary}" serve`
    : `${engineBinary} serve`;
  const contents = [
    "[Unit]",
    "Description=Shannon Engine (shannon serve — local API/WebSocket server on :33420)",
    "After=network-online.target",
    "Wants=network-online.target",
    "",
    "[Service]",
    "Type=simple",
    `ExecStart=${execStart}`,
    "Restart=on-failure",
    "RestartSec=2",
    "",
    "[Install]",
    "WantedBy=default.target",
    "",
  ].join("\n");
  const path = join(
    homedir(),
    ".config",
    "systemd",
    "user",
    "shannon-serve.service",
  );
  return { path, contents };
}

/** launchd agent: ~/Library/LaunchAgents/com.shannon-agent.serve.plist */
function buildLaunchdEngineUnit(engineBinary: string): ServiceUnit {
  const programArgs = [
    `<string>${escapeXml(engineBinary)}</string>`,
    "<string>serve</string>",
  ].join("\n      ");
  const contents = `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.shannon-agent.serve</string>
  <key>ProgramArguments</key>
  <array>
      ${programArgs}
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>${escapeXml(join(homedir(), ".shannon", "serve.log"))}</string>
  <key>StandardErrorPath</key>
  <string>${escapeXml(join(homedir(), ".shannon", "serve.err.log"))}</string>
</dict>
</plist>
`;
  const path = join(
    homedir(),
    "Library",
    "LaunchAgents",
    "com.shannon-agent.serve.plist",
  );
  return { path, contents };
}

/**
 * Windows scheduled task for the engine, mirroring buildWindowsUnit: the task
 * is registered imperatively via `schtasks /create /tn shannon-serve /xml`;
 * the XML is kept on disk for inspection and uninstall/status reference.
 */
function buildWindowsEngineUnit(engineBinary: string): ServiceUnit {
  const command = engineBinary.includes(" ")
    ? `"${engineBinary}" serve`
    : `${engineBinary} serve`;
  const contents = `<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Shannon Engine (shannon serve — local API/WebSocket server on :33420)</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
    </LogonTrigger>
  </Triggers>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT2M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions>
    <Exec>
      <Command>${escapeXml(command)}</Command>
    </Exec>
  </Actions>
</Task>
`;
  const path = join(homedir(), ".shannon", "shannon-serve.task.xml");
  return { path, contents };
}

function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&apos;");
}

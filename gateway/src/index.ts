/**
 * shannon-gateway entry point.
 *
 * Loads `~/.shannon/gateway/config.json` (or `$SHANNON_GATEWAY_CONFIG`, or the
 * `--config <path>` arg), wires the four layers via `bootstrap()`, and runs
 * until SIGINT/SIGTERM.
 *
 * All eight platform adapter factories register here. The router looks them up
 * by `config.platform`; bootstrap throws at startup if an enabled platform has
 * no factory. Real-credential end-to-end smoke is a separate manual step per
 * platform (bot tokens live in the OS keyring, never in this repo).
 */
import { bootstrap, type AdapterFactory } from "./bootstrap.js";
import { loadConfig } from "./config/loader.js";
import { createConsoleLogger } from "./logger.js";

import { createSlackAdapter } from "./adapters/slack/slackAdapter.js";
import { createTelegramAdapter } from "./adapters/telegram/telegramAdapter.js";
import { createDiscordAdapter } from "./adapters/discord/discordAdapter.js";
import { createMatrixAdapter } from "./adapters/matrix/matrixAdapter.js";
import { createWhatsAppAdapter } from "./adapters/whatsapp/whatsappAdapter.js";
import { createWeComAdapter } from "./adapters/wecom/wecomAdapter.js";
import { createFeishuAdapter } from "./adapters/feishu/feishuAdapter.js";
import { createDingTalkAdapter } from "./adapters/dingtalk/dingtalkAdapter.js";

import {
  enroll,
  install,
  list,
  migrateLegacy,
  restart,
  setup,
  start,
  status,
  stop,
  uninstall,
} from "./service/service.js";

export { GATEWAY_VERSION } from "./version.js";

/** Platform id → factory. One adapter per platform; the router looks up by id. */
const factories = new Map<string, AdapterFactory>([
  ["slack", createSlackAdapter],
  ["telegram", createTelegramAdapter],
  ["discord", createDiscordAdapter],
  ["matrix", createMatrixAdapter],
  ["whatsapp", createWhatsAppAdapter],
  ["wecom", createWeComAdapter],
  ["feishu", createFeishuAdapter],
  ["dingtalk", createDingTalkAdapter],
]);

/** Top-level subcommands handled by the service module. */
const SERVICE_SUBCOMMANDS = new Set([
  "install",
  "uninstall",
  "start",
  "stop",
  "restart",
  "status",
  "list",
  "setup",
  "migrate-legacy",
  "enroll",
]);

/** Optional `--profile <name>` parser shared by run + service subcommands. */
function parseProfile(argv: string[]): string | undefined {
  const idx = argv.indexOf("--profile");
  if (idx >= 0 && idx + 1 < argv.length) return argv[idx + 1];
  return undefined;
}

/** Load config (optionally from --config / --profile) and run until signaled. */
async function runGateway(extraArgs: string[]): Promise<void> {
  const logger = createConsoleLogger("info");

  let configPath: string | undefined;
  const cfgIdx = extraArgs.indexOf("--config");
  if (cfgIdx >= 0 && cfgIdx + 1 < extraArgs.length) {
    configPath = extraArgs[cfgIdx + 1];
  }

  const config = loadConfig(configPath);

  if (factories.size === 0 && config.adapters.some((a) => a.enabled)) {
    logger.warn(
      "no platform adapter factories are registered but the config enables adapters; " +
        "bootstrap will fail.",
    );
  }

  const handle = await bootstrap(config, { factories });

  const shutdown = async (sig: string): Promise<void> => {
    logger.info(`received ${sig}; shutting down`);
    try {
      await handle.stop();
    } finally {
      process.exit(0);
    }
  };
  process.on("SIGINT", () => void shutdown("SIGINT"));
  process.on("SIGTERM", () => void shutdown("SIGTERM"));
}

/** Dispatch a service-management subcommand. Returns an exit code. */
async function runServiceCommand(sub: string): Promise<number> {
  const profile = parseProfile(process.argv.slice(3));
  switch (sub) {
    case "install": {
      const r = install(profile);
      process.stdout.write(
        `installed (${r.platform}) unit at ${r.unitPath}; started=${r.started}\n`,
      );
      return r.started ? 0 : 1;
    }
    case "uninstall":
      uninstall(profile);
      process.stdout.write("uninstalled.\n");
      return 0;
    case "start": {
      const r = start(profile);
      process.stdout.write(`start: ${r.ok ? "ok" : "failed"}\n`);
      return r.ok ? 0 : 1;
    }
    case "stop": {
      const r = stop(profile);
      process.stdout.write(`stop: ${r.ok ? "ok" : "failed"}\n`);
      return r.ok ? 0 : 1;
    }
    case "restart": {
      const r = await restart(profile);
      process.stdout.write(`restart: ${r.ok ? "ok" : "failed"}\n`);
      return r.ok ? 0 : 1;
    }
    case "status": {
      const s = await status(profile);
      process.stdout.write(
        `profile=${s.profile} configured=${s.configured} state=${s.serviceState} ` +
          `pid=${s.pid ?? "-"} health=${s.health ? (s.health.reachable ? "reachable" : "down") : "n/a"}\n`,
      );
      return 0;
    }
    case "list": {
      const all = await list();
      for (const s of all) {
        process.stdout.write(
          `profile=${s.profile} configured=${s.configured} state=${s.serviceState} ` +
            `health=${s.health ? (s.health.reachable ? "reachable" : "down") : "n/a"}\n`,
        );
      }
      return 0;
    }
    case "setup":
      setup(profile);
      return 0;
    case "migrate-legacy":
      migrateLegacy();
      return 0;
    case "enroll":
      enroll();
      return 0;
    default:
      process.stderr.write(`unknown subcommand: ${sub}\n`);
      return 2;
  }
}

async function main(): Promise<void> {
  const sub = process.argv[2];

  // Service subcommands take priority over legacy bare invocation.
  if (sub && SERVICE_SUBCOMMANDS.has(sub)) {
    const code = await runServiceCommand(sub);
    if (code !== 0) process.exit(code);
    return;
  }

  // `run` is the explicit entry point used by the service unit; bare invocation
  // (no subcommand) is kept for dev/direct use and behaves identically.
  if (!sub || sub === "run") {
    const rest = sub === "run" ? process.argv.slice(3) : process.argv.slice(2);
    await runGateway(rest);
    return;
  }

  process.stderr.write(
    `usage: shannon-gateway [run] [--config <path>] [--profile <name>]\n` +
      `       shannon-gateway <${[...SERVICE_SUBCOMMANDS].join("|")}> [--profile <name>]\n`,
  );
  process.exit(2);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((err: unknown) => {
    console.error("shannon-gateway failed to start:", err);
    process.exit(1);
  });
}

export { main };

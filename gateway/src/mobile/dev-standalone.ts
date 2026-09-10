/**
 * Dev-only standalone mobile host: boots the REAL gateway mobile server
 * (pairing + engine bridge, `requireSession` enforced) on an ephemeral
 * loopback port with in-memory pair tokens / device registry, and prints one
 * JSON line
 *
 *   {"port": <n>, "token": "<pairToken>", "expiresAt": <epochMs>}
 *
 * so a client smoke test (shannon-mobile `tool/gateway_smoke.dart`) can
 * exercise the live `shannon/*` wire contract without the desktop app or a
 * running engine — queries fail with ENGINE_ERROR (engine connect refused),
 * which is exactly the error-path contract a phone must handle.
 *
 * Exits on stdin EOF or SIGINT.
 */
import { createConsoleLogger } from "../logger.js";
import { GATEWAY_VERSION } from "../version.js";
import {
  createMobileHandlers,
  DeviceRegistry,
  PairTokenStore,
} from "./pairing.js";
import { MobileServer } from "./server.js";

const logger = createConsoleLogger("warn");
const tokens = new PairTokenStore();
const registry = new DeviceRegistry();
const handlers = createMobileHandlers({
  engine: {
    engineWsUrl: "ws://127.0.0.1:33420/api/ws",
    engineHttpBaseUrl: "http://127.0.0.1:33420",
    defaultModel: "claude-sonnet-4-6",
    version: GATEWAY_VERSION,
    logger,
  },
  tokens,
  registry,
  logger,
});

const server = new MobileServer({
  host: "127.0.0.1",
  port: 0,
  logger,
  handlers,
  servePage: false,
});
const handle = await server.start();
const record = tokens.issue();
process.stdout.write(
  `${JSON.stringify({
    port: handle.port,
    token: record.token,
    expiresAt: record.expiresAt,
  })}\n`,
);

let stopping = false;
const shutdown = (): void => {
  if (stopping) return;
  stopping = true;
  void server
    .stop()
    .catch(() => {})
    .then(() => process.exit(0));
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);
process.stdin.on("end", shutdown);

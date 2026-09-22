import { homedir } from "node:os";
import { writeFileSync, mkdirSync, chmodSync } from "node:fs";
import { dirname, join } from "node:path";

import { type AdapterContext, type ChannelAdapter, type Logger } from "./adapters/types.js";
import { AdapterRegistry } from "./adapters/registry.js";
import { EngineWsClient } from "./engine/wsClient.js";
import { SessionRouter, type EngineClientFactory } from "./router/router.js";
import { type TurnHandler } from "./router/types.js";
import { createApprovalTurnHandler } from "./router/approvalTurnHandler.js";
import { type AdapterConfig, type GatewayConfig, type LogLevel } from "./config/types.js";
import { type SecretProvider } from "./secrets/types.js";
import { createEnvSecretProvider } from "./secrets/envProvider.js";
import { createCliKeyringProvider } from "./secrets/cliKeyring.js";
import { createChainedSecretProvider } from "./secrets/chain.js";
import { createConsoleLogger } from "./logger.js";
import { GATEWAY_VERSION } from "./version.js";
import { MobileServer } from "./mobile/server.js";
import { ensureTlsMaterial } from "./mobile/mobileTls.js";
import { advertiseMobileServer, type MdnsHandle } from "./mobile/mdns.js";
import {
  createMobileHandlers,
  DeviceRegistry,
  PairTokenStore,
} from "./mobile/pairing.js";
import type { EngineClientFactory as MobileEngineClientFactory } from "./mobile/engineBridge.js";
import { MobileDispatchHub } from "./mobile/hub.js";
import { createMobileChannelAdapter } from "./mobile/channel.js";
import { createTaskHandlers } from "./mobile/taskHandlers.js";
import {
  deriveRelayAuthTag,
  deriveSessionKey,
  generateHostE2EKeyPair,
} from "./mobile/relay/e2e.js";
import { startRelayHost, type RelayHostHandle } from "./mobile/relay/relayHost.js";
import { generateQrV2Payload, generateRelaySessionId } from "./mobile/relay/qrV2.js";
import { evaluateTrigger, resolveTriggerConfig } from "./router/trigger.js";
import { withTaskLifecycle } from "./router/lifecycle.js";
import { AllowlistGuard, type InboundGuard } from "./access/guard.js";
import { Allowlist } from "./access/allowlist.js";
import { PairingStore } from "./access/pairing.js";

/**
 * Turns an `AdapterConfig` + secret-backed `AdapterContext` into a live
 * `ChannelAdapter`. Each platform registers one factory; the bootstrap looks it
 * up by `config.platform`. Factories are injectable so the entry point can
 * assemble the real platform map while tests pass a mock.
 */
export type AdapterFactory = (
  cfg: AdapterConfig,
  ctx: AdapterContext,
) => ChannelAdapter | Promise<ChannelAdapter>;

export interface BootstrapOptions {
  /** platform id → factory. Real adapters register here (Slack in P1-g, others in T6). */
  factories: Map<string, AdapterFactory>;
  /** Override the engine WS client factory (tests inject a mock). */
  engineClientFactory?: EngineClientFactory;
  /** Override the turn handler (default: approval-aware, posting to engine.httpBaseUrl). */
  turnHandler?: TurnHandler;
  /** Override the secret provider (default: env → OS keyring chain). */
  secretProvider?: SecretProvider;
  /** Override the logger (default: console, level from config.logLevel). */
  logger?: Logger;
  /**
   * Mobile server test seams (only used when `config.mobile.enabled`). The
   * engine-client factory lets tests stub the query stream; `fetchImpl` stubs
   * the engine health/approval HTTP calls. Both optional in production.
   */
  mobileEngineClientFactory?: MobileEngineClientFactory;
  mobileFetchImpl?: typeof fetch;
  /**
   * Access-control seam (review §P0-7). Production defaults to an
   * `AllowlistGuard` with empty in-memory state — meaning any IM sender
   * that has not been paired/allowlisted receives a pairing challenge on
   * DM and is denied in group chats. Tests inject a mock guard.
   */
  accessGuard?: InboundGuard;
}

export interface BootstrapHandle {
  /** Stop adapters + router lanes + mobile server. Idempotent. */
  stop(): Promise<void>;
  /** Number of adapters that started (diagnostics / tests). */
  readonly adapterCount: number;
  /** Bound mobile server port, or `null` when the mobile server is disabled. */
  readonly mobilePort: number | null;
}

/**
 * Wire the four layers from a `GatewayConfig`:
 *
 * 1. build the secret-backed `AdapterContext` (env → OS keyring chain),
 * 2. instantiate each enabled adapter from `factories` and register it,
 * 3. build a per-session `EngineWsClient` factory (each lane pins its own
 *    connection + `session_id`, consuming the engine's P0-d/e persistence),
 * 4. wire `adapter.onMessage → trigger gate → router.handleInbound` (P1-4:
 *    DMs answer directly; group chats need @mention or `/shannon`),
 * 5. `startAll` the adapters.
 *
 * The default engine client factory only *constructs* `EngineWsClient`s; the
 * lane connects them lazily on the first turn, so `bootstrap()` itself opens no
 * socket and won't fail if the engine happens to be down at startup.
 */
export async function bootstrap(
  config: GatewayConfig,
  opts: BootstrapOptions,
): Promise<BootstrapHandle> {
  const logger = opts.logger ?? createConsoleLogger((config.logLevel ?? "info") as LogLevel);
  const secretProvider =
    opts.secretProvider ??
    createChainedSecretProvider([createEnvSecretProvider(), createCliKeyringProvider()]);

  const ctx: AdapterContext = {
    logger,
    getSecret: (key: string) => secretProvider.get(key),
  };

  const registry = new AdapterRegistry();
  for (const cfg of config.adapters) {
    if (!cfg.enabled) {
      logger.info(`adapter "${cfg.platform}" disabled; skipping`);
      continue;
    }
    const factory = opts.factories.get(cfg.platform);
    if (!factory) {
      throw new Error(`no adapter factory registered for platform "${cfg.platform}"`);
    }
    const adapter = await factory(cfg, ctx);
    registry.register(adapter);
  }

  // Engine api_server bearer (F14): config names the secret entry, the raw
  // token resolves from the keyring/env once at boot and stays in memory.
  const engineAuthToken = config.engine.authTokenKey
    ? await secretProvider.get(config.engine.authTokenKey)
    : null;
  if (config.engine.authTokenKey && !engineAuthToken) {
    logger.warn(
      `engine.authTokenKey "${config.engine.authTokenKey}" resolved to no secret — ` +
        "engine calls may 401 if the engine enforces its bearer",
    );
  }

  // P2-1 mobile dispatch: when the mobile channel is enabled, the paired-phone
  // channel becomes a first-class platform adapter ("mobile") so dispatched
  // tasks ride the same lane/approval/lifecycle pipeline as the IM adapters.
  const dispatchHub = config.mobile?.enabled
    ? new MobileDispatchHub({ logger })
    : null;
  if (dispatchHub) {
    registry.register(createMobileChannelAdapter({ hub: dispatchHub }));
  }

  const clientFactory: EngineClientFactory =
    opts.engineClientFactory ?? ((sessionKey: string) => createEngineClient(config, sessionKey, engineAuthToken));

  // P1-4: report 任务开始/完成/失败 back to the IM channel around every
  // adapter-routed turn (opt-out via config.im.taskLifecycle = false). The
  // mobile shannon/* path keeps its own engine bridge and is unaffected.
  const baseTurnHandler: TurnHandler =
    opts.turnHandler ??
    createApprovalTurnHandler({
      engineBaseUrl: config.engine.httpBaseUrl,
      // review §P1-13: forward the engine bearer to the approval POST so IM
      // approvals reach a non-loopback-bound engine without 401ing.
      authToken: engineAuthToken,
    });
  const turnHandler: TurnHandler =
    config.im?.taskLifecycle === false ? baseTurnHandler : withTaskLifecycle(baseTurnHandler);

  const router = new SessionRouter({ registry, clientFactory, turnHandler, logger });

  // P2-1: dispatched tasks enter the router here — the same trigger-free,
  // lane-serialized, lifecycle-wrapped pipeline the IM adapters feed.
  dispatchHub?.setSubmit((inbound) => router.handleInbound(inbound));

  // Inbound → access guard (review §P0-7) → trigger gate (P1-4) → router.
  // The guard ensures only paired/allowlisted senders can drive the engine;
  // unpaired DMs receive a pairing challenge, group mentions by strangers
  // are denied with a hint. Access control was previously implemented but
  // never wired in — fix closes that gap so strangers cannot drive tools.
  const accessGuard = opts.accessGuard ?? new AllowlistGuard(new Allowlist(), new PairingStore());
  const triggerByPlatform = new Map(
    config.adapters.map((cfg) => [cfg.platform, resolveTriggerConfig(cfg.options)]),
  );
  for (const adapter of registry.all()) {
    const triggerCfg = triggerByPlatform.get(adapter.platform) ?? {};
    adapter.onMessage((m) => {
      void (async () => {
        // 1) access guard: deny/challenge before any further work.
        const decision = await accessGuard.check(m);
        if (decision.decision !== "allow") {
          const replyTarget = {
            platform: m.platform,
            chatId: m.chatId,
            threadId: m.threadId,
          };
          if (decision.decision === "challenge") {
            logger.info(`pairing challenge issued for ${m.platform}:${m.senderId}`);
            void adapter.send(
              replyTarget,
              `Pairing required — approve code ${decision.code} in the Shannon desktop app. (expires in 5 min)`,
            );
          } else {
            logger.info(`denied inbound from ${m.platform}:${m.senderId} (not paired)`);
            void adapter.send(replyTarget, decision.reason);
          }
          return;
        }
        // 2) trigger gate: DMs reply directly; group chats need @mention or /shannon.
        const verdict = evaluateTrigger(m, triggerCfg);
        if (!verdict.triggered) {
          logger.debug(
            `inbound on ${m.platform}:${m.chatId} ignored by trigger policy (${verdict.via})`,
          );
          return;
        }
        const routed = verdict.text === m.text ? m : { ...m, text: verdict.text };
        void router.handleInbound(routed);
      })();
    });
  }

  await registry.startAll(ctx);
  logger.info(`shannon-gateway up: ${registry.size} adapter(s) started`);

  const mobile = config.mobile?.enabled
    ? await startMobileServer(config, logger, opts, dispatchHub!, engineAuthToken)
    : null;
  if (mobile) {
    logger.info(
      `mobile shannon/* server listening on ${config.mobile?.host ?? "0.0.0.0"}:${mobile.port}`,
    );
  }

  let stopped = false;
  return {
    get adapterCount(): number {
      return registry.size;
    },
    get mobilePort(): number | null {
      return mobile?.port ?? null;
    },
    async stop(): Promise<void> {
      if (stopped) return;
      stopped = true;
      await mobile?.handle.stop().catch(() => {});
      await router.stop();
      await registry.stopAll();
      logger.info("shannon-gateway stopped");
    },
  };
}

/**
 * Start the inbound mobile `shannon/*` server (Option B adapter, P1.1–P1.3).
 *
 * Design D control channel: pairing state is shared via files — the desktop
 * appends one-time tokens to `tokensFile` (consumed here on `shannon/pair`) and
 * both processes read/write the device registry at `devicesFile`. The engine
 * bridge enforces `requireSession` (query/cancel/approval are gated behind a
 * paired device) and mandates an Ed25519 signature on every approval decision.
 *
 * P2-1: the same server also serves the built-in PWA page on GET / and hands
 * every accepted connection to the dispatch hub, so `shannon/task.dispatch`
 * can route texts through the IM pipeline and lifecycle stamps / approval
 * requests can be pushed back to the paired phone. The same handlers serve the
 * relay-E2E transport.
 */
async function startMobileServer(
  config: GatewayConfig,
  logger: Logger,
  opts: BootstrapOptions,
  dispatchHub: MobileDispatchHub,
  engineAuthToken: string | null,
): Promise<{ handle: { stop(): Promise<void> }; port: number }> {
  const mobileCfg = config.mobile!;
  const host = mobileCfg.host ?? "0.0.0.0";
  const port = mobileCfg.port ?? 33430;
  const tokensFile = mobileCfg.tokensFile ?? join(homedir(), ".shannon", "mobile-pair-tokens.jsonl");
  const devicesFile = mobileCfg.devicesFile ?? join(homedir(), ".shannon", "mobile-devices.json");

  const tokens = new PairTokenStore({ filePath: tokensFile });
  const registry = new DeviceRegistry({ filePath: devicesFile });
  // v0.12 LAN hardening: TLS with the persisted self-signed cert. Phones pin
  // the cert fingerprint carried in the QR (out-of-band trust, same model as
  // the pair token). Fail loud on cert trouble — silent plaintext fallback
  // would defeat the hardening.
  const tlsEnabled = mobileCfg.tls?.enabled === true;
  const tlsMaterial = tlsEnabled ? ensureTlsMaterial() : null;
  if (tlsMaterial) {
    logger.info(
      `mobile TLS enabled — cert fingerprint ${tlsMaterial.fingerprint} ` +
        `(phones pin it from the QR)`,
    );
  }
  const handlers = createMobileHandlers({
    engine: {
      engineWsUrl: config.engine.wsUrl,
      engineHttpBaseUrl: config.engine.httpBaseUrl,
      defaultModel: config.engine.model ?? null,
      version: GATEWAY_VERSION,
      logger,
      engineClientFactory: opts.mobileEngineClientFactory,
      fetchImpl: opts.mobileFetchImpl,
      engineAuthToken,
    },
    tokens,
    registry,
    logger,
    tasks: createTaskHandlers({
      hub: dispatchHub,
      // review §P1-13: revoked devices must not be able to dispatch tasks
      // through their still-open WS connection. Bootstrap closes the loop
      // so a device removed from devices.json loses all gateways —
      // shannon/* RPC (engineBridge) and shannon/task.* alike.
      isDeviceTrusted: (deviceId: string) => registry.has(deviceId),
    }),
  });

  const server = new MobileServer({
    host,
    port,
    logger,
    handlers,
    onContext: (ctx) => dispatchHub.registerConnection(ctx),
    ...(tlsMaterial ? { tls: { key: tlsMaterial.key, cert: tlsMaterial.cert } } : {}),
  });
  const handle = await server.start();

  // §A8/§A8b (cross-repo-adaptation-spec): advertise _shannon._tcp while the
  // pairing server is up. iOS ATS rejects raw-IP ws:// endpoints outright, so
  // LAN direct-connect requires the phone to reach a .local hostname — this
  // advertisement is that scenario's hard prerequisite. Loopback-only binds
  // cannot serve phones anyway, so skip (and flag) instead of advertising an
  // unreachable endpoint — also keeps test boots (127.0.0.1) mDNS-free.
  let mdns: MdnsHandle | null = null;
  if (host === "127.0.0.1" || host === "localhost" || host === "::1") {
    logger.warn(
      `mobile server bound to loopback (${host}) — LAN direct-connect is ` +
        "unreachable from phones; set mobile.host to 0.0.0.0",
    );
  } else {
    mdns = advertiseMobileServer({ port: handle.port, version: GATEWAY_VERSION, logger });
  }
  const stopServerAndMdns = async (): Promise<void> => {
    await mdns?.stop().catch(() => {});
    await handle.stop();
  };

  // Relay host mode: also connect outbound to shannon-relay so phones can
  // pair without LAN access. The same MethodHandlers are reused — the relay
  // host wraps messages in E2E encryption over the relay's binary transport.
  if (mobileCfg.relay?.enabled && mobileCfg.relay.url) {
    const relayUrl = mobileCfg.relay.url;
    const relaySid = generateRelaySessionId();
    const pairRecord = tokens.issue();
    // Legacy (token-only) key — kept as the fallback for pre-v0.3 phones.
    const sessionKey = deriveSessionKey(pairRecord.token);
    // v0.3: per-session ephemeral X25519 keypair (forward secrecy) — the pub
    // travels in the QR, the handshake completes via the phone's `e2e_hello`.
    const hostE2E = generateHostE2EKeyPair();
    // Relay handshake auth tag (pins the session to this pairing secret).
    const relayAuthTag = deriveRelayAuthTag(pairRecord.token);

    const relayHandle: RelayHostHandle = startRelayHost({
      relayUrl,
      sid: relaySid,
      sessionKey,
      handlers,
      logger,
      relayAuthTag,
      hostE2E: { privateKey: hostE2E.privateKey, pairToken: pairRecord.token },
      onContext: (ctx) => dispatchHub.registerConnection(ctx),
    });
    // Nobody awaits `paired` — without a handler here the 75s pair timeout
    // rejection would surface as an unhandled rejection and take the
    // gateway down (Node's default is to throw). A phone that never joins is
    // a normal condition, not a crash.
    relayHandle.paired.catch((err: Error) => {
      logger.info(`relay host: initial pair window closed (${err.message}); ` +
        "host stays registered for late joins via auto-reconnect/re-pair");
    });

    // Generate the QR v2 payload for the phone to scan. The LAN-endpoint
    // scheme reflects TLS, not the relay scheme: with mobile.tls the phone
    // dials wss and pins the cert fingerprint from this payload.
    const scheme = tlsMaterial ? "wss" : relayUrl.startsWith("wss") ? "wss" : "ws";
    const qrPayload = generateQrV2Payload({
      scheme,
      host,
      port: handle.port,
      pairToken: pairRecord.token,
      expiresAt: pairRecord.expiresAt,
      relayUrl,
      relaySessionId: relaySid,
      hostE2EPubKey: hostE2E.pubB64,
      certFingerprint: tlsMaterial?.fingerprint ?? null,
    });

    const qrJson = JSON.stringify(qrPayload);
    // The payload embeds the pair token, which IS the E2E session key material
    // (e2e.ts): anyone who reads it can decrypt/forge the whole relay session.
    // Never log it at info — it's available via the 0600 payload file (rendered
    // by the desktop) or by enabling debug logs.
    logger.info(
      `relay host: session ${relaySid} at ${relayUrl} — pair token valid for ` +
        `${Math.max(0, pairRecord.expiresAt - Date.now())}ms; QR available via ` +
        `${mobileCfg.qrPayloadFile ?? "debug logging (mobile.qrPayloadFile unset)"}`,
    );

    if (mobileCfg.qrPayloadFile) {
      mkdirSync(dirname(mobileCfg.qrPayloadFile), { recursive: true });
      writeFileSync(mobileCfg.qrPayloadFile, qrJson, { encoding: "utf8", mode: 0o600 });
      chmodSync(mobileCfg.qrPayloadFile, 0o600);
      logger.info(`relay host: QR payload written to ${mobileCfg.qrPayloadFile} (0600)`);
    } else {
      logger.debug(`relay host: QR v2 payload: ${qrJson}`);
    }

    // Extend the stop handle to also stop the relay host (and the mDNS
    // advertisement, via stopServerAndMdns).
    return {
      handle: {
        stop: async () => {
          await relayHandle.stop().catch(() => {});
          await stopServerAndMdns();
        },
      },
      port: handle.port,
    };
  }

  return { handle: { stop: stopServerAndMdns }, port: handle.port };
}

function createEngineClient(
  config: GatewayConfig,
  sessionKey: string,
  authToken: string | null,
): EngineWsClient {
  return new EngineWsClient({
    url: config.engine.wsUrl,
    model: config.engine.model ?? null,
    sessionId: sessionKey,
    headers: authToken ? { authorization: `Bearer ${authToken}` } : undefined,
  });
}

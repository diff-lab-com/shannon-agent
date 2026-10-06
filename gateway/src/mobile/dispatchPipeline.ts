/**
 * The mobile-only dispatch pipeline — the §K "paired phone as a first-class
 * channel" composition, extracted so every host that serves the task face
 * assembles it exactly ONE way (this is the same wiring the live bootstrap
 * builds inline for platform "mobile", minus the IM-adapter concerns):
 *
 *   hub.dispatch → SessionRouter lane (serial per device, stable session key)
 *                → §K3 mobile task turn handler → engine
 *                → structured task stream (query.started / task.progress /
 *                  task.message / query.failed, session_id = task id)
 *                  pushed back to the initiating phone through the hub.
 *
 * Real components only — `MobileDispatchHub` (injected, shared with the
 * `shannon/task.*` handlers and the server's `onContext` binder), the mobile
 * channel adapter, the production `SessionRouter`/lane machinery and the
 * §K3 task turn handler. There is deliberately NO second, dev-only event
 * semantics: a dispatched turn runs the identical code path it runs in the
 * live gateway.
 *
 * Deliberately NOT part of this factory (the bootstrap layers them around the
 * full multi-platform router): IM adapters, the access guard, the trigger
 * gate and the IM task-lifecycle reporter. Dispatch enters via
 * `hub.dispatch → submit` directly, exactly as it does in the bootstrap.
 *
 * Consumers:
 *  - `dev-standalone.ts` — the dev host mounts the §K task face with this
 *    factory (the full bootstrap's router/lanes need config + IM adapters;
 *    the mobile platform is this host's only tenant).
 *  - tests — the seam-routed `engineClientFactory` keeps the pipeline
 *    unit-testable without a live engine.
 */

import { type Logger } from "../adapters/types.js";
import { AdapterRegistry } from "../adapters/registry.js";
import { EngineWsClient } from "../engine/wsClient.js";
import { SessionRouter } from "../router/router.js";
import { type ActiveQueryRegistry } from "../router/activeQueries.js";
import type { InboundSubmit, MobileDispatchHub } from "./hub.js";
import { createMobileChannelAdapter } from "./channel.js";
import { createMobileTaskTurnHandler } from "./taskTurnHandler.js";

export interface MobileDispatchPipelineOptions {
  /** The dispatch hub — shared with the task handlers and the server wiring. */
  hub: MobileDispatchHub;
  /** Engine WS URL, e.g. `ws://127.0.0.1:33420/api/ws`. */
  engineWsUrl: string;
  /** Engine HTTP base URL (the turn handler's approval POST target). */
  engineHttpBaseUrl: string;
  /** Default model for lanes that don't override one. */
  defaultModel?: string | null;
  logger: Logger;
  /** Engine bearer token, forwarded to the approval POST (review §P1-13). */
  engineAuthToken?: string | null;
  /** Override for tests; defaults to the global fetch (approval POST). */
  fetchImpl?: typeof fetch;
  /**
   * Test seam: build the per-lane engine client (the argument is the router
   * session key, e.g. `mobile:<deviceId>`). Defaults to a real
   * `EngineWsClient` pinned to that session key — same construction the
   * bootstrap's `createEngineClient` performs.
   */
  engineClientFactory?: (sessionKey: string) => EngineWsClient;
  /**
   * Shared in-flight query registry — the SAME instance the `shannon/*`
   * handlers' engine bridge holds (the composer creates it once, mirroring
   * the approval-registry injection). When set, each turn registers its lane
   * client under the router session key (`mobile:<deviceId>`) so
   * `shannon/cancel` can interrupt a dispatched task's engine turn.
   */
  activeQueries?: ActiveQueryRegistry;
}

export interface MobileDispatchPipeline {
  /** Late-bind into `MobileDispatchHub.setSubmit`. */
  submit: InboundSubmit;
  /** Close every lane's engine client (call on host shutdown). */
  stop(): Promise<void>;
}

export function createMobileDispatchPipeline(
  opts: MobileDispatchPipelineOptions,
): MobileDispatchPipeline {
  // Platform "mobile" is the only tenant: the adapter delegates delivery to
  // the hub (send = out-of-band push, requestApproval = parked lane), so no
  // platform transport is started or stopped here.
  const registry = new AdapterRegistry();
  registry.register(createMobileChannelAdapter({ hub: opts.hub }));

  const turnHandler = createMobileTaskTurnHandler({
    hub: opts.hub,
    engineBaseUrl: opts.engineHttpBaseUrl,
    authToken: opts.engineAuthToken ?? null,
    fetchImpl: opts.fetchImpl,
  });

  const router = new SessionRouter({
    registry,
    clientFactory:
      opts.engineClientFactory ??
      ((sessionKey: string) =>
        new EngineWsClient({
          url: opts.engineWsUrl,
          model: opts.defaultModel ?? null,
          sessionId: sessionKey,
          headers: opts.engineAuthToken
            ? { authorization: `Bearer ${opts.engineAuthToken}` }
            : undefined,
        })),
    turnHandler,
    logger: opts.logger,
    activeQueries: opts.activeQueries,
  });

  return {
    submit: (inbound) => router.handleInbound(inbound),
    stop: () => router.stop(),
  };
}

/**
 * P1.1b engine bridge: binds the `shannon/*` method surface (P1.1a) to the real
 * Shannon engine. The MobileServer owns transport; this module owns semantics.
 *
 *   shannon/query           → engine WS Query (streaming EngineEvent → ShannonEvent)
 *   shannon/cancel          → engine WS cancel on the in-flight client
 *   shannon/approval/decide → engine HTTP POST /api/approval/respond
 *   shannon/health          → probe engine HTTP liveness
 *   shannon/model.list      → configured/switched default (minimal; real discovery later)
 *   shannon/model.switch    → override default model for subsequent queries
 *   shannon/agent.list      → [] stub (P1.x wires real session enumeration)
 *   shannon/agent.detail    → NOT_IMPLEMENTED (session-watch is a later phase)
 *   shannon/pair            → NOT_IMPLEMENTED (P1.2: Ed25519 pairing + OS keyring)
 *   shannon/device.resume   → NOT_IMPLEMENTED (P1.2)
 *
 * The phone is a first-class streaming client (architecture doc §5.2): the full
 * engine event stream is forwarded as `shannon/event` notifications, mapped to
 * the mobile-friendly ShannonEvent union by `mapEngineEvent`.
 *
 * Active-query tracking: one EngineClient per query, keyed by session_id (or
 * "__anon__" when none) so `shannon/cancel` can find and interrupt it. P1.2's
 * device pairing replaces the anon key with a stable device session id, and
 * verifies the Ed25519 signature on every approval decision.
 */

import { respondToApproval, type GatewayApprovalChoice } from "../engine/httpClient.js";
import { EngineWsClient, type EngineWsClientOptions } from "../engine/wsClient.js";
import type { EngineEvent } from "../engine/runtime.js";
import type { Logger } from "../adapters/types.js";
import { approvalMessage } from "./crypto.js";
import {
  ShannonError,
  type AgentListResult,
  type ApprovalDecideParams,
  type CancelParams,
  type HealthResult,
  type ModelListResult,
  type OkResult,
  type QueryParams,
  type ShannonEvent,
} from "./protocol.js";
import type { MethodContext, MethodHandlers } from "./server.js";

/**
 * The engine-client surface this bridge consumes. `EngineWsClient` satisfies it;
 * tests pass a fake. Parameterized so the bridge never imports a concrete socket
 * implementation except as the default factory.
 */
export interface EngineClient {
  connect(): Promise<void>;
  runQuery(
    prompt: string,
    opts?: { model?: string | null; sessionId?: string | null },
  ): AsyncIterable<EngineEvent>;
  cancel(): void;
  close(): Promise<void>;
}

export type EngineClientFactory = (opts: EngineWsClientOptions) => EngineClient;

/**
 * Verifies a device signature. Backed by the DeviceRegistry in production (P1.2
 * composer wires it via `createRegistryVerifier`); tests inject a fake. Kept as a
 * plain callback so the bridge has no compile-time dependency on the pairing module.
 */
export type DeviceSignatureVerifier = (
  deviceId: string,
  message: string,
  signature: string,
) => boolean;

export interface EngineBridgeOptions {
  /** Engine WS URL, e.g. `ws://127.0.0.1:33420/api/ws`. */
  engineWsUrl: string;
  /** Engine HTTP base URL, e.g. `http://127.0.0.1:33420` (approval POST + health). */
  engineHttpBaseUrl: string;
  /** Default model when neither the request nor a `model.switch` override sets one. */
  defaultModel?: string | null;
  /** Gateway version, surfaced in `shannon/health`. */
  version: string;
  logger: Logger;
  /** Test seam: override EngineClient construction. */
  engineClientFactory?: EngineClientFactory;
  /** Test seam for the health probe + approval POST (defaults to global fetch). */
  fetchImpl?: typeof fetch;
  /**
   * P1.2 access gate: when true, query/cancel/approval require a bound device
   * session (ctx.sessionId set by shannon/pair or shannon/device.resume) and
   * every approval decision must carry a valid Ed25519 signature. Default false
   * so P1.1b's open-by-default tests stay green; the live gateway enables it.
   */
  requireSession?: boolean;
  /** Registry-backed signature verifier; required for approval signing when requireSession is on. */
  verifyDeviceSignature?: DeviceSignatureVerifier;
  /**
   * WP-15 T3: is this bound device still in the registry? Checked on every
   * gated RPC so an out-of-band revoke (desktop UI or `shannon/device.revoke`)
   * takes effect immediately — the revoked phone gets PAIRING_REQUIRED on its
   * next call instead of riding a stale bound session forever.
   */
  isDeviceTrusted?: (deviceId: string) => boolean;
  /**
   * Bearer token for engine calls (WS handshake + HTTP), resolved from the
   * secret provider at bootstrap. The engine api_server enforces an optional
   * bearer on non-loopback binds; without this the gateway's engine calls
   * would 401 the moment that auth is enabled. Null/absent = engine runs
   * unauthenticated (loopback default).
   */
  engineAuthToken?: string | null;
}

/** Sentinel key for queries without a session_id (P1.2 replaces it with a device id). */
const ANON_KEY = "__anon__";

/**
 * Build the `MethodHandlers` map that wires `shannon/*` to the engine. Stateful
 * (tracks in-flight queries for cancel + the model.switch override) but stateless
 * across restarts — P1.2 adds persistent device/session binding.
 */
export function createEngineHandlers(opts: EngineBridgeOptions): MethodHandlers {
  const factory: EngineClientFactory =
    opts.engineClientFactory ?? ((o) => new EngineWsClient(o));
  const fetchImpl = opts.fetchImpl ?? fetch;

  /** session key → in-flight client (for cancel). One query per key at a time. */
  const activeQueries = new Map<string, EngineClient>();
  let modelOverride: string | null = null;

  const requireSession = opts.requireSession === true;
  const engineAuthToken = opts.engineAuthToken ?? null;
  /** Authorization headers for engine HTTP calls (empty when no token set). */
  const engineAuthHeaders = (): Record<string, string> =>
    engineAuthToken ? { authorization: `Bearer ${engineAuthToken}` } : {};
  /** P1.2 gate: null = proceed; otherwise return this error outcome. */
  const sessionGate = (
    ctx: MethodContext,
  ): { kind: "error"; code: number; message: string } | null => {
    if (!requireSession) return null;
    if (ctx.sessionId == null) {
      return {
        kind: "error",
        code: ShannonError.PAIRING_REQUIRED,
        message: "pair a device first (shannon/pair or shannon/device.resume)",
      };
    }
    // WP-15 T3: a bound session whose device was revoked mid-flight is no
    // longer trusted — every gated RPC re-checks the registry.
    if (opts.isDeviceTrusted && !opts.isDeviceTrusted(ctx.sessionId)) {
      return {
        kind: "error",
        code: ShannonError.PAIRING_REQUIRED,
        message: "device revoked — pair again (shannon/pair)",
      };
    }
    return null;
  };

  return {
    // ── streaming query ───────────────────────────────────────────────────
    "shannon/query": async (raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<QueryParams>;
      if (typeof params.prompt !== "string" || params.prompt.trim().length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.prompt (non-empty string) is required",
        };
      }
      // Ownership: in gated mode the turn runs under the CALLER's device
      // session. A caller-supplied session_id can't target another device's
      // in-flight query (steer/cancel hijack) — mismatch is ignored with a
      // warning. Open mode (dev/test) keeps the caller-controlled behavior.
      let sessionId = params.session_id ?? null;
      if (requireSession) {
        if (sessionId != null && sessionId !== ctx.sessionId) {
          opts.logger.warn(
            `shannon/query: ignoring foreign session_id (caller=${ctx.sessionId})`,
          );
        }
        sessionId = ctx.sessionId;
      }
      const model = params.model ?? modelOverride ?? opts.defaultModel ?? null;
      const key = sessionId ?? ANON_KEY;

      // If a previous query on this key never reached a terminal event (e.g. the
      // phone reopened the socket), tear its client down before starting fresh.
      const prior = activeQueries.get(key);
      if (prior) {
        opts.logger.warn(`shannon/query: replacing in-flight client for key=${key}`);
        activeQueries.delete(key);
        await prior.close().catch(() => {});
      }

      const client = factory({
        url: opts.engineWsUrl,
        model,
        sessionId,
        headers: engineAuthHeaders(),
      });
      try {
        await client.connect();
      } catch (err) {
        return {
          kind: "error",
          code: ShannonError.ENGINE_ERROR,
          message: `engine connect failed: ${(err as Error).message}`,
        };
      }
      activeQueries.set(key, client);

      const turnId = crypto.randomUUID();
      const stream = (async function* (): AsyncGenerator<ShannonEvent> {
        yield { type: "query.started", turn_id: turnId };
        try {
          for await (const ev of client.runQuery(params.prompt as string, { model, sessionId })) {
            const mapped = mapEngineEvent(ev);
            if (mapped) {
              // WP-15 P2-8: stamp the routing key on every progress frame so
              // clients can correlate without the one-in-flight-turn-per-socket
              // convention. Additive — clients that ignore `turn_id` are
              // unaffected.
              if (mapped.type === "task.progress") mapped.turn_id = turnId;
              yield mapped;
            }
          }
        } finally {
          activeQueries.delete(key);
          await client.close().catch(() => {});
        }
      })();

      return { kind: "stream", stream, result: { ok: true } satisfies OkResult };
    },

    // ── cancel ────────────────────────────────────────────────────────────
    "shannon/cancel": async (raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<CancelParams>;
      // Ownership (mirror of query): a bound device may only cancel its own
      // in-flight turn — params.session_id can't reach another device's query.
      const key = requireSession ? ctx.sessionId! : params.session_id ?? ANON_KEY;
      const client = activeQueries.get(key);
      if (client) {
        client.cancel();
      } else {
        // Idempotent: a cancel for nothing-in-flight is a no-op success, matching
        // the engine's own cancel semantics.
        opts.logger.info(`shannon/cancel: no in-flight query for key=${key} (no-op)`);
      }
      return { kind: "result", result: { ok: true } satisfies OkResult };
    },

    // ── approval decision ─────────────────────────────────────────────────
    "shannon/approval/decide": async (raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<ApprovalDecideParams>;
      if (typeof params.request_id !== "string" || params.request_id.length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.request_id is required",
        };
      }
      if (params.choice !== "allow" && params.choice !== "deny") {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: 'params.choice must be "allow" or "deny"',
        };
      }
      // P1.2: every approval decision MUST be signed by the bound device over
      // `${request_id}:${choice}`. Unsigned or invalid signatures are rejected
      // before the engine is touched, so a stolen/ungated connection can't
      // auto-approve a destructive tool. The wire is still untrusted (the
      // params cast is unchecked), so the runtime check stays defensive even
      // though `ApprovalDecideParams.signature` is now a required field.
      if (requireSession) {
        const sig = typeof params.signature === "string" ? params.signature : "";
        const deviceId = ctx.sessionId as string;
        const ok =
          sig.length > 0 &&
          (opts.verifyDeviceSignature?.(deviceId, approvalMessage(params.request_id, params.choice), sig) ?? false);
        if (!ok) {
          return {
            kind: "error",
            code: ShannonError.BAD_PARAMS,
            message: "invalid or missing approval signature",
          };
        }
      } else if (!params.signature) {
        // requireSession off (e.g. P1.1b open mode) — still warn so the gap is visible.
        opts.logger.warn("shannon/approval/decide: missing signature (requireSession is off)");
      }
      try {
        await respondToApproval({
          engineBaseUrl: opts.engineHttpBaseUrl,
          requestId: params.request_id,
          choice: params.choice as GatewayApprovalChoice,
          authToken: engineAuthToken,
          fetchImpl,
        });
      } catch (err) {
        return {
          kind: "error",
          code: ShannonError.ENGINE_ERROR,
          message: (err as Error).message,
        };
      }
      return { kind: "result", result: { ok: true } satisfies OkResult };
    },

    // ── health ────────────────────────────────────────────────────────────
    "shannon/health": async () => {
      const engine = await probeEngineHttp(opts.engineHttpBaseUrl, fetchImpl, engineAuthHeaders());
      return {
        kind: "result",
        result: { gateway: "ok", engine, version: opts.version } satisfies HealthResult,
      };
    },

    // ── models ────────────────────────────────────────────────────────────
    // WP-15 T5: proxy the engine's /api/models catalog (full directory with
    // display names) so the phone's model picker offers everything. Falls
    // back to the configured/switched model when the engine is unreachable —
    // the picker stays usable offline. Gated: model.list/switch mutate or
    // reflect gateway-wide engine state, so they require a paired session.
    "shannon/model.list": async (_raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const current = modelOverride ?? opts.defaultModel ?? null;
      const fallback = {
        models: (current ? [{ id: current }] : []) as ModelListResult["models"],
        current,
      } satisfies ModelListResult;
      try {
        const res = await fetchImpl(`${opts.engineHttpBaseUrl.replace(/\/$/, "")}/api/models`, {
          headers: engineAuthHeaders(),
          signal: AbortSignal.timeout(2000),
        });
        if (!res.ok) return { kind: "result", result: fallback };
        const body = (await res.json()) as {
          models?: Array<{ id?: unknown; name?: unknown }>;
        };
        const models = (body.models ?? [])
          .filter((m): m is { id: string; name?: string } => typeof m.id === "string" && m.id.length > 0)
          .map((m) => ({ id: m.id, label: typeof m.name === "string" && m.name.length > 0 ? m.name : m.id }));
        if (models.length === 0) return { kind: "result", result: fallback };
        return {
          kind: "result",
          result: { models, current } satisfies ModelListResult,
        };
      } catch {
        return { kind: "result", result: fallback };
      }
    },

    "shannon/model.switch": async (raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as { model?: unknown };
      if (typeof params.model !== "string" || params.model.trim().length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.model (non-empty string) is required",
        };
      }
      modelOverride = params.model;
      return { kind: "result", result: { ok: true } satisfies OkResult };
    },

    // ── agents (stub surface; P1.x wires enumeration) ─────────────────────
    "shannon/agent.list": async (_raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      // P1.x: enumerate the host's active sessions from the engine. P1.1b returns
      // an empty roster so the phone UI can ship against a stable shape.
      return { kind: "result", result: { agents: [] } satisfies AgentListResult };
    },

    "shannon/agent.detail": async (_raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      return {
        kind: "error",
        code: ShannonError.NOT_IMPLEMENTED,
        message: "shannon/agent.detail (session watch) is not implemented in P1.1b",
      };
    },

    // ── pairing (P1.2) ────────────────────────────────────────────────────
    "shannon/pair": async () => ({
      kind: "error",
      code: ShannonError.NOT_IMPLEMENTED,
      message: "shannon/pair lands in P1.2 (Ed25519 pairing + OS keyring)",
    }),

    "shannon/device.resume": async () => ({
      kind: "error",
      code: ShannonError.NOT_IMPLEMENTED,
      message: "shannon/device.resume lands in P1.2",
    }),
  };
}

/**
 * Translate one engine WS event into a mobile-facing ShannonEvent. Returns null
 * for events with no phone-facing representation (currently `session_info`, which
 * is metadata-only). Pure + exported so the mapping is unit-testable in isolation
 * and stays the single source of truth for engine→mobile semantics.
 */
export function mapEngineEvent(ev: EngineEvent): ShannonEvent | null {
  switch (ev.type) {
    case "text":
      return { type: "task.progress", content: ev.content };
    case "thinking":
      // WP-15 P0-2: the engine now routes inline `<think>` reasoning out of
      // `text` into this dedicated variant. The phone doesn't render a
      // thinking section yet (its render-side think_filter stays as defense
      // for old engines), so the reasoning is intentionally not forwarded —
      // the important property is that it no longer pollutes `text`.
      return null;
    case "tool_use":
      return {
        type: "task.progress",
        tool: { kind: "use", name: ev.name, input: ev.input },
      };
    case "tool_result":
      return {
        type: "task.progress",
        tool: { kind: "result", name: ev.name, output: ev.output },
      };
    case "usage":
      return {
        type: "task.progress",
        usage: {
          input_tokens: ev.input_tokens,
          output_tokens: ev.output_tokens,
          cost_usd: ev.cost_usd,
        },
      };
    case "completed":
      return { type: "query.completed", model: ev.model };
    case "failed":
      return { type: "query.failed", error: ev.error };
    case "cancelled":
      return { type: "query.cancelled" };
    case "approval_request":
      return {
        type: "approval.request",
        request_id: ev.request_id,
        tool_name: ev.tool_name,
        tool_input: ev.tool_input,
        description: ev.description,
        is_destructive: ev.is_destructive,
        diff_preview: ev.diff_preview ?? null,
      };
    case "session_info":
      // Metadata-only; no mobile-facing event. (Usage/cost for the turn already
      // arrives via the `usage` event, so nothing is lost.)
      return null;
    case "error":
      return { type: "query.failed", error: ev.message };
    default: {
      // Exhaustiveness guard — if EngineEvent gains a variant, this errors at
      // compile time, forcing mapEngineEvent to handle it.
      const _exhaustive: never = ev;
      void _exhaustive;
      return null;
    }
  }
}

/**
 * Liveness probe via HTTP rather than WS: the P0.2 engine gates the WS route
 * behind a bearer token, so an unauthenticated WS handshake would 401 and look
 * "down" even when the engine is healthy. Any HTTP response (200/401/404) means
 * the server is up; only connection refusal or timeout means down.
 */
async function probeEngineHttp(
  baseUrl: string,
  fetchImpl: typeof fetch,
  headers: Record<string, string> = {},
  timeoutMs = 2000,
): Promise<"ok" | "down"> {
  try {
    const res = await fetchImpl(baseUrl, {
      method: "GET",
      headers,
      signal: AbortSignal.timeout(timeoutMs),
    });
    // 5xx means the server reached us but is itself failing; treat as down so the
    // phone surfaces a real degradation rather than a silent "ok".
    return res.status < 500 ? "ok" : "down";
  } catch {
    return "down";
  }
}

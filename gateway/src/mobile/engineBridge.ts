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
 *
 * The registry itself is the SHARED `ActiveQueryRegistry` (injectable via
 * `opts.activeQueries`; defaults to a private instance): the dispatch
 * pipeline's per-lane clients register under their router session key
 * (`mobile:<deviceId>`) for the duration of each turn, so `shannon/cancel`
 * can also interrupt a dispatched task's engine turn — the cancel lookup
 * probes the bare device session id first, then that lane-key alias.
 */

import { respondToApproval, type GatewayApprovalChoice } from "../engine/httpClient.js";
import { EngineWsClient, type EngineWsClientOptions } from "../engine/wsClient.js";
import { type EngineEvent } from "../engine/runtime.js";
import { type Logger } from "../adapters/types.js";
import { deviceLaneKey, ActiveQueryRegistry } from "../router/activeQueries.js";
import { approvalMessage, approvalMessageV2, approvalDecideTimestampWindowMs } from "./crypto.js";
import { approvalWireItem, engineAgent, engineRisk, type ApprovalRegistry } from "./approvalRegistry.js";
import {
  fetchEngineSessionHistory,
  fetchEngineSessions,
  mapSessionSummary,
  mapSessionTranscript,
  type EngineSessionCaller,
} from "./engineSessions.js";
import {
  ShannonError,
  type AgentListResult,
  type ApprovalDecideParams,
  type ApprovalListResult,
  type CancelParams,
  type HealthResult,
  type ModelListResult,
  type OkResult,
  type QueryParams,
  type SessionHistoryParams,
  type SessionListResult,
  type ShannonEvent,
} from "./protocol.js";
import type { HandlerOutcome, MethodContext, MethodHandlers } from "./server.js";

/**
 * The engine-client surface this bridge consumes. `EngineWsClient` satisfies it;
 * tests pass a fake. Parameterized so the bridge never imports a concrete socket
 * implementation except as the default factory.
 *
 * `call` (optional so older test fakes stay valid) is the one-shot
 * request/response surface the §J session RPCs use — `EngineWsClient` implements
 * it; a fake that omits it simply makes the session handlers report the engine
 * surface as unavailable.
 */
export interface EngineClient {
  connect(): Promise<void>;
  runQuery(
    prompt: string,
    opts?: { model?: string | null; sessionId?: string | null },
  ): AsyncIterable<EngineEvent>;
  cancel(): void;
  close(): Promise<void>;
  call?<T>(
    message: unknown,
    match: (frame: unknown) => T | null,
    opts?: { timeoutMs?: number },
  ): Promise<T>;
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
  /**
   * §L2: the process-wide pending-approval registry. When set, every
   * `approval.request` this bridge streams records into it and every
   * successful `shannon/approval/decide` resolves the entry — the restore face
   * (`shannon/approval.list` / snapshot `pendingApprovals`) stays truthful.
   */
  approvalRegistry?: ApprovalRegistry;
  /**
   * §K: notified after a signed `shannon/approval/decide` landed at the
   * engine. The bootstrap wires this to `MobileDispatchHub.settleApproval` so
   * a dispatched task's parked approval lane unblocks on the phone's decision
   * (the Y/N-text settle left the RPC face with §K — this is the only path
   * besides the 300s timeout).
   */
  approvalDecisionSink?: (requestId: string, choice: GatewayApprovalChoice) => void;
  /**
   * r2-w2d: deny-settle every approval the device still has parked in the
   * dispatch hub — wired to `MobileDispatchHub.cancelPendingApprovals`.
   * `shannon/cancel` invokes this with the cancel's device id (the session key)
   * at the same time it cancels the in-flight engine client: the parked
   * approval is the cancelled turn's own gate, and settling it unblocks the
   * approval round-trip NOW so the turn observes the engine's `cancelled`
   * terminal and the task stream's `query.failed` lands immediately instead of
   * after the device answers or the 300s timeout. Absent → cancel keeps its
   * legacy no-settle behavior (a cancel is still delivered; only the terminal
   * timing regresses to the parking window).
   */
  cancelPendingApprovals?: (deviceId: string) => number;
  /**
   * Shared in-flight query registry (see `../router/activeQueries.ts`). When
   * injected, the bridge registers its direct `shannon/query` clients here AND
   * `shannon/cancel` probes the dispatch pipeline's lane entries
   * (`mobile:<deviceId>`) — one instance across both producers, created by the
   * composer. Absent → a private instance (legacy behavior, direct queries
   * only).
   */
  activeQueries?: ActiveQueryRegistry;
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

  /** session key → in-flight client (for cancel). One query per key at a time.
   *  Shared with the dispatch pipeline's lane clients when injected (see
   *  `ActiveQueryRegistry`) so cancel reaches dispatched tasks too. */
  const activeQueries = opts.activeQueries ?? new ActiveQueryRegistry();
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

  /**
   * §J session RPCs run on a throwaway engine connection: connect → one-shot
   * `call` → close. Transport failures (connect refused, timeout) surface as
   * ENGINE_ERROR — distinguishable from an empty roster / unknown session,
   * which are honest successes per the contract.
   */
  const withSessionClient = async (
    run: (call: EngineSessionCaller["call"]) => Promise<HandlerOutcome>,
  ): Promise<HandlerOutcome> => {
    const client = factory({
      url: opts.engineWsUrl,
      model: null,
      sessionId: null,
      headers: engineAuthHeaders(),
    });
    try {
      const call = (client as { call?: EngineSessionCaller["call"] }).call;
      if (typeof call !== "function") {
        return {
          kind: "error",
          code: ShannonError.ENGINE_ERROR,
          message: "engine client does not support one-shot calls (session surface unavailable)",
        };
      }
      await client.connect();
      return await run(call.bind(client) as EngineSessionCaller["call"]);
    } catch (err) {
      return {
        kind: "error",
        code: ShannonError.ENGINE_ERROR,
        message: `engine session call failed: ${(err as Error).message}`,
      };
    } finally {
      await client.close().catch(() => {});
    }
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
              // §L2: this push is a record point — the ask joins the restore
              // face until `shannon/approval/decide` resolves it. The engine's
              // §L1 rich fields (ts/agent/risk) ride the raw event; read them
              // defensively so a lagging generated-type regeneration can't
              // break this bridge (absent fields → honest omission downstream).
              if (mapped.type === "approval.request") {
                const rich = ev as { ts?: unknown; agent?: unknown; risk?: unknown };
                opts.approvalRegistry?.record({
                  requestId: mapped.request_id,
                  toolName: mapped.tool_name,
                  toolInput: mapped.tool_input,
                  description: mapped.description,
                  isDestructive: mapped.is_destructive,
                  diffPreview: mapped.diff_preview,
                  ts:
                    typeof rich.ts === "number" && Number.isFinite(rich.ts)
                      ? rich.ts
                      : Date.now(),
                  agent: engineAgent(rich.agent),
                  risk: engineRisk(rich.risk),
                });
              }
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
      // Two producers share the registry: the bridge's own query clients sit
      // under the bare key (device session id / anon), a dispatched task's
      // lane client sits under `mobile:<deviceId>`. Probe both — direct first,
      // then the lane alias — so cancel interrupts dispatched tasks too.
      const client = activeQueries.get(key) ?? activeQueries.get(deviceLaneKey(key));
      if (client) {
        client.cancel();
        // r2-w2d: the interrupted turn may be parked at its own approval gate —
        // deny-settle this device's parked approvals so the lane's approval
        // round-trip unblocks now and the `cancelled` terminal (→ §K3
        // `query.failed`) is delivered immediately, not after the parked ask
        // times out. Direct `shannon/query` turns never park in the hub, so
        // this is a 0-entry no-op for them (behavior unchanged); a parked
        // approval for a DIFFERENT device is never touched.
        const settled = opts.cancelPendingApprovals?.(key) ?? 0;
        if (settled > 0) {
          opts.logger.info(
            `shannon/cancel: deny-settled ${settled} parked approval(s) for key=${key}`,
          );
        }
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
      // P1.2: every approval decision MUST be signed by the bound device.
      // Unsigned or invalid signatures are rejected before the engine is
      // touched, so a stolen/ungated connection can't auto-approve a
      // destructive tool. The wire is still untrusted (the params cast is
      // unchecked), so the runtime check stays defensive even though
      // `ApprovalDecideParams.signature` is now a required field.
      //
      // v1/v2 dispatch (docs/approval-decide-signing.md §2/§6): the signed
      // message bytes ARE the version. A request WITHOUT `timestamp` verifies
      // exactly as the pre-v2 v1 shape (`${request_id}:${choice}`); a request
      // WITH `timestamp` must be integer epoch ms within ±
      // approvalDecideTimestampWindowMs and verifies ONLY the v2 shape. There
      // is deliberately NO v1 fallback for timestamped requests — a failed v2
      // verification that fell back to v1 would let an attacker strip the
      // freshness binding and make the replay window bypassable.
      if (requireSession) {
        const sig = typeof params.signature === "string" ? params.signature : "";
        const deviceId = ctx.sessionId as string;
        let ok = false;
        if (sig.length > 0 && params.timestamp === undefined) {
          // v1 (legacy, no freshness binding) — byte-for-byte the pre-v2 path.
          ok =
            opts.verifyDeviceSignature?.(deviceId, approvalMessage(params.request_id, params.choice), sig) ?? false;
        } else if (sig.length > 0) {
          // typeof doubles as the TS narrowing guard; Number.isInteger then
          // rejects NaN, ±Infinity and fractions at runtime.
          const ts = params.timestamp;
          if (typeof ts !== "number" || !Number.isInteger(ts)) {
            return {
              kind: "error",
              code: ShannonError.BAD_PARAMS,
              message: "params.timestamp must be an integer epoch-ms value",
              data: { timestamp: ts },
            };
          }
          if (Math.abs(Date.now() - ts) > approvalDecideTimestampWindowMs) {
            return {
              kind: "error",
              code: ShannonError.BAD_PARAMS,
              message: "timestamp outside approval decide window",
              data: {
                timestamp: ts,
                windowMs: approvalDecideTimestampWindowMs,
              },
            };
          }
          ok =
            opts.verifyDeviceSignature?.(
              deviceId,
              approvalMessageV2(params.request_id, params.choice, ts),
              sig,
            ) ?? false;
        }
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
      // §L2: the decision landed at the engine — the ask leaves the restore
      // face (resolve BEFORE the ok so an aborted response still can't leave
      // a decided approval listed as pending). §K: the same signal also
      // unblocks a dispatched task's parked approval lane.
      opts.approvalRegistry?.resolve(params.request_id);
      opts.approvalDecisionSink?.(params.request_id, params.choice);
      return { kind: "result", result: { ok: true } satisfies OkResult };
    },

    // ── §L2 pending-approval restore face ──────────────────────────────────
    // Verbatim envelope `{"pendingApprovals": [...]}` (the phone's
    // `live_providers.dart` reads exactly that key); items are the
    // `approvalFromMap` contract — see `approvalWireItem`.
    "shannon/approval.list": async (_raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const pendingApprovals = (opts.approvalRegistry?.listPending() ?? []).map(approvalWireItem);
      return { kind: "result", result: { pendingApprovals } satisfies ApprovalListResult };
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

    // ── §J session face (cross-repo spec; engine RPC via one-shot call) ────
    "shannon/session.list": async (_raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      return withSessionClient(async (call) => {
        // §J1/§J4: v1 takes no params — unknown keys are ignored (the
        // reserve-then-enable pattern; `cursor` lands later).
        const snapshot = await fetchEngineSessions(call);
        const sessions = snapshot.sessions
          .map(mapSessionSummary)
          .filter((s): s is NonNullable<ReturnType<typeof mapSessionSummary>> => s !== null);
        return { kind: "result", result: { sessions } satisfies SessionListResult };
      });
    },

    "shannon/session.history": async (raw, ctx) => {
      const gate = sessionGate(ctx);
      if (gate) return gate;
      const params = (raw ?? {}) as Partial<SessionHistoryParams>;
      if (typeof params.sessionId !== "string" || params.sessionId.length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.sessionId is required",
        };
      }
      const before =
        typeof params.before === "string" && params.before.length > 0 ? params.before : undefined;
      const limit =
        typeof params.limit === "number" && Number.isFinite(params.limit) && params.limit >= 1
          ? Math.floor(params.limit)
          : undefined;
      return withSessionClient(async (call) => {
        // §J2: an unknown sessionId must NOT error — the engine degrades it to
        // an empty transcript (see `fetchEngineSessionHistory`), and the phone
        // then keeps its local records instead of blanking the thread.
        const transcript = await fetchEngineSessionHistory(
          call,
          params.sessionId as string,
          { before, limit },
        );
        return {
          kind: "result",
          result: mapSessionTranscript(transcript, params.sessionId as string),
        };
      });
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
    case "approval_request": {
      // §L1: the engine's rich fields (ts / agent / risk) ride the generated
      // types — pass them through verbatim, omitting the key when the engine
      // doesn't supply a usable value (the phone degrades honestly; the
      // legacy six-key shape stays byte-identical for old engines).
      const agent = engineAgent(ev.agent);
      const risk = engineRisk(ev.risk);
      return {
        type: "approval.request",
        request_id: ev.request_id,
        tool_name: ev.tool_name,
        tool_input: ev.tool_input,
        description: ev.description,
        is_destructive: ev.is_destructive,
        diff_preview: ev.diff_preview ?? null,
        ...(typeof ev.ts === "number" && ev.ts !== null ? { ts: ev.ts } : {}),
        ...(agent ? { agent } : {}),
        ...(risk ? { risk } : {}),
      };
    }
    case "session_info":
      // Metadata-only; no mobile-facing event. (Usage/cost for the turn already
      // arrives via the `usage` event, so nothing is lost.)
      return null;
    case "sessions.snapshot":
    case "session.transcript":
      // §J engine RPC responses — consumed by the one-shot `call()` path
      // (shannon/session.list / .history), never pushed as phone events.
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

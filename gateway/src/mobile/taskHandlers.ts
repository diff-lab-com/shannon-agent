/**
 * §K `shannon/task.dispatch` + `shannon/task.list` handlers (cross-repo spec,
 * mock-server aligned).
 *
 * Both REQUIRE a bound device session (pairing gate) — dispatching tasks and
 * reading the journal from an unpaired connection is rejected with
 * PAIRING_REQUIRED before anything is touched:
 *  - dispatch takes `{prompt, agent_id?}` (§K1). The engine has NO per-agent
 *    routing face (`shannon/agent.list` is a read-only roster view, not a
 *    dispatch target list), so ANY non-empty `agent_id` is rejected with
 *    INVALID_PARAMS instead of being silently routed to some other agent.
 *    The response is the full §K task object
 *    (`{task: {id, prompt, status, agent_id, created_at, …}}`), synchronously,
 *    before the §K3 event stream starts; the streamed content reaches the
 *    initiating device as `shannon/event`s whose `session_id` IS the task id.
 *  - list is a read-only §K2 projection of the in-memory task journal
 *    (`{tasks: [{id, prompt, status, agent_id, created_at, …}]}`, newest
 *    first; B1a adds the optional title/finished_at/error keys).
 *
 * §K also removed the P2-1 Y/N-text approval settle from this face: a
 * dispatch ALWAYS creates a task; pending approvals are answered via the
 * signed `shannon/approval/decide` (the hub settles its parked lane through
 * `MobileDispatchHub.settleApproval`).
 */

import {
  ShannonError,
  type TaskDispatchParams,
  type TaskDispatchResult,
  type TaskListParams,
  type TaskListResult,
} from "./protocol.js";
import { type MobileDispatchHub, wireTask } from "./hub.js";
import type { MethodHandlers } from "./server.js";

export interface TaskHandlersOptions {
  hub: MobileDispatchHub;
  /**
   * review §P1-13: trust registry for revoking compromised devices.
   * Without this, a device whose entry is removed from `devices.json` keeps
   * its long-lived session and can still dispatch tasks until the WS
   * reconnects. Injected by bootstrap alongside the engineBridge check.
   */
  isDeviceTrusted?: (sessionId: string) => boolean;
}

const PAIRING_REQUIRED = {
  kind: "error" as const,
  code: ShannonError.PAIRING_REQUIRED,
  message: "pair a device first (shannon/pair or shannon/device.resume)",
};

export function createTaskHandlers(opts: TaskHandlersOptions): MethodHandlers {
  const hub = opts.hub;
  return {
    "shannon/task.dispatch": async (raw, ctx) => {
      if (ctx.sessionId == null) return PAIRING_REQUIRED;
      // review §P1-13: revoked devices must not be able to dispatch tasks
      // through their still-open WS connection. The trust check matches
      // the one engineBridge already runs on shannon/* RPC methods.
      if (opts.isDeviceTrusted && !opts.isDeviceTrusted(ctx.sessionId)) {
        return {
          kind: "error",
          code: ShannonError.PAIRING_REQUIRED,
          message: "device trust revoked — pair again before dispatching",
        };
      }
      const params = (raw ?? {}) as Partial<TaskDispatchParams>;
      if (typeof params.prompt !== "string" || params.prompt.trim().length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.prompt (non-empty string) is required",
        };
      }
      // §K1: unknown agent_id → INVALID_PARAMS, never a silent re-route. The
      // engine has no per-agent routing face, so every non-empty value is
      // "unknown" — the B0 roster is a read view, not a dispatch target list.
      if (params.agent_id != null && String(params.agent_id).trim().length > 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "unknown agent_id — this host dispatches without an agent roster",
          data: { agent_id: params.agent_id },
        };
      }
      const outcome = hub.dispatch(ctx.sessionId, params.prompt.trim());
      const result: TaskDispatchResult = { task: wireTask(outcome.record) };
      return { kind: "result", result };
    },

    "shannon/task.list": async (raw, ctx) => {
      if (ctx.sessionId == null) return PAIRING_REQUIRED;
      const params = (raw ?? {}) as Partial<TaskListParams>;
      const limit =
        typeof params.limit === "number" && Number.isFinite(params.limit) && params.limit > 0
          ? Math.min(Math.floor(params.limit), 100)
          : 20;
      const result: TaskListResult = {
        tasks: hub.listTasks(ctx.sessionId, limit).map(wireTask),
      };
      return { kind: "result", result };
    },
  };
}

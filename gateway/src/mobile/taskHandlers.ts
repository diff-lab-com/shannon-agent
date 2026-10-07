/**
 * §K `shannon/task.dispatch` + `shannon/task.list` handlers (cross-repo spec,
 * mock-server aligned).
 *
 * Both REQUIRE a bound device session (pairing gate) — dispatching tasks and
 * reading the journal from an unpaired connection is rejected with
 * PAIRING_REQUIRED before anything is touched:
 *  - dispatch takes `{prompt, agent_id?}` (§K1). B0: a non-empty `agent_id` is
 *    validated against the host roster (`loadAgentRoster`, the same
 *    `~/.shannon/agents/*.toml` set `shannon/agent.list` serves) — a roster
 *    hit is accepted and recorded as the task's ATTRIBUTION (the wire task
 *    object's `agent_id`, plus `shannon/session.list` enrichment through the
 *    hub journal); anything else stays INVALID_PARAMS instead of being
 *    silently routed elsewhere. The boundary stands: the engine has NO
 *    per-agent routing face, so the recorded agent_id never changes what
 *    executes — the turn still runs as the default engine; the key only says
 *    "this configured agent owns the task".
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
import { loadAgentRoster } from "./agentRoster.js";
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
  /**
   * B0: `agent_id` validation scan targets (see `loadAgentRoster`) — the SAME
   * seam `createEngineHandlers` exposes for `shannon/agent.list`, so both
   * faces agree on one roster. Absent → the default `~/.shannon/agents` dir;
   * tests inject a tmp dir (or `[]` for a deterministically empty roster).
   */
  agentRosterDirs?: string[];
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
      // §K1 / B0: a non-empty agent_id must name a CONFIGURED agent — the
      // same roster `shannon/agent.list` serves (one injected dirs seam, one
      // truth). A roster hit is recorded as the task's attribution (the turn
      // still executes as the default engine — no per-agent routing face);
      // an unknown name keeps the §K1 INVALID_PARAMS, never a silent re-route.
      // Read per dispatch so a roster edit takes effect without a restart
      // (the loader never throws — broken files are skipped inside it).
      let agentId: string | null = null;
      const requestedAgent = String(params.agent_id ?? "").trim();
      if (requestedAgent.length > 0) {
        const roster = loadAgentRoster(opts.agentRosterDirs);
        if (!roster.some((a) => a.id === requestedAgent)) {
          return {
            kind: "error",
            code: ShannonError.BAD_PARAMS,
            message:
              "unknown agent_id — not in this host's agent roster (shannon/agent.list)",
            data: { agent_id: params.agent_id },
          };
        }
        agentId = requestedAgent;
      }
      const outcome = hub.dispatch(ctx.sessionId, params.prompt.trim(), agentId);
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

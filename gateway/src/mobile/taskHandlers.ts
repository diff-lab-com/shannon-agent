/**
 * P2-1 `shannon/task.dispatch` + `shannon/task.list` handlers.
 *
 * These are the phone-facing entry points of the mobile dispatch MVP. They are
 * deliberately thin:
 *  - both REQUIRE a bound device session (pairing gate) — dispatching tasks
 *    and reading the journal from an unpaired connection is rejected with
 *    PAIRING_REQUIRED before anything is touched;
 *  - dispatch delegates to the hub, which either resolves a pending approval
 *    (Y/N text, DingTalk parseChoice dialect) or routes the text through the
 *    same inbound pipeline the IM adapters use;
 *  - list is a read-only projection of the in-memory task journal.
 */

import {
  ShannonError,
  type TaskDispatchParams,
  type TaskDispatchResult,
  type TaskListParams,
  type TaskListResult,
} from "./protocol.js";
import type { MobileDispatchHub } from "./hub.js";
import type { MethodHandlers } from "./server.js";

export interface TaskHandlersOptions {
  hub: MobileDispatchHub;
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
      const params = (raw ?? {}) as Partial<TaskDispatchParams>;
      if (typeof params.text !== "string" || params.text.trim().length === 0) {
        return {
          kind: "error",
          code: ShannonError.BAD_PARAMS,
          message: "params.text (non-empty string) is required",
        };
      }
      const outcome = hub.dispatch(ctx.sessionId, params.text);
      if (outcome.kind === "approval") {
        const result: TaskDispatchResult = {
          ok: true,
          kind: "approval",
          task_id: null,
          choice: outcome.choice,
        };
        return { kind: "result", result };
      }
      const result: TaskDispatchResult = {
        ok: true,
        kind: "task",
        task_id: outcome.taskId,
      };
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
        tasks: hub.listTasks(ctx.sessionId, limit),
      };
      return { kind: "result", result };
    },
  };
}

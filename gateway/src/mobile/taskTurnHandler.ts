/**
 * §K3 mobile task turn handler — the engine-event consumption point for turns
 * dispatched from a paired phone (`shannon/task.dispatch`), replacing the P2-1
 * behavior of riding the IM text pipeline (`adapter.send` bubbles + lifecycle
 * stamps) with the structured task stream the phone routes by session key:
 *
 *   query.started  {turn_id, session_id: task.id}   ← hub, at dispatch acceptance
 *   task.progress  {session_id: task.id, content}   ← per engine text delta
 *   task.progress  {session_id: task.id, usage}     ← per engine usage frame
 *   task.message   {session_id: task.id, text}      ← terminal, final full reply
 *   query.failed   {session_id: task.id, error}     ← terminal, engine failure
 *
 * The phone keys its local task thread by the dispatch response's task id and
 * routes these events into it (live_chat_conversations `_onEvent`), so nothing
 * here may go out as an IM text bubble — `sendText` is the IM channel's
 * semantic and would double-send / misroute into a guessed thread. The IM
 * lifecycle stamps (🚀/✅/❌) are likewise NOT part of the task stream (§K3:
 * task.message/query.failed are the terminals); the journal transitions are
 * owned by the hub. Approvals still ride the shared in-channel round-trip
 * (`resolveApprovalInChannel`): the `approval.request` event reaches the phone
 * via the hub push and the device answers through the signed
 * `shannon/approval/decide`, which the bootstrap wires back to
 * `MobileDispatchHub.settleApproval` to unblock this lane.
 */

import { type EngineEvent } from "../engine/runtime.js";
import { resolveApprovalInChannel } from "../router/approvalTurnHandler.js";
import { toEngineAttachments } from "../router/media.js";
import { newAccumulator } from "../router/reply.js";
import { type TurnContext, type TurnHandler } from "../router/types.js";
import type { UsageFrame } from "./protocol.js";
import type { MobileDispatchHub } from "./hub.js";

export interface MobileTaskTurnHandlerOptions {
  /** The dispatch hub — structured event pushes + journal transitions. */
  hub: MobileDispatchHub;
  /** Engine HTTP base URL, e.g. `http://127.0.0.1:33420` (approval POST). */
  engineBaseUrl: string;
  /** Engine bearer token, forwarded to the approval POST (review §P1-13). */
  authToken?: string | null;
  /** Override for tests; defaults to the global fetch. */
  fetchImpl?: typeof fetch;
}

export function createMobileTaskTurnHandler(
  opts: MobileTaskTurnHandlerOptions,
): TurnHandler {
  const { hub } = opts;
  return {
    async handle(ctx: TurnContext): Promise<void> {
      const { client, adapter, replyTarget, inbound, logger } = ctx;
      // No reporter: the 🚀/✅/❌ lifecycle stamps are an IM-channel concern;
      // the task stream's terminals are task.message / query.failed below.
      const acc = newAccumulator();

      const query = client.runQuery(
        inbound.text,
        // Parity with the IM handlers — inbound media rides as attachments.
        { attachments: await toEngineAttachments(inbound.media, { logger }) },
      ) as AsyncIterable<EngineEvent>;
      for await (const ev of query) {
        switch (ev.type) {
          case "text":
            hub.pushTaskDelta(inbound.chatId, ev.content);
            acc.chunks.push(ev.content);
            break;
          case "usage": {
            const usage: UsageFrame = {
              input_tokens: ev.input_tokens,
              output_tokens: ev.output_tokens,
              cost_usd: ev.cost_usd,
            };
            hub.pushTaskUsage(inbound.chatId, usage);
            break;
          }
          case "approval_request":
            await resolveApprovalInChannel(opts, adapter, replyTarget, logger, ev);
            break;
          case "failed":
            acc.failed = ev.error;
            break;
          case "cancelled":
            acc.cancelled = true;
            break;
          default:
            // tool_use / tool_result / completed / session_info — the §K3
            // stream carries no query.completed and no tool frames.
            break;
        }
      }

      if (acc.failed !== null) {
        hub.failActiveTask(inbound.chatId, acc.failed);
      } else if (!acc.cancelled) {
        hub.completeActiveTask(inbound.chatId, acc.chunks.join(""));
      }
    },
  };
}

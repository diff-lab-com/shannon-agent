/**
 * P2-1 "mobile" platform adapter — the paired-phone channel expressed as a
 * `ChannelAdapter` so dispatched tasks ride the exact IM routing pipeline (T9):
 *
 *   task.dispatch → SessionRouter lane (serial per device, stable session_id)
 *                   → §K3 mobile task turn handler → engine
 *                   → structured task stream (query.started / task.progress /
 *                     task.message / query.failed, session_id = task id)
 *                     pushed back to the initiating phone through the hub.
 *
 * It has no platform transport of its own — `MobileServer` / the relay host
 * own the sockets; this adapter delegates delivery to the `MobileDispatchHub`
 * and surfaces the approval loop via `requestApproval` (the phone answers
 * through the signed `shannon/approval/decide`, which the bootstrap wires back
 * to the hub's parked lane).
 *
 * Capabilities: threading false (one lane per device), streaming "none" (the
 * §K3 task stream is NOT send/edits — the turn handler pushes structured
 * events straight through the hub; `send` keeps IM-bubble semantics only for
 * out-of-band pushes, and the live engine stream remains available on the
 * direct `shannon/query` path).
 */

import {
  type AdapterContext,
  type AdapterCapabilities,
  type ApprovalDecision,
  type ApprovalReq,
  type ChannelAdapter,
  type MessageReceipt,
  type NormalizedInbound,
  type ReplyTarget,
  type SendOpts,
  type SessionConversation,
} from "../adapters/types.js";
import type { MobileDispatchHub } from "./hub.js";

export interface MobileChannelAdapterOptions {
  hub: MobileDispatchHub;
}

export function createMobileChannelAdapter(
  opts: MobileChannelAdapterOptions,
): ChannelAdapter {
  const { hub } = opts;
  const capabilities: AdapterCapabilities = {
    threading: false,
    pairing: true,
    // The phone decides approvals via the signed shannon/approval/decide RPC,
    // not in-channel buttons — the adapter only parks the lane.
    approvalButtons: false,
    streaming: "none",
  };

  return {
    platform: "mobile",
    capabilities,

    // The WS servers own the transport lifecycle; there is nothing to dial.
    async start(_ctx: AdapterContext): Promise<void> {},
    async stop(): Promise<void> {},

    // Inbound arrives via shannon/task.dispatch (hub.dispatch), not a poller.
    onMessage(_handler: (m: NormalizedInbound) => void): void {},

    async send(target: ReplyTarget, text: string, _opts?: SendOpts): Promise<MessageReceipt> {
      const delivered = hub.sendText(target.chatId, text);
      if (!delivered) {
        throw new Error(
          `mobile: no connected device for "${target.chatId}" (pair and keep the page open)`,
        );
      }
      return { messageId: `mobile-${Date.now()}` };
    },

    async requestApproval(target: ReplyTarget, req: ApprovalReq): Promise<ApprovalDecision> {
      const choice = await hub.requestApproval(target.chatId, req);
      return { requestId: req.requestId, choice };
    },

    resolveSessionConversation(rawId: string): SessionConversation {
      return { baseChatId: rawId };
    },
  };
}

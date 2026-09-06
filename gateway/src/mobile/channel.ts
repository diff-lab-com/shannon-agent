/**
 * P2-1 "mobile" platform adapter — the paired-phone channel expressed as a
 * `ChannelAdapter` so dispatched tasks ride the exact IM pipeline (T9):
 *
 *   task.dispatch → SessionRouter lane (serial per device, stable session_id)
 *                   → approval-aware turn handler → engine
 *                   → replies + 任务开始/完成/失败 lifecycle stamps pushed
 *                     back to the phone through this adapter.
 *
 * It has no platform transport of its own — `MobileServer` / the relay host
 * own the sockets; this adapter delegates delivery to the `MobileDispatchHub`
 * and surfaces the DingTalk-style Y/N approval loop via `requestApproval`
 * (v1 is text Y/N / two buttons that send the same text, per the brief).
 *
 * Capabilities: threading false (one lane per device), streaming "none" (the
 * phone gets the final reply + lifecycle stamps as `task.message` events; the
 * live engine stream remains available on the direct `shannon/query` path).
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
    approvalButtons: false, // v1: text Y/N (the PWA page's buttons send the same text)
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

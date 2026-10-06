import {
  type ChannelAdapter,
  type Logger,
  type NormalizedInbound,
  type Platform,
  type ReplyTarget,
} from "../adapters/types.js";
import { Allowlist, defaultAllowlistPath } from "./allowlist.js";
import { type PairingRecord, PairingStore } from "./pairing.js";

/**
 * Access-control decision for one inbound (F14). The host acts on the outcome
 * before the message reaches the router.
 */
export type GuardDecision =
  | { decision: "allow" }
  | { decision: "challenge"; code: string; expiresAt: number }
  | { decision: "deny"; reason: string };

export interface InboundGuard {
  check(inbound: NormalizedInbound): GuardDecision | Promise<GuardDecision>;
}

/**
 * The pairing-challenge copy shown to an unpaired DM sender (review F42).
 * States the approval channels that ACTUALLY exist: a paired sender replying
 * `approve <code>` in any chat, or the gateway owner adding the entry to the
 * persisted allowlist file. Single source of truth so the bootstrap wiring
 * and `createGuardedInbound` never drift.
 */
export function pairingChallengeMessage(code: string, allowlistFile?: string): string {
  const file = allowlistFile ?? defaultAllowlistPath();
  return (
    `Pairing required — an already-paired user must reply "approve ${code}" ` +
    `to this bot (any chat), or the gateway owner must add your id to ` +
    `${file}. (expires in 5 min)`
  );
}

/**
 * Pairing-based guard.
 *
 * - allowlisted sender → allow
 * - unallowlisted DM (isDirect) → challenge (issue a pairing code; a paired
 *   sender approves it with `approve <code>` → Allowlist.allow, persisted)
 * - unallowlisted group mention → deny ("DM the bot to pair first")
 *
 * Groups never trigger a challenge: pairing only happens in a private context,
 * so a stranger can't bootstrap access by @-mentioning the bot in a shared
 * channel.
 */
export class AllowlistGuard implements InboundGuard {
  constructor(
    private readonly allowlist: Allowlist,
    private readonly pairing: PairingStore,
  ) {}

  check(inbound: NormalizedInbound): GuardDecision {
    if (this.allowlist.isAllowed(inbound.platform, inbound.senderId)) {
      return { decision: "allow" };
    }
    if (inbound.isDirect) {
      const record = this.pairing.issue(inbound);
      return {
        decision: "challenge",
        code: record.code,
        expiresAt: record.expiresAt,
      };
    }
    return {
      decision: "deny",
      reason: "You're not paired with this agent yet — send it a direct message to pair.",
    };
  }

  /**
   * Review F42: handle an `approve <code>` command from an already-allowed
   * sender — consume the pending code, allowlist (and thereby persist) the
   * requester. Self-approval is rejected: the sender who was challenged can
   * never approve their own code. Never throws; the outcome becomes the
   * channel reply.
   */
  approve(
    inbound: NormalizedInbound,
    code: string,
  ): { ok: true; record: PairingRecord } | { ok: false; reason: string } {
    return approvePairingCode({
      allowlist: this.allowlist,
      pairing: this.pairing,
      code,
      approver: { platform: inbound.platform, senderId: inbound.senderId },
    });
  }
}

/**
 * Who is approving. An IM approver is the `{platform, senderId}` the `approve
 * <code>` reply came from; the desktop RPC approver has no IM identity (it is
 * the gateway owner's device) and passes `undefined`.
 */
export interface PairingApprover {
  platform: Platform;
  senderId: string;
}

export type PairingApprovalOutcome =
  | { ok: true; record: PairingRecord }
  | { ok: false; reason: string };

/**
 * THE pairing-approval implementation (T9) — one code path for both channels:
 * the IM `approve <code>` reply (`AllowlistGuard.approve`) and the desktop's
 * `shannon/pairing.approve` RPC. Consumes the pending code via the PairingStore
 * and persists the requester via `Allowlist.allow`.
 *
 * With `approver` set (IM reply): the sender must already be allowlisted and
 * can never approve their own code. Without `approver` (desktop RPC): the
 * caller has already authenticated as the owner (paired device or a fresh
 * one-time pair token — see mobile/accessRpc.ts), so only the code check
 * applies. Never throws; the outcome becomes the channel reply.
 */
export function approvePairingCode(opts: {
  allowlist: Allowlist;
  pairing: PairingStore;
  code: string;
  approver?: PairingApprover;
}): PairingApprovalOutcome {
  const { allowlist, pairing, code, approver } = opts;
  if (approver && !allowlist.isAllowed(approver.platform, approver.senderId)) {
    return {
      ok: false,
      reason: "Only an already-paired sender can approve a pairing.",
    };
  }
  const pending = pairing.peek(code);
  if (!pending) {
    return {
      ok: false,
      reason:
        `Unknown or expired pairing code ${code} — ask the requester to DM ` +
        "the bot again for a fresh one.",
    };
  }
  if (
    approver &&
    pending.platform === approver.platform &&
    pending.senderId === approver.senderId
  ) {
    return {
      ok: false,
      reason: "Pairing cannot be self-approved — a different paired sender must approve it.",
    };
  }
  const consumed = pairing.consume(code);
  if (!consumed) {
    return { ok: false, reason: `Pairing code ${code} just expired — request a fresh one.` };
  }
  allowlist.allow(consumed.platform, consumed.senderId);
  return { ok: true, record: consumed };
}

/**
 * Compose a guard in front of inbound routing. Returns the handler to pass to
 * `adapter.onMessage`:
 *
 *   adapter.onMessage((m) => void guarded(m, adapter));
 *
 * `onAllow` is typically `router.handleInbound` (passed as a callback so this
 * module stays decoupled from the router). Challenge/deny outcomes send an
 * explanatory message to the inbound's reply target and do NOT reach the engine.
 */
export function createGuardedInbound(opts: {
  guard: InboundGuard;
  onAllow: (inbound: NormalizedInbound) => Promise<void>;
  logger: Logger;
}): (inbound: NormalizedInbound, adapter: ChannelAdapter) => Promise<void> {
  const { guard, onAllow, logger } = opts;
  return async (inbound, adapter) => {
    const decision = await guard.check(inbound);
    if (decision.decision === "allow") {
      await onAllow(inbound);
      return;
    }
    const replyTarget: ReplyTarget = {
      platform: inbound.platform,
      chatId: inbound.chatId,
      threadId: inbound.threadId,
    };
    if (decision.decision === "challenge") {
      logger.info(`pairing challenge issued for ${inbound.platform}:${inbound.senderId}`);
      await adapter.send(replyTarget, pairingChallengeMessage(decision.code));
      return;
    }
    logger.info(`denied inbound from ${inbound.platform}:${inbound.senderId} (not paired, group)`);
    await adapter.send(replyTarget, decision.reason);
  };
}

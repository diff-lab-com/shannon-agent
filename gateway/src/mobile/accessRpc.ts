/**
 * T9: desktop pairing-approval RPC — `shannon/pairing.pending` +
 * `shannon/pairing.approve`.
 *
 * Review F42's "full form": an approver should see the pending IM pairing
 * codes in the Shannon desktop app and approve them there, instead of only
 * replying `approve <code>` from an already-allowed sender. Both channels
 * coexist over ONE approval implementation (`approvePairingCode` in
 * access/guard.ts) and the same PairingStore/Allowlist the IM guard uses.
 *
 * Caller trust — the same bar as the sibling sensitive RPCs (the
 * `shannon/approval/decide` sessionGate), with one desktop-specific leg:
 *  - a bound, still-trusted device session (what a paired phone holds), OR
 *  - a valid pair token in the request — the desktop host's credential. The
 *    desktop has no device key; its control channel to the gateway is the
 *    pair-token JSONL it alone can mint (Design D, the same tokens
 *    `shannon/pair` consumes). `pending` is a read, so the token is only
 *    verified; `approve` mutates access control and consumes it (single-use,
 *    so a replayed token approves nothing twice).
 *
 * Transport: the JSON-RPC methods ride the `shannon/*` WebSocket dispatch like
 * every other mobile method. The SAME handler functions are also exposed over
 * plain HTTP POST on the mobile listener (`createPairingAccessHttp`) — that is
 * the leg the desktop Rust app calls (it has an HTTP client but no WS client;
 * the endpoint is TLS'd with the gateway's self-signed cert when mobile.tls is
 * on). The HTTP skin is deliberately narrow: exactly these two paths, JSON
 * bodies, and the mobile server applies the same cross-site Origin defense it
 * applies to WS upgrades.
 */

import { type Allowlist } from "../access/allowlist.js";
import { approvePairingCode } from "../access/guard.js";
import { type PairingRecord, type PairingStore } from "../access/pairing.js";
import { type Logger } from "../adapters/types.js";
import {
  ShannonError,
  type PairingApproveParams,
  type PairingApproveResult,
  type PairingPendingResult,
  type PairingRequestRecord,
} from "./protocol.js";
import { type PairTokenStore } from "./pairing.js";
import type { HandlerOutcome, MethodContext, MethodHandlers } from "./server.js";
import { type DeviceRegistry } from "./pairing.js";

export interface PairingAccessOptions {
  allowlist: Allowlist;
  pairing: PairingStore;
  /** The same token store `shannon/pair` consumes — the desktop's credential. */
  tokens: PairTokenStore;
  /** Device trust registry, mirroring engineBridge's sessionGate re-check. */
  registry: DeviceRegistry;
  logger: Logger;
}

/** The desktop-facing HTTP paths, in one place so desktop and gateway agree. */
export const PAIRING_PENDING_PATH = "/rpc/pairing/pending";
export const PAIRING_APPROVE_PATH = "/rpc/pairing/approve";

export type PairingHttpApi = (
  path: string,
  rawBody: string,
) => Promise<{ status: number; body: string } | null>;

/** Wire shape for one pending/approved pairing (protocol.ts PairingRequestRecord). */
function toRecord(r: PairingRecord): PairingRequestRecord {
  return {
    code: r.code,
    platform: r.platform,
    senderId: r.senderId,
    requestedAt: r.createdAt,
    expiresAt: r.expiresAt,
  };
}

function pairingRequired(message: string): HandlerOutcome {
  return { kind: "error", code: ShannonError.PAIRING_REQUIRED, message };
}

/**
 * Build the pairing-access surface. `result.handlers` plugs into the mobile
 * `shannon/*` MethodHandlers map; `result.http` plugs into the MobileServer's
 * `httpApi` POST hook. Both skins share the private `pending`/`approve` logic.
 */
export function createPairingAccess(opts: PairingAccessOptions): {
  handlers: MethodHandlers;
  http: PairingHttpApi;
} {
  const { allowlist, pairing, tokens, registry, logger } = opts;

  /**
   * Mirror of engineBridge's sessionGate (requireSession mode): a caller with
   * a bound session must still be a registered (non-revoked) device.
   */
  const sessionTrusted = (ctx: MethodContext): boolean =>
    ctx.sessionId != null && registry.has(ctx.sessionId);

  /**
   * Token leg. `consume: false` for read-only pending (verify only);
   * `consume: true` for approve (single-use — the mutation buys the burn).
   */
  const tokenAuthorized = (raw: unknown, consume: boolean): boolean => {
    const token = typeof (raw as { token?: unknown })?.token === "string"
      ? (raw as { token: string }).token
      : "";
    if (token.length === 0) return false;
    return consume ? tokens.consume(token) != null : tokens.verify(token) != null;
  };

  async function pending(raw: unknown, ctx: MethodContext): Promise<HandlerOutcome> {
    if (!sessionTrusted(ctx) && !tokenAuthorized(raw, false)) {
      return pairingRequired(
        "pair a device first or supply a valid pair token for shannon/pairing.pending",
      );
    }
    const result: PairingPendingResult = {
      pending: pairing.listPending().map(toRecord),
    };
    return { kind: "result", result };
  }

  async function approve(raw: unknown, ctx: MethodContext): Promise<HandlerOutcome> {
    const params = (raw ?? {}) as Partial<PairingApproveParams>;
    const code = typeof params.code === "string" ? params.code.trim() : "";
    if (code.length === 0) {
      return { kind: "error", code: ShannonError.BAD_PARAMS, message: "params.code is required" };
    }
    if (!sessionTrusted(ctx) && !tokenAuthorized(raw, true)) {
      return pairingRequired(
        "pair a device first or supply a valid pair token for shannon/pairing.approve",
      );
    }
    // THE shared approval — the IM `approve <code>` reply lands here too. The
    // RPC approver is the owner's device, not an IM sender, so no approver
    // identity is passed (self-approval does not apply).
    const outcome = approvePairingCode({ allowlist, pairing, code });
    if (!outcome.ok) {
      return { kind: "error", code: ShannonError.BAD_PARAMS, message: outcome.reason };
    }
    logger.info(
      `pairing approved via RPC — ${outcome.record.platform}:${outcome.record.senderId} allowlisted`,
    );
    const result: PairingApproveResult = { ok: true, record: toRecord(outcome.record) };
    return { kind: "result", result };
  }

  const handlers: MethodHandlers = {
    "shannon/pairing.pending": (raw, ctx) => pending(raw, ctx),
    "shannon/pairing.approve": (raw, ctx) => approve(raw, ctx),
  };

  const http: PairingHttpApi = async (path, rawBody) => {
    const method =
      path === PAIRING_PENDING_PATH
        ? "shannon/pairing.pending"
        : path === PAIRING_APPROVE_PATH
          ? "shannon/pairing.approve"
          : null;
    if (method === null) return null;
    let parsed: unknown;
    try {
      parsed = rawBody.trim().length === 0 ? {} : (JSON.parse(rawBody) as unknown);
    } catch {
      return jsonOut(400, {
        error: { code: ShannonError.PARSE_ERROR, message: "request body is not valid JSON" },
      });
    }
    // The handlers never bind a session over HTTP — callers authenticate via
    // the pair-token leg only. sessionId stays null (gate falls through to it).
    const ctx: MethodContext = {
      socket: null as unknown as MethodContext["socket"],
      sessionId: null,
      logger,
    };
    const handler = handlers[method];
    if (!handler) {
      return jsonOut(500, { error: { message: `no handler for ${method}` } });
    }
    const outcome = await handler(parsed, ctx);
    if (outcome.kind === "error") {
      return jsonOut(400, {
        error: { code: outcome.code, message: outcome.message },
      });
    }
    return jsonOut(200, { result: outcome.result });
  };

  return { handlers, http };
}

function jsonOut(status: number, body: unknown): { status: number; body: string } {
  return { status, body: JSON.stringify(body) };
}

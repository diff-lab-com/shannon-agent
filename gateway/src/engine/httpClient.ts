/**
 * HTTP helpers for the engine's non-streaming endpoints.
 *
 * The query stream lives on the WebSocket (src/engine/wsClient.ts), but the
 * approval round-trip is a plain POST (P0-b deliberately used HTTP so the
 * response need not share the query socket): `POST /api/approval/respond`
 * with body `{ request_id, choice }`.
 *
 * Uses the global `fetch` (Node 20+) — no extra dependency.
 *
 * Choice mapping: the gateway adapter exposes `allow | deny | allow_session`;
 * the engine's wire enum is `allow_once | always_allow | always_allow_session |
 * deny`. We map `allow → allow_once` (the safe one-shot) and
 * `allow_session → always_allow_session` (P3-3: in-session grant, never
 * persisted). "always_allow" (persisted) stays a non-gateway option.
 */

import { ENGINE_HTTP_TIMEOUT_MS } from "../lib/netTimeouts.js";

// Re-exported for callers/tests that pin the approval POST budget.
export { ENGINE_HTTP_TIMEOUT_MS };

export type GatewayApprovalChoice = "allow" | "deny" | "allow_session";

export interface RespondToApprovalOptions {
  /** Engine HTTP base URL, e.g. `http://127.0.0.1:33420`. */
  engineBaseUrl: string;
  requestId: string;
  choice: GatewayApprovalChoice;
  /** Engine bearer token; sent as `Authorization: Bearer …` when set. */
  authToken?: string | null;
  /** Override for tests; defaults to the global fetch. */
  fetchImpl?: typeof fetch;
  /**
   * review §P2-23: abort the POST after this many ms instead of hanging on a
   * wedged engine. Defaults to {@link ENGINE_HTTP_TIMEOUT_MS}.
   */
  timeoutMs?: number;
}

export async function respondToApproval(
  opts: RespondToApprovalOptions,
): Promise<void> {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const wireChoice =
    opts.choice === "allow"
      ? "allow_once"
      : opts.choice === "allow_session"
        ? "always_allow_session"
        : "deny";
  const url = `${opts.engineBaseUrl.replace(/\/+$/, "")}/api/approval/respond`;
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (opts.authToken) headers.authorization = `Bearer ${opts.authToken}`;
  const res = await fetchImpl(url, {
    method: "POST",
    headers,
    body: JSON.stringify({ request_id: opts.requestId, choice: wireChoice }),
    signal: AbortSignal.timeout(opts.timeoutMs ?? ENGINE_HTTP_TIMEOUT_MS),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "<no body>");
    throw new Error(
      `approval respond failed: HTTP ${res.status} from ${url}: ${body}`,
    );
  }
}

/** P3-3: options for the approval-mode read/tighten helpers. */
export interface ApprovalModeOptions {
  engineBaseUrl: string;
  /** The device's attached engine session (the mobile ctx.sessionId). */
  sessionId: string;
  authToken?: string | null;
  fetchImpl?: typeof fetch;
  timeoutMs?: number;
}

/** P3-3: `GET /api/approval/mode` — the token currently in effect. */
export async function getApprovalMode(
  opts: ApprovalModeOptions,
): Promise<{ mode: string }> {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const url = `${opts.engineBaseUrl.replace(/\/+$/, "")}/api/approval/mode?session_id=${encodeURIComponent(opts.sessionId)}`;
  const headers: Record<string, string> = {};
  if (opts.authToken) headers.authorization = `Bearer ${opts.authToken}`;
  const res = await fetchImpl(url, {
    method: "GET",
    headers,
    signal: AbortSignal.timeout(opts.timeoutMs ?? ENGINE_HTTP_TIMEOUT_MS),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "<no body>");
    throw new Error(`approval mode read failed: HTTP ${res.status} from ${url}: ${body}`);
  }
  return (await res.json()) as { mode: string };
}

/** P3-3: `POST /api/approval/mode` — the mobile TIGHTEN route; the engine
 *  rejects everything except `readonly`. */
export async function setApprovalMode(
  opts: ApprovalModeOptions & { mode: "readonly" },
): Promise<{ mode: string }> {
  const fetchImpl = opts.fetchImpl ?? fetch;
  const url = `${opts.engineBaseUrl.replace(/\/+$/, "")}/api/approval/mode`;
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (opts.authToken) headers.authorization = `Bearer ${opts.authToken}`;
  const res = await fetchImpl(url, {
    method: "POST",
    headers,
    body: JSON.stringify({ session_id: opts.sessionId, mode: opts.mode }),
    signal: AbortSignal.timeout(opts.timeoutMs ?? ENGINE_HTTP_TIMEOUT_MS),
  });
  if (!res.ok) {
    const body = await res.text().catch(() => "<no body>");
    throw new Error(`approval mode set failed: HTTP ${res.status} from ${url}: ${body}`);
  }
  return (await res.json()) as { mode: string };
}

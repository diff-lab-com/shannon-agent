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
 * Choice mapping: the gateway adapter exposes `allow | deny`; the engine's
 * wire enum is `allow_once | always_allow | deny`. We map `allow → allow_once`
 * (the safe one-shot). "always_allow" is a future platform-UX option.
 */

import { ENGINE_HTTP_TIMEOUT_MS } from "../lib/netTimeouts.js";

// Re-exported for callers/tests that pin the approval POST budget.
export { ENGINE_HTTP_TIMEOUT_MS };

export type GatewayApprovalChoice = "allow" | "deny";

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
  const wireChoice = opts.choice === "allow" ? "allow_once" : "deny";
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

/**
 * Outbound request timeouts (review §P2-23).
 *
 * Before this module every gateway outbound call — engine HTTP, platform
 * APIs, media downloads — could hang forever on a wedged peer, holding the
 * adapter's send path (and with it the session lane) indefinitely. Every
 * outbound `fetch` now carries an `AbortSignal.timeout` from here so a dead
 * peer degrades into a normal error the existing retry/log paths already
 * handle.
 *
 * Two magnitudes, chosen by callee semantics:
 *  - engine calls (`/api/approval/respond`, model probes): the engine may
 *    legitimately take a while to unwind an in-flight turn, but must not
 *    hold the gateway longer than a turn's practical lifetime → 60s.
 *  - platform API calls (Slack/Telegram/Discord/… `chat.postMessage`,
 *    media downloads): platform APIs answer in single-digit seconds under
 *    normal operation; 30s is a generous ceiling that still unblocks the
 *    lane instead of hanging it → 30s.
 */

/** Timeout for calls into the Shannon engine's HTTP surface. */
export const ENGINE_HTTP_TIMEOUT_MS = 60_000;

/** Timeout for calls out to chat-platform APIs (send/edit/media download). */
export const PLATFORM_HTTP_TIMEOUT_MS = 30_000;

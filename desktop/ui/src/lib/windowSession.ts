// P1-1 — session multi-window helpers.
//
// A dedicated session window boots with `/?windowSession=<uuid>` (set by
// the backend `open_session_window` command). This module parses that
// parameter and provides the event-scoping predicate used by AppContext so
// a session window only consumes `query:*` events belonging to its own
// session. Nothing here touches persistence — window mode is derived from
// the URL on every boot (in-memory only, per the task brief).

export const WINDOW_SESSION_PARAM = 'windowSession'

/**
 * Desktop-internal control event emitted (targeted at `main`) by the
 * window-mode header's「在主窗口打开」button — the backend command
 * `reveal_session_in_main` focuses `main` and emits this; Layout switches
 * to the session. Must match `SESSION_WINDOW_REVEAL` in
 * `desktop/src/session_window_commands.rs`.
 */
export const SESSION_WINDOW_REVEAL_EVENT = 'session-window:reveal'

// Session ids are UUIDs end-to-end (backend registry `SessionKey(Uuid)`).
const UUID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/**
 * Parse `?windowSession=<uuid>` from a query string.
 * Returns `null` in the main window, on a missing param, or when the value
 * is not a well-formed UUID (a malformed param must never silently switch
 * sessions). Defaults to `window.location.search` in the browser.
 */
export function parseWindowSession(
  search: string = typeof window !== 'undefined' ? window.location.search : '',
): string | null {
  if (!search) return null
  const value = new URLSearchParams(search).get(WINDOW_SESSION_PARAM)
  if (!value) return null
  const trimmed = value.trim()
  return UUID_RE.test(trimmed) ? trimmed.toLowerCase() : null
}

/**
 * Whether a streamed-event payload belongs to the current window's session.
 *
 * - Main window (`windowSessionId === null`): accept everything — existing
 *   behavior is untouched.
 * - Session window: accept the payload when it carries this window's
 *   session id. Payloads without a `session_id` (older backend) are also
 *   accepted so the filter degrades to "no filtering" instead of dropping
 *   the stream.
 */
export function isEventForCurrentWindow(
  payloadSessionId: string | null | undefined,
  windowSessionId: string | null,
): boolean {
  if (windowSessionId === null) return true
  if (payloadSessionId == null) return true
  return payloadSessionId === windowSessionId
}

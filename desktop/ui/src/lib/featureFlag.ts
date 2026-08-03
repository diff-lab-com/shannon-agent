/**
 * P2-5a — feature-flag seam for the chat.v2 runtime adapter.
 *
 * The chat.v2 flag gates the assistant-ui runtime on the production
 * `/chat` route. With it OFF, the legacy `Chat.tsx` rendering path is
 * untouched. With it ON, `AppProvider` wraps its children in
 * `<AssistantRuntimeProvider>` driven by `makeShannonTauriAdapter`.
 *
 * Resolution order:
 *   1. `window.__SHANNON_CHAT_V2__` (useful for runtime toggles during
 *      manual testing / staged rollouts).
 *   2. `import.meta.env.VITE_CHAT_V2` (build-time Vite env var). Accepts
 *      the literal strings `'1'` / `'true'` / `'on'` (case-insensitive).
 *   3. Default: `false` — every existing test and production build keeps
 *      the legacy path until we explicitly opt in.
 *
 * Acceptance criterion #3 in `docs/plans/chat-upgrade.md §3.1` requires
 * the OFF path to be byte-equivalent to the legacy `Chat.tsx` behaviour,
 * which the `Chat.test.tsx` suite guards. The test file
 * `__tests__/featureFlag.test.ts` covers the resolution order itself.
 */

declare global {
  interface Window {
    /** Runtime override (e.g. set by an embedded webview loader). */
    __SHANNON_CHAT_V2__?: boolean
  }
}

/** Truthy string set recognised by the Vite env-var fallback. */
const TRUTHY = new Set(['1', 'true', 'on', 'yes'])

/** Read a tri-state override from a source, returning `undefined` if absent. */
function readFromRecord(
  src: Record<string, unknown> | undefined,
  key: string,
): boolean | undefined {
  if (!src) return undefined
  const raw = src[key]
  if (typeof raw === 'boolean') return raw
  if (typeof raw === 'string') {
    const v = raw.trim().toLowerCase()
    if (TRUTHY.has(v)) return true
    if (v === '' || v === '0' || v === 'false' || v === 'off' || v === 'no') return false
  }
  return undefined
}

/** Resolve the chat.v2 flag. Safe to call from server/SSR contexts. */
export function isChatV2Enabled(env?: Record<string, unknown>): boolean {
  // Runtime override — useful for staged rollouts and a kill switch.
  if (typeof window !== 'undefined' && typeof window.__SHANNON_CHAT_V2__ === 'boolean') {
    return window.__SHANNON_CHAT_V2__
  }
  // Test override: caller passes an env-shaped record (e.g. `import.meta.env`).
  if (env !== undefined) {
    const v = readFromRecord(env, 'VITE_CHAT_V2')
    if (typeof v === 'boolean') return v
  }
  // Default: production fallback reads from Vite's `import.meta.env`.
  // Guarded with `typeof` so unit tests without Vite don't crash.
  if (typeof import.meta !== 'undefined') {
    const metaEnv = (import.meta as { env?: Record<string, unknown> }).env
    const v = readFromRecord(metaEnv, 'VITE_CHAT_V2')
    if (typeof v === 'boolean') return v
  }
  return false
}

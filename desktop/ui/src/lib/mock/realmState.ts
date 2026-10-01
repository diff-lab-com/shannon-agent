// Realm-global singleton store for the mock layer (R1 chat-testing infra).
//
// WHY THIS EXISTS: vite dev serves the demo app with TWO module-graph
// instances of the mock layer per page (pre-existing on the base commit —
// coreMock's init log prints twice, one fetch, one realm). The old coreMock
// was stateless so the fork was invisible; the scripted backend adds mutable
// state (player, seed, event registry) that MUST be shared, or one instance
// arms the script while the other answers `send_message`. All mutable mock
// state therefore lives on the window realm, not in module bindings.

/**
 * Return the realm-wide singleton stored under `key`, creating it on first
 * use. Without a window (SSR/unit tests) each call gets a fresh instance.
 */
export function realmSingleton<T>(key: string, create: () => T): T {
  if (typeof window === 'undefined') return create()
  const w = window as unknown as Record<string, unknown>
  if (!w[key]) w[key] = create()
  return w[key] as T
}

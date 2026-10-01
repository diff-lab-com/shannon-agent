// ScriptedBackend assembly — glues the event bridge, the script player and
// the mock handlers together, and installs the `window.__shannonMock` test
// console (R1 chat-testing infra, plan §2.2).
//
// Called ONCE from coreMock module init (the vite alias guarantees that runs
// before any component renders, so a boot script passed via
// `window.__SHANNON_SCRIPT__` — see e2e/helpers/scriptLoader.ts — seeds the
// mock store before the app's first fetch).

import { handlers } from '../handlers'
import { dispatchEvent, eventPluginHandlers, installTauriInternals } from '../eventBridge'
import { realmSingleton } from '../realmState'
import { ScriptPlayer } from './player'

/**
 * The single player instance backing `window.__shannonMock`. Realm-global
 * (see ../realmState.ts): vite dev evaluates the mock layer twice per page,
 * so a module-bound player would fork — one instance arms the script while
 * the other answers `send_message`.
 */
export const chatPlayer = realmSingleton('__shannonMockPlayer', () => new ScriptPlayer({
  emit: (event, payload) => dispatchEvent(event, payload),
}))

type ShannonMockConsole = {
  /** Emit one event through the bridge, exactly like the scripted player. */
  emit(name: string, payload?: Record<string, unknown>): void
  /** Validate + arm a ChatScript (accepts the object or its JSON string). */
  loadScript(json: unknown): { ok: boolean; errors: string[] }
  control: {
    /** Resume a paused turn (waitFor / permission-request / pauseAt). */
    resume(): void
    /** Park the player when the current turn reaches step index `i`. */
    pauseAt(i: number): void
    /** Emit immediately, bypassing player state (raw payload, no fill). */
    emitNow(name: string, payload?: Record<string, unknown>): void
    /** Chunk-gap time scale (2 = twice as fast). Persisted until reset. */
    speed: number
  }
  /** Drop the script + seed override; back to the default demo data. */
  reset(): void
  /** Introspection for debugging: { phase, turnIndex, stepIndex, … }. */
  snapshot(): ReturnType<ScriptPlayer['snapshot']>
}

declare global {
  interface Window {
    /** Set by e2e/helpers/scriptLoader.ts BEFORE app scripts run. */
    __SHANNON_SCRIPT__?: unknown
    /** Demo-mode test console (this module). */
    __shannonMock?: ShannonMockConsole
  }
}

/**
 * Merge the Tauri v2 event-plugin handlers into the mock handler table,
 * install the window internals the real event module needs, expose the
 * test console and arm a boot script when one was injected.
 * Idempotent (Safe to call again — e.g. under HMR).
 */
export function initScriptedBackend(routeInvoke: (cmd: string, args: Record<string, unknown> | undefined) => Promise<unknown>): void {
  for (const [cmd, handler] of Object.entries(eventPluginHandlers)) {
    // Keep any existing registration (HMR re-entry) — these are ours.
    if (!handlers[cmd]) handlers[cmd] = handler
  }
  installTauriInternals(routeInvoke)
  if (typeof window === 'undefined') return

  const control: ShannonMockConsole['control'] = {
    resume: () => chatPlayer.resume(),
    pauseAt: (i: number) => chatPlayer.pauseAt(i),
    emitNow: (name: string, payload?: Record<string, unknown>) => chatPlayer.emitNow(name, payload ?? {}),
    get speed() { return chatPlayer.speed },
    set speed(v: number) { chatPlayer.speed = v },
  }

  const console_: ShannonMockConsole = {
    emit: (name, payload) => dispatchEvent(name, payload ?? {}),
    loadScript: (json) => {
      const parsed = typeof json === 'string' ? safeJsonParse(json) : json
      if (typeof json === 'string' && parsed === null) {
        return { ok: false, errors: ['loadScript: string input is not valid JSON'] }
      }
      return chatPlayer.load(parsed)
    },
    control,
    reset: () => chatPlayer.reset(),
    snapshot: () => chatPlayer.snapshot(),
  }
  window.__shannonMock = console_

  // Boot script — injected by e2e/helpers/scriptLoader.ts via addInitScript
  // so the seed lands before the app's first get_config/list_sessions fetch.
  if (window.__SHANNON_SCRIPT__ != null) {
    const result = console_.loadScript(window.__SHANNON_SCRIPT__)
    if (!result.ok) {
      console.error('[mock] __SHANNON_SCRIPT__ failed validation:', result.errors)
    }
  }
}

function safeJsonParse(json: string): unknown {
  try {
    return JSON.parse(json)
  } catch {
    return null
  }
}

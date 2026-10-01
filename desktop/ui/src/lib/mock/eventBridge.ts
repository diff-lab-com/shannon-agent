// Tauri v2 event bridge for mock (demo) mode — R1 chat-testing infra.
//
// WHY THIS EXISTS
// `@tauri-apps/api/event` (v2.11) never calls the aliased coreMock: its
// `listen`/`emit` import `{ invoke, transformCallback }` from `./core.js`
// — a RELATIVE specifier the vite alias (`@tauri-apps/api/core`) does not
// rewrite — and both funnel into `window.__TAURI_INTERNALS__`:
//
//   invoke(cmd, args, options)      → window.__TAURI_INTERNALS__.invoke(cmd, args, options)
//   transformCallback(cb, once)     → window.__TAURI_INTERNALS__.transformCallback(cb, once)
//   listen(event, handler, opts)    → invoke('plugin:event|listen', { event, target, handler: <cbId> })
//                                     → resolves eventId; returns unlisten closure
//   unlisten()                      → window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(event, eventId)
//                                     then invoke('plugin:event|unlisten', { event, eventId })
//   emit(event, payload)            → invoke('plugin:event|emit', { event, payload })
//
// (Measured against node_modules/@tauri-apps/api@2.11.1 event.js / core.js;
// the same shapes the official `mocks.js` mockIPC uses — see §"chosen
// interception path" below.)
//
// CHOSEN INTERCEPTION PATH
// We install the two `window.__TAURI_*_INTERNALS__` registries at coreMock
// module init (this module is imported there — the alias guarantees that
// happens before any component renders, hence before the first `listen`).
// `__TAURI_INTERNALS__.invoke` routes into the SAME handlers table the
// aliased coreMock invoke uses, so app-code invokes and real-event-module
// invokes land in one place, and `transformCallback` registers callbacks in
// a real registry instead of returning a random number (the old behavior —
// every demo-mode `listen()` used to reject with "__TAURI_INTERNALS__ is
// undefined"). `isTauri()` is untouched: core.js checks `globalThis.isTauri`,
// which we never set, so Tauri-detection code keeps reporting "not in Tauri".
//
// CALLBACK SHAPE (the contract the player relies on)
// Each registered callback is invoked with `{ event, id, payload }` where
// `id` is that listener's eventId — exactly what the real backend passes and
// what `once()` needs (`eventData.id` drives its self-unlisten).
//
// STATE NOTE: the registry lives on the window realm (realmState.ts) — vite
// dev evaluates the mock layer twice per page (pre-existing), so module-level
// state would fork into two disconnected registries.

import { realmSingleton } from './realmState'

/** One `plugin:event|listen` registration: the callback id + its event id. */
interface ListenerEntry {
  callbackId: number
  eventId: number
}

type EventCallback = (data: { event: string; id: number; payload: unknown }) => void

interface BridgeState {
  callbacks: Map<number, EventCallback>
  listeners: Map<string, ListenerEntry[]>
  callbackSeq: number
  eventSeq: number
}

interface BridgeStateBox {
  bridge: BridgeState
}

function loadBridge(): BridgeState {
  return realmSingleton<BridgeStateBox>('__shannonMockEventBridge', () => ({
    bridge: {
      callbacks: new Map<number, EventCallback>(),
      listeners: new Map<string, ListenerEntry[]>(),
      callbackSeq: 1,
      eventSeq: 1,
    },
  })).bridge
}

/** Tauri v2 event-plugin command handlers, to be merged into `handlers`. */
export const eventPluginHandlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  // Real contract (event.js listen): args = { event, target, handler };
  // resolves to an eventId the JS side keeps for unlisten.
  'plugin:event|listen': (args) => {
    const bridge = loadBridge()
    const event = String(args.event)
    const callbackId = Number(args.handler)
    const eventId = bridge.eventSeq++
    const list = bridge.listeners.get(event) ?? []
    list.push({ callbackId, eventId })
    bridge.listeners.set(event, list)
    return eventId
  },
  // Real contract (event.js _unlisten): args = { event, eventId }.
  'plugin:event|unlisten': (args) => {
    const bridge = loadBridge()
    const event = String(args.event)
    const eventId = Number(args.eventId)
    const list = bridge.listeners.get(event)
    if (!list) return null
    const idx = list.findIndex(l => l.eventId === eventId)
    if (idx !== -1) list.splice(idx, 1)
    if (list.length === 0) bridge.listeners.delete(event)
    return null
  },
  // Real contract (event.js emit): args = { event, payload } — the backend
  // fans the payload back out to every listener of that event, INCLUDING
  // the emitting webview. Callback receives { event, id, payload }.
  'plugin:event|emit': (args) => {
    dispatchEvent(String(args.event), args.payload)
    return null
  },
}

/**
 * Fan one event out to every live listener, synchronously, in registration
 * order. Each callback gets its OWN listener id as `data.id` (that's what
 * `once()` uses to self-unlisten).
 */
export function dispatchEvent(event: string, payload: unknown): void {
  const { callbacks, listeners } = loadBridge()
  const entries = listeners.get(event)
  if (!entries) return
  // Snapshot: a handler that unlistens mid-fanout must not corrupt the loop.
  for (const { callbackId, eventId } of [...entries]) {
    const cb = callbacks.get(callbackId)
    if (!cb) continue
    try {
      cb({ event, id: eventId, payload })
    } catch (e) {
      // A listener bug must not break the fanout for the other listeners
      // (real backend isolates panics per callback delivery too).
      console.error(`[mock] listener for "${event}" threw:`, e)
    }
  }
}

/** transformCallback equivalent: register a handler, return its id. */
export function registerEventCallback(callback: EventCallback | null, once = false): number {
  const { callbacks } = loadBridge()
  const id = loadBridge().callbackSeq++
  if (!callback) return id // real transformCallback also mints ids for undefined callbacks
  callbacks.set(id, once
    ? (data) => { callbacks.delete(id); callback(data) }
    : callback)
  return id
}

export function unregisterEventCallback(id: number): void {
  loadBridge().callbacks.delete(id)
}

/**
 * `window.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener` — the JS side
 * calls this BEFORE `plugin:event|unlisten`; it drops the registered
 * callback so a straggling backend emit finds nothing to call.
 */
function unregisterListener(event: string, eventId: number): void {
  const bridge = loadBridge()
  const list = bridge.listeners.get(event)
  if (!list) return
  const entry = list.find(l => l.eventId === eventId)
  if (entry) unregisterEventCallback(entry.callbackId)
}

/**
 * Install `window.__TAURI_INTERNALS__` / `window.__TAURI_EVENT_PLUGIN_INTERNALS__`
 * so the REAL `@tauri-apps/api/event` module works in the browser. Idempotent
 * and non-destructive: if a real Tauri shell (or another mock) already
 * injected internals, we keep theirs instead of clobbering the runtime.
 */
export function installTauriInternals(routeInvoke: (cmd: string, args: Record<string, unknown> | undefined) => Promise<unknown>): void {
  if (typeof window === 'undefined') return
  const w = window as unknown as Record<string, unknown>
  if (w.__TAURI_INTERNALS__ || w.__TAURI_EVENT_PLUGIN_INTERNALS__) return
  w.__TAURI_INTERNALS__ = {
    invoke: (cmd: string, args?: Record<string, unknown>) => routeInvoke(cmd, args),
    transformCallback: (callback?: EventCallback, once?: boolean) => registerEventCallback(callback ?? null, once),
    unregisterCallback: unregisterEventCallback,
    // Exposed for parity with mocks.js / debugging in the console.
    get callbacks() { return loadBridge().callbacks },
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
  }
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener }
}

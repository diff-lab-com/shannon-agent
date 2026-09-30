// companionBridge — Office Wave 3 C3 seam for the companion Quick Capture
// window.
//
// Two legs, two transports:
// 1. CROSS-WINDOW (companion → main): a Tauri event (`emitTo('main', …)`).
//    Window CustomEvents don't cross webviews; Tauri events do. Targeted at
//    `main` only, so session windows never see companion prompts.
// 2. IN-WINDOW (main listener → composer): reuses the Wave 2 composer draft
//    bridge (`pushComposerDraft`, a window CustomEvent the ChatInput listens
//    for). Trust contract unchanged — a capture lands as a DRAFT the user
//    reviews and sends, never auto-sent.
//
// Event name and route mirror `desktop/src/companion_window_commands.rs`
// (`COMPANION_PROMPT_EVENT` / `COMPANION_ROUTE`); the ACL that lets the
// companion webview emit at all lives in
// `desktop/capabilities/companion-window.json`.

import { useEffect, useRef } from 'react'
import { emitTo, listen } from '@tauri-apps/api/event'
import { parseWindowSession } from '@/lib/windowSession'

/**
 * Tauri event carrying `{ text }` from the companion window. Must match
 * `COMPANION_PROMPT_EVENT` in `desktop/src/companion_window_commands.rs`.
 */
export const COMPANION_PROMPT_EVENT = 'shannon:companion-prompt'

/** Companion Send targets the main window only (session windows skip it). */
export const COMPANION_TARGET_WINDOW = 'main'

/**
 * Route the companion window boots with (Rust side: `WebviewUrl::App`).
 * The same path is the standalone fallback route inside the main window.
 */
export const COMPANION_ROUTE = '/companion'

/** Payload shape of {@link COMPANION_PROMPT_EVENT}. */
export interface CompanionPromptPayload {
  text?: unknown
}

/**
 * Emit a quick-capture text to the main window. Resolves once Tauri acks
 * the emit (delivery to the main window's listeners is fire-and-forget by
 * design — the capture is a draft, nothing can be "lost mid-send").
 */
export async function emitCompanionPrompt(text: string): Promise<void> {
  await emitTo(COMPANION_TARGET_WINDOW, COMPANION_PROMPT_EVENT, { text })
}

/**
 * Whether THIS webview is the main window. URL-derived (no Tauri globals)
 * so the predicate stays pure in jsdom: session windows boot with
 * `/?windowSession=<uuid>`, the companion boots `/companion`, the main
 * window is everything else.
 */
export function isMainWindowLocation(
  pathname: string = window.location.pathname,
  search: string = window.location.search,
): boolean {
  return parseWindowSession(search) === null && !pathname.startsWith(COMPANION_ROUTE)
}

/**
 * Subscribe to companion prompts (main-window side).
 *
 * The subscription outlives renders: `onPrompt` is read through a
 * latest-ref, so callers may pass an inline closure over changing state
 * without re-subscribing (one Tauri listener for the hook's lifetime;
 * the async `listen` promise is handled so a listener resolving after
 * unmount is unlistened immediately instead of leaking).
 */
export function useCompanionPromptListener(onPrompt: (text: string) => void): void {
  const onPromptRef = useRef(onPrompt)
  useEffect(() => {
    onPromptRef.current = onPrompt
  })
  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined
    void listen<CompanionPromptPayload>(COMPANION_PROMPT_EVENT, (event) => {
      const text = event.payload?.text
      if (typeof text === 'string') onPromptRef.current(text)
    }).then((dispose) => {
      if (disposed) dispose()
      else unlisten = dispose
    })
    return () => {
      disposed = true
      unlisten?.()
    }
  }, [])
}

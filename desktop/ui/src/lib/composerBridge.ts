// composerBridge — the office Wave 2 seam between surface components and the
// chat composer (B2 v1 PPT outline flow).
//
// Trust contract (same theme as Wave 1): a surface that "generates" content
// NEVER sends a message itself. It pushes a draft into the composer and the
// user reviews + hits send — the model only ever sees what the user
// explicitly approved. Implemented as a plain window CustomEvent so any
// component (dialogs, pages, future entry points) can push without prop
// drilling through the chat tree.

import { useEffect, useRef } from 'react'

/** Window event name carrying `{ detail: { text } }` composer drafts. */
export const COMPOSER_DRAFT_EVENT = 'shannon:composer-draft'

/** Push `text` into the composer as a draft. Never sends. */
export function pushComposerDraft(text: string): void {
  window.dispatchEvent(new CustomEvent(COMPOSER_DRAFT_EVENT, { detail: { text } }))
}

/**
 * Subscribe to composer drafts pushed by {@link pushComposerDraft}.
 *
 * The subscription outlives renders: `onDraft` is read through a latest-ref,
 * so callers may pass an inline closure that closes over changing state
 * without re-subscribing (one listener for the hook's lifetime; cleaned up
 * on unmount).
 */
export function useComposerDraftListener(onDraft: (text: string) => void): void {
  const onDraftRef = useRef(onDraft)
  useEffect(() => {
    onDraftRef.current = onDraft
  })
  useEffect(() => {
    const handler = (e: Event) => {
      const text = (e as CustomEvent<{ text?: unknown }>).detail?.text
      if (typeof text === 'string') onDraftRef.current(text)
    }
    window.addEventListener(COMPOSER_DRAFT_EVENT, handler)
    return () => window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
  }, [])
}

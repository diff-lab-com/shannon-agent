// composerBridge — the office Wave 2 seam between surface components and the
// chat composer (B2 v1 PPT outline flow).
//
// Trust contract (same theme as Wave 1): a surface that "generates" content
// NEVER sends a message itself. It pushes a draft into the composer and the
// user reviews + hits send — the model only ever sees what the user
// explicitly approved. Implemented as a plain window CustomEvent so any
// component (dialogs, pages, future entry points) can push without prop
// drilling through the chat tree.
//
// G5 P0-6 — pending-draft one-shot queue: the composer only exists on /chat,
// so a push from anywhere else (DataSources "add to chat", the companion
// Quick Capture window while the main window is on another route or still
// booting) used to fire the event with nobody listening and vanish. Now a
// push made while no composer is subscribed parks in {@link pendingDrafts}
// and flushes the moment ChatInput mounts (callers navigate to /chat to make
// that happen). This replaces the old 150ms setTimeout bet in App.tsx.

import { useEffect, useRef } from 'react'

/** Window event name carrying `{ detail: { text } }` composer drafts. */
export const COMPOSER_DRAFT_EVENT = 'shannon:composer-draft'

/** Drafts pushed while no composer was mounted; flushed on subscribe. */
const pendingDrafts: string[] = []

/** Whether a `useComposerDraftListener` subscriber is currently mounted. */
let composerSubscribed = false

/** True while the mount flush is draining {@link pendingDrafts} (A-16). */
let flushing = false

function dispatchDraft(text: string): void {
  window.dispatchEvent(new CustomEvent(COMPOSER_DRAFT_EVENT, { detail: { text } }))
}

/**
 * Push `text` into the composer as a draft. Never sends.
 *
 * Delivery is one-shot and FIFO: with the composer subscribed the event
 * lands directly; otherwise the text waits in the pending queue and is
 * delivered exactly once, in push order, when the composer next mounts.
 * A push arriving WHILE the queue drains (a delivered draft synchronously
 * pushing another) is parked behind the still-queued drafts instead of
 * jumping them through the direct path (A-16). Callers that may run while
 * /chat is not mounted should navigate there after pushing (ChatInput
 * flushes the queue on subscribe).
 */
export function pushComposerDraft(text: string): void {
  if (composerSubscribed && !flushing) {
    dispatchDraft(text)
    return
  }
  // Parked (or arriving mid-flush). The raw event still dispatches so
  // non-hook observers keep seeing queued pushes — except mid-flush, where
  // the observer IS the drainer and a re-dispatch would break the order.
  if (!flushing) dispatchDraft(text)
  pendingDrafts.push(text)
}

/**
 * Subscribe to composer drafts pushed by {@link pushComposerDraft}.
 *
 * Mounting a subscription ALSO flushes the pending queue (after this
 * listener is attached, so flushed drafts arrive through the normal event
 * path). ChatInput appends drafts, and the queue drains once, so a flush
 * cannot duplicate text.
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
    // A-16: drain as a FIFO loop with the direct path suppressed for the
    // duration — a draft delivered here may synchronously push another, and
    // that push must queue up behind the remaining drafts, not cut ahead.
    // `composerSubscribed` flips only after the drain, so the parked-push
    // branch above catches everything arriving mid-drain.
    flushing = true
    try {
      while (pendingDrafts.length > 0) {
        const text = pendingDrafts.shift()
        if (text !== undefined) dispatchDraft(text)
      }
    } finally {
      flushing = false
    }
    composerSubscribed = true
    return () => {
      window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
      composerSubscribed = false
    }
  }, [])
}

/**
 * Test seam: drop any queued drafts and clear the subscription flag, so
 * jsdom tests start from a clean bridge state.
 */
export function resetPendingComposerDraftsForTests(): void {
  pendingDrafts.length = 0
  composerSubscribed = false
  flushing = false
}

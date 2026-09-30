// office Wave 2 — composerBridge contract tests (B2).
//
// The bridge is the seam parallel work depends on: pushComposerDraft must
// dispatch the exact event shape ({ detail: { text } }), and the hook must
// deliver pushes without re-subscribing when the callback identity changes.
//
// G5 P0-6 adds the pending-draft one-shot queue: pushes made while no
// composer is subscribed (user on another route, /chat still lazy-loading)
// park in the queue and flush on the next mount instead of vanishing.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import {
  COMPOSER_DRAFT_EVENT,
  pushComposerDraft,
  resetPendingComposerDraftsForTests,
  useComposerDraftListener,
} from '@/lib/composerBridge'

beforeEach(() => {
  resetPendingComposerDraftsForTests()
})

describe('composerBridge', () => {
  it('pushComposerDraft dispatches the COMPOSER_DRAFT_EVENT with { detail: { text } }', () => {
    const seen: Array<{ text: string }> = []
    const handler = (e: Event) => seen.push((e as CustomEvent<{ text: string }>).detail)
    window.addEventListener(COMPOSER_DRAFT_EVENT, handler)
    try {
      pushComposerDraft('hello deck')
      expect(seen).toEqual([{ text: 'hello deck' }])
      expect(COMPOSER_DRAFT_EVENT).toBe('shannon:composer-draft')
    } finally {
      window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
    }
  })

  it('useComposerDraftListener delivers pushed drafts', () => {
    const onDraft = vi.fn()
    renderHook(() => useComposerDraftListener(onDraft))
    act(() => pushComposerDraft('first'))
    expect(onDraft).toHaveBeenCalledTimes(1)
    expect(onDraft).toHaveBeenCalledWith('first')
  })

  it('cleans up on unmount — no delivery afterwards', () => {
    const onDraft = vi.fn()
    const { unmount } = renderHook(() => useComposerDraftListener(onDraft))
    unmount()
    act(() => pushComposerDraft('after unmount'))
    expect(onDraft).not.toHaveBeenCalled()
  })

  it('does NOT re-subscribe when onDraft changes identity (latest-ref semantics)', () => {
    const first = vi.fn()
    const second = vi.fn()
    const { rerender } = renderHook(({ cb }) => useComposerDraftListener(cb), {
      initialProps: { cb: first },
    })
    rerender({ cb: second })
    // One subscription total: a single push reaches exactly the LATEST
    // callback, once. A re-subscription would drop nothing but would churn
    // listeners every render — and a naive closed-over capture would call
    // the stale `first`.
    act(() => pushComposerDraft('once'))
    expect(first).not.toHaveBeenCalled()
    expect(second).toHaveBeenCalledTimes(1)
    expect(second).toHaveBeenCalledWith('once')
  })
})

// ─── G5 P0-6: pending-draft one-shot queue ───

describe('composerBridge pending queue (G5 P0-6)', () => {
  it('holds a push made before any subscriber, then flushes on mount', () => {
    // No composer mounted yet — the "user is on /extensions/datasources"
    // case. The raw event still dispatches (nothing hears it), but the
    // draft must survive in the pending queue.
    act(() => pushComposerDraft('cross-route draft'))

    const onDraft = vi.fn()
    renderHook(() => useComposerDraftListener(onDraft))
    expect(onDraft).toHaveBeenCalledTimes(1)
    expect(onDraft).toHaveBeenCalledWith('cross-route draft')
  })

  it('delivers queued drafts in push order and drains exactly once', () => {
    act(() => pushComposerDraft('first'))
    act(() => pushComposerDraft('second'))

    const onDraft = vi.fn()
    const { unmount } = renderHook(() => useComposerDraftListener(onDraft))
    expect(onDraft.mock.calls.map(c => c[0])).toEqual(['first', 'second'])

    // One-shot: a re-mount (navigating away from /chat and back) must not
    // replay consumed drafts.
    unmount()
    const second = vi.fn()
    renderHook(() => useComposerDraftListener(second))
    expect(second).not.toHaveBeenCalled()
  })

  it('does not queue a push that was already delivered directly', () => {
    const onDraft = vi.fn()
    renderHook(() => useComposerDraftListener(onDraft))

    act(() => pushComposerDraft('live'))
    expect(onDraft).toHaveBeenCalledTimes(1)

    // A second subscriber mounting later (StrictMode remount, route churn)
    // must not receive the already-delivered draft.
    const late = vi.fn()
    renderHook(() => useComposerDraftListener(late))
    expect(late).not.toHaveBeenCalled()
  })

  it('still dispatches the raw event even while queued (event shape intact)', () => {
    const seen: Array<{ text: string }> = []
    const handler = (e: Event) => seen.push((e as CustomEvent<{ text: string }>).detail)
    window.addEventListener(COMPOSER_DRAFT_EVENT, handler)
    try {
      act(() => pushComposerDraft('queued-and-dispatched'))
      expect(seen).toEqual([{ text: 'queued-and-dispatched' }])
    } finally {
      window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
    }
  })
})

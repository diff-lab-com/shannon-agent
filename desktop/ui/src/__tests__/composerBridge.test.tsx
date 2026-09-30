// office Wave 2 — composerBridge contract tests (B2).
//
// The bridge is the seam parallel work depends on: pushComposerDraft must
// dispatch the exact event shape ({ detail: { text } }), and the hook must
// deliver pushes without re-subscribing when the callback identity changes.

import { describe, it, expect, vi } from 'vitest'
import { renderHook, act } from '@testing-library/react'
import { COMPOSER_DRAFT_EVENT, pushComposerDraft, useComposerDraftListener } from '@/lib/composerBridge'

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

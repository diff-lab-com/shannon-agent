// Keyboard navigation for DiffViewer + DiffDialog (P1.1 M3).
//
// Shortcuts (only active when the dialog is open and focus is inside it):
//   j / ArrowDown   → move to next hunk
//   k / ArrowUp     → move to previous hunk
//   a               → accept current hunk
//   r               → reject current hunk
//   u               → mark current hunk undecided
//   Enter           → apply (delegates to onApply)
//
// Esc handling stays in the dialog's own useEffect (independent of diff
// being loaded) so closing the modal always works even mid-fetch.
//
// The hook tracks which hunk id is "current" and exposes setters so the
// hosting dialog can aria-activedescendant the focused hunk. Key handler
// is a no-op when the user is typing in an input/textarea/contenteditable.

import { useEffect, useState, useCallback } from 'react'
import type { Hunk } from '@/lib/diff-merge'

interface UseDiffKeyboardArgs {
  enabled: boolean
  hunks: Hunk[]
  onToggleDecision: (hunkId: string, decision: 'accept' | 'reject' | 'pending') => void
  onApply?: () => void
}

export function useDiffKeyboard({
  enabled,
  hunks,
  onToggleDecision,
  onApply,
}: UseDiffKeyboardArgs) {
  const [currentIdx, setCurrentIdx] = useState(0)

  useEffect(() => {
    if (currentIdx >= hunks.length) setCurrentIdx(0)
  }, [hunks.length, currentIdx])

  const moveBy = useCallback((delta: number) => {
    setCurrentIdx(prev => {
      if (hunks.length === 0) return prev
      const next = (prev + delta + hunks.length) % hunks.length
      return next
    })
  }, [hunks.length])

  useEffect(() => {
    if (!enabled) return
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (target) {
        const tag = target.tagName
        if (tag === 'INPUT' || tag === 'TEXTAREA' || target.isContentEditable) return
      }
      const currentHunk = hunks[currentIdx]
      switch (e.key) {
        case 'j':
        case 'ArrowDown':
          e.preventDefault()
          moveBy(1)
          break
        case 'k':
        case 'ArrowUp':
          e.preventDefault()
          moveBy(-1)
          break
        case 'a':
          if (currentHunk) {
            e.preventDefault()
            onToggleDecision(currentHunk.id, 'accept')
          }
          break
        case 'r':
          if (currentHunk) {
            e.preventDefault()
            onToggleDecision(currentHunk.id, 'reject')
          }
          break
        case 'u':
          if (currentHunk) {
            e.preventDefault()
            onToggleDecision(currentHunk.id, 'pending')
          }
          break
        case 'Enter':
          if (onApply) {
            e.preventDefault()
            onApply()
          }
          break
      }
    }
    document.addEventListener('keydown', handler)
    return () => document.removeEventListener('keydown', handler)
  }, [enabled, hunks, currentIdx, moveBy, onToggleDecision, onApply])

  return {
    currentHunkId: hunks[currentIdx]?.id ?? null,
    setCurrentHunkId: (id: string) => {
      const idx = hunks.findIndex(h => h.id === id)
      if (idx >= 0) setCurrentIdx(idx)
    },
  }
}

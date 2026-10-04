// B1 §4-11 / B1-3 — per-session composer draft storage (lib/composerDraft).
// Storage contract: key format `shannon.draft.<id>`, best-effort on every
// path (corrupted payloads read as null, quota failures write as 'failed',
// clears never throw). Extracted verbatim from pages/Chat.tsx so
// deleteSessionAction can clear a session's draft on delete (B1-3/P1-4).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import {
  clearDraft,
  draftKey,
  readDraft,
  writeDraft,
} from '@/lib/composerDraft'

describe('composerDraft (B1 §4-11)', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('keys drafts under shannon.draft.<sessionId>', () => {
    expect(draftKey('abc')).toBe('shannon.draft.abc')
    writeDraft('s1', 'hello', ['/tmp/a.png'])
    expect(localStorage.getItem('shannon.draft.s1')).not.toBeNull()
  })

  it('round-trips text and attachments through writeDraft/readDraft', () => {
    expect(writeDraft('s1', 'hello', ['a', 'b'])).toBe('saved')
    expect(readDraft('s1')).toEqual({ text: 'hello', attachments: ['a', 'b'] })
  })

  it('reads absent, corrupt, and wrong-shaped drafts as null', () => {
    expect(readDraft('missing')).toBeNull()
    localStorage.setItem('shannon.draft.bad', '{not json')
    expect(readDraft('bad')).toBeNull()
    localStorage.setItem('shannon.draft.wrong', JSON.stringify({ nope: true }))
    expect(readDraft('wrong')).toBeNull()
  })

  it('filters non-string entries out of the attachment list', () => {
    localStorage.setItem(
      'shannon.draft.s1',
      JSON.stringify({ text: 't', attachments: ['ok', 42, null, 'fine'] }),
    )
    expect(readDraft('s1')).toEqual({ text: 't', attachments: ['ok', 'fine'] })
  })

  it('rejects oversized drafts without writing (A-21: caller warns)', () => {
    const huge = 'x'.repeat(64 * 1024 + 1)
    expect(writeDraft('s1', huge, [])).toBe('oversize')
    expect(localStorage.getItem('shannon.draft.s1')).toBeNull()
  })

  it('reports quota failures as failed instead of throwing', () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded')
    })
    expect(writeDraft('s1', 'hello', [])).toBe('failed')
    expect(setItem).toHaveBeenCalled()
  })

  it('clearDraft removes exactly the target key and no-ops when absent', () => {
    writeDraft('s1', 'keep me out', [])
    writeDraft('s2', 'keep me in', [])
    clearDraft('s1')
    expect(localStorage.getItem('shannon.draft.s1')).toBeNull()
    expect(readDraft('s2')).toEqual({ text: 'keep me in', attachments: [] })
    // A missing key (already gone / never written) must not throw.
    expect(() => clearDraft('never-written')).not.toThrow()
  })
})

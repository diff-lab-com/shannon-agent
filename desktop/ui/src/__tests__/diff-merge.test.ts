import { describe, it, expect } from 'vitest'
import { computeHunks, mergeFile, type HunkDecision } from '@/lib/diff-merge'

// All fixtures use explicit \n line endings; the merge output always ends
// with a trailing \n to match how `save_text_file` round-trips through
// fs::write.

describe('computeHunks', () => {
  it('returns no hunks when contents are identical', () => {
    expect(computeHunks('a\nb\nc\n', 'a\nb\nc\n')).toEqual([])
  })

  it('returns no hunks when both are empty', () => {
    expect(computeHunks('', '')).toEqual([])
  })

  it('detects a single-line replacement', () => {
    const hunks = computeHunks('a\nb\nc\n', 'a\nB\nc\n')
    expect(hunks).toHaveLength(1)
    expect(hunks[0].oldStart).toBe(2)
    expect(hunks[0].oldEnd).toBe(2)
    expect(hunks[0].newStart).toBe(2)
    expect(hunks[0].newEnd).toBe(2)
    expect(hunks[0].lines.map(l => l.type)).toEqual(['removed', 'added'])
    expect(hunks[0].lines.map(l => l.text)).toEqual(['b', 'B'])
  })

  it('detects a pure insertion at the end', () => {
    const hunks = computeHunks('a\nb\n', 'a\nb\nc\n')
    expect(hunks).toHaveLength(1)
    // After old line 2 ('b'), before old line 3 (doesn't exist) — empty range.
    expect(hunks[0].oldStart).toBe(3)
    expect(hunks[0].oldEnd).toBe(2)
    expect(hunks[0].newStart).toBe(3)
    expect(hunks[0].newEnd).toBe(3)
    const types = hunks[0].lines.map(l => l.type)
    expect(types).toEqual(['added'])
  })

  it('detects a pure deletion at the start', () => {
    const hunks = computeHunks('a\nb\nc\n', 'b\nc\n')
    expect(hunks).toHaveLength(1)
    expect(hunks[0].oldStart).toBe(1)
    expect(hunks[0].oldEnd).toBe(1)
    // Before new line 1 (doesn't exist) — empty range on new side.
    expect(hunks[0].newStart).toBe(1)
    expect(hunks[0].newEnd).toBe(0)
  })

  it('produces two separate hunks for non-adjacent changes', () => {
    const hunks = computeHunks('a\nb\nc\nd\ne\n', 'A\nb\nc\nD\ne\n')
    expect(hunks).toHaveLength(2)
    expect(hunks[0].oldStart).toBe(1)
    expect(hunks[1].oldStart).toBe(4)
    // IDs must differ — they encode position.
    expect(hunks[0].id).not.toBe(hunks[1].id)
  })

  it('produces stable ids across calls for the same input', () => {
    const a = computeHunks('x\ny\n', 'x\nY\n')
    const b = computeHunks('x\ny\n', 'x\nY\n')
    expect(a[0].id).toBe(b[0].id)
  })

  it('assigns correct old/new line numbers in a mixed hunk', () => {
    const hunks = computeHunks('one\ntwo\nthree\n', 'one\nTWO\nTWO_B\nthree\n')
    expect(hunks).toHaveLength(1)
    const removed = hunks[0].lines.filter(l => l.type === 'removed')
    const added = hunks[0].lines.filter(l => l.type === 'added')
    expect(removed[0].oldLineNo).toBe(2)
    expect(added[0].newLineNo).toBe(2)
    expect(added[1].newLineNo).toBe(3)
  })
})

describe('mergeFile', () => {
  const oldContent = 'a\nb\nc\n'
  const newContent = 'a\nB\nc\n'

  it('returns old content unchanged when no decisions given (all pending)', () => {
    const decisions = new Map<string, HunkDecision>()
    expect(mergeFile(oldContent, newContent, decisions)).toBe('a\nb\nc\n')
  })

  it('returns old content when all hunks are rejected', () => {
    const hunks = computeHunks(oldContent, newContent)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'reject']])
    expect(mergeFile(oldContent, newContent, decisions)).toBe('a\nb\nc\n')
  })

  it('returns new content when all hunks are accepted', () => {
    const hunks = computeHunks(oldContent, newContent)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'accept']])
    expect(mergeFile(oldContent, newContent, decisions)).toBe('a\nB\nc\n')
  })

  it('mixed accept/reject across two hunks applies only the accepted one', () => {
    const oldC = 'a\nb\nc\nd\ne\n'
    const newC = 'A\nb\nc\nD\ne\n'
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(2)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'reject'], // keep old 'a'
      [hunks[1].id, 'accept'], // take new 'D'
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('a\nb\nc\nD\ne\n')
  })

  it('handles pure insertion accepted', () => {
    const oldC = 'a\nb\n'
    const newC = 'a\nb\nc\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'accept']])
    expect(mergeFile(oldC, newC, decisions)).toBe('a\nb\nc\n')
  })

  it('handles pure insertion rejected (drops the new line)', () => {
    const oldC = 'a\nb\n'
    const newC = 'a\nb\nc\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'reject']])
    expect(mergeFile(oldC, newC, decisions)).toBe('a\nb\n')
  })

  it('handles pure deletion accepted', () => {
    const oldC = 'a\nb\nc\n'
    const newC = 'a\nc\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'accept']])
    expect(mergeFile(oldC, newC, decisions)).toBe('a\nc\n')
  })

  it('handles pure deletion rejected (keeps the line)', () => {
    const oldC = 'a\nb\nc\n'
    const newC = 'a\nc\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'reject']])
    expect(mergeFile(oldC, newC, decisions)).toBe('a\nb\nc\n')
  })

  it('empty old + non-empty new with accept yields the new content', () => {
    const decisions = new Map<string, HunkDecision>()
    const hunks = computeHunks('', 'a\nb\n')
    for (const h of hunks) decisions.set(h.id, 'accept')
    expect(mergeFile('', 'a\nb\n', decisions)).toBe('a\nb\n')
  })

  it('preserves trailing newline from the source', () => {
    const decisions = new Map<string, HunkDecision>()
    const hunks = computeHunks('x\n', 'y\n')
    for (const h of hunks) decisions.set(h.id, 'accept')
    expect(mergeFile('x\n', 'y\n', decisions)).toBe('y\n')
  })
})

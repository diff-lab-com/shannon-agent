import { describe, it, expect } from 'vitest'
import { computeHunks, mergeFile, type HunkDecision } from '@/lib/diff-merge'

// All fixtures use explicit \n line endings. Trailing-newline semantics:
// all-accept → new content verbatim, none accepted → old content verbatim,
// mixed → the old file's trailing-newline convention.

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

  // ---- B0 P0-2: identical repeated changes must not share a decision ----

  it('applies independent decisions to two identical changes (one accept, one reject)', () => {
    // The node-reproduced P0-2 case: two `dup→DUP` edits, first rejected,
    // second accepted. The old content-equality lookup always resolved the
    // second hunk to the first hunk's (missing) decision and wrote both.
    const oldC = 'dup\nmid\ndup\n'
    const newC = 'DUP\nmid\nDUP\n'
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(2)
    // Position-addressed ids — the two identical edits can never collide.
    expect(hunks[0].id).not.toBe(hunks[1].id)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'reject'],
      [hunks[1].id, 'accept'],
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('dup\nmid\nDUP\n')
  })

  it('applies independent decisions in the reverse order too', () => {
    const oldC = 'dup\nmid\ndup\n'
    const newC = 'DUP\nmid\nDUP\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'accept'],
      [hunks[1].id, 'reject'],
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('DUP\nmid\ndup\n')
  })

  it('handles adjacent identical double hunks separated by one context line', () => {
    const oldC = 'dup\nctx\ndup\ntail\n'
    const newC = 'NEW\nctx\nNEW\ntail\n'
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(2)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'accept'],
      [hunks[1].id, 'reject'],
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('NEW\nctx\ndup\ntail\n')
  })

  it('treats missing decisions as reject even when a later identical hunk is accepted', () => {
    const oldC = 'dup\nmid\ndup\n'
    const newC = 'DUP\nmid\nDUP\n'
    const hunks = computeHunks(oldC, newC)
    const decisions = new Map<string, HunkDecision>([[hunks[1].id, 'accept']])
    expect(mergeFile(oldC, newC, decisions)).toBe('dup\nmid\nDUP\n')
  })

  // ---- B0 P0-2: trailing-newline semantics (three cases) ----

  it('all-accept keeps the new content verbatim, with trailing newline', () => {
    const hunks = computeHunks('x\ny\n', 'x\nY\n')
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'accept']])
    expect(mergeFile('x\ny\n', 'x\nY\n', decisions)).toBe('x\nY\n')
  })

  it('all-accept keeps the new content verbatim, without trailing newline', () => {
    const oldC = 'x\ny\n'
    const newC = 'x\nY' // real edit + proposal drops the final newline
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(1)
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'accept']])
    expect(mergeFile(oldC, newC, decisions)).toBe('x\nY')
  })

  it('no hunks (trailing-newline-only difference) returns the old content exactly', () => {
    expect(mergeFile('a\nb\n', 'a\nb', new Map())).toBe('a\nb\n')
    expect(mergeFile('a\nb', 'a\nb\n', new Map())).toBe('a\nb')
  })

  it('none-accepted returns the old content exactly, preserving a missing trailing newline', () => {
    const hunks = computeHunks('a\nb', 'a\nB\n')
    const decisions = new Map<string, HunkDecision>([[hunks[0].id, 'reject']])
    expect(mergeFile('a\nb', 'a\nB\n', decisions)).toBe('a\nb')
  })

  it('mixed decisions keep the old file trailing-newline convention', () => {
    const oldC = 'a\nk\nx\nb\n'
    const newC = 'A\nk\nX\nb\n'
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(2)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'accept'],
      [hunks[1].id, 'reject'],
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('A\nk\nx\nb\n')
  })

  it('mixed decisions over an old file without trailing newline never add one', () => {
    const oldC = 'a\nk\nx\nb'
    const newC = 'A\nk\nX\nb\n'
    const hunks = computeHunks(oldC, newC)
    expect(hunks).toHaveLength(2)
    const decisions = new Map<string, HunkDecision>([
      [hunks[0].id, 'accept'],
      [hunks[1].id, 'reject'],
    ])
    expect(mergeFile(oldC, newC, decisions)).toBe('A\nk\nx\nb')
  })
})

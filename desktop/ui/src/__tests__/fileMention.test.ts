// GB P2-10b — @ file-reference helpers (pure logic).

import { describe, it, expect } from 'vitest'
import {
  activeMentionQuery,
  caretAfterMentionInsert,
  filterMentionCandidates,
  flattenFileTree,
  insertMention,
  relativeToWorkingDir,
  subsequenceStart,
} from '@/lib/fileMention'
import type { FileNode } from '@/lib/tauri-api'

describe('activeMentionQuery', () => {
  it('opens at input start', () => {
    expect(activeMentionQuery('@')).toEqual({ token: '', startPos: 0, endPos: 1 })
    expect(activeMentionQuery('@src')).toEqual({ token: 'src', startPos: 0, endPos: 4 })
  })

  it('opens after whitespace', () => {
    expect(activeMentionQuery('look at @src/ap')).toEqual({ token: 'src/ap', startPos: 8, endPos: 15 })
  })

  it('never opens mid-word (emails stay emails)', () => {
    expect(activeMentionQuery('mail me at user@example.com')).toBeNull()
    expect(activeMentionQuery('foo@')).toBeNull()
  })

  it('a second @ ends the token (no nesting)', () => {
    // '@a@b' — the regex requires no @ inside the token, and 'a@b' has one.
    expect(activeMentionQuery('@a@b')).toBeNull()
  })

  it('whitespace inside the token ends the query', () => {
    expect(activeMentionQuery('@src main')).toBeNull()
  })
})

describe('relativeToWorkingDir', () => {
  it('strips the working dir prefix', () => {
    expect(relativeToWorkingDir('/w/src/a.ts', '/w')).toBe('src/a.ts')
    expect(relativeToWorkingDir('/w/a.ts', '/w/')).toBe('a.ts')
  })

  it('keeps foreign paths verbatim', () => {
    expect(relativeToWorkingDir('/etc/hosts', '/w')).toBe('/etc/hosts')
    expect(relativeToWorkingDir('/w/src/a.ts', null)).toBe('/w/src/a.ts')
  })
})

describe('insertMention', () => {
  // 'see the @ap' — the token starts at index 8 (after the space) and the
  // caret sits at 11 (end of 'ap').
  const q = { token: 'ap', startPos: 8, endPos: 11 }

  it('replaces the @token with @relative/path plus one space', () => {
    expect(insertMention('see the @ap file', q, '/w/src/app.ts', '/w'))
      .toBe('see the @src/app.ts file')
    // No trailing whitespace before the caret → the separator space is added.
    expect(insertMention('see the @ap', q, '/w/src/app.ts', '/w'))
      .toBe('see the @src/app.ts ')
  })

  it('caretAfterMentionInsert lands after the inserted space', () => {
    const caret = caretAfterMentionInsert(q, '/w/src/app.ts', '/w')
    expect(caret).toBe('see the @src/app.ts '.length)
  })
})

describe('filterMentionCandidates', () => {
  const paths = [
    '/w/src/main.rs',
    '/w/src/app.tsx',
    '/w/docs/README.md',
    '/w/main.py',
    '/w/assets/logo.svg',
  ]

  it('empty query returns the head of the list (index order)', () => {
    expect(filterMentionCandidates(paths, '')).toEqual(paths.slice(0, 8))
  })

  it('basename prefix beats basename substring beats path substring', () => {
    const ranked = filterMentionCandidates(paths, 'main')
    expect(ranked[0]).toBe('/w/main.py')
    expect(ranked[1]).toBe('/w/src/main.rs')
  })

  it('matches path substrings and is case-insensitive', () => {
    expect(filterMentionCandidates(paths, 'readme')).toEqual(['/w/docs/README.md'])
    expect(filterMentionCandidates(paths, 'src/app')).toEqual(['/w/src/app.tsx'])
  })

  it('subsequence fallback finds fuzzy matches', () => {
    expect(filterMentionCandidates(paths, 'mrs')).toContain('/w/src/main.rs')
  })

  it('caps the result and returns no non-matches', () => {
    const many = Array.from({ length: 30 }, (_, i) => `/w/f${i}.ts`)
    expect(filterMentionCandidates(many, 'f1', 8)).toHaveLength(8)
    expect(filterMentionCandidates(paths, 'zzz')).toEqual([])
  })
})

describe('subsequenceStart', () => {
  it('reports the earliest match start', () => {
    expect(subsequenceStart('main.rs', 'mrs')).toBe(0)
    expect(subsequenceStart('app.tsx', 'mrs')).toBe(-1)
    expect(subsequenceStart('abc', '')).toBe(0)
  })
})

describe('flattenFileTree', () => {
  const tree: FileNode[] = [
    {
      name: 'w', path: '/w', type: 'directory',
      children: [
        { name: 'src', path: '/w/src', type: 'directory', children: [
          { name: 'a.ts', path: '/w/src/a.ts', type: 'file' },
        ] },
        { name: 'node_modules', path: '/w/node_modules', type: 'directory', children: [
          { name: 'x.js', path: '/w/node_modules/x.js', type: 'file' },
        ] },
        { name: 'README.md', path: '/w/README.md', type: 'file' },
      ],
    },
  ]

  it('collects file paths pre-order and prunes ignored dirs', () => {
    expect(flattenFileTree(tree)).toEqual(['/w/src/a.ts', '/w/README.md'])
  })

  it('tolerates a single root node, null, and the cap', () => {
    expect(flattenFileTree({ name: 'f', path: '/f.ts', type: 'file' })).toEqual(['/f.ts'])
    expect(flattenFileTree(null)).toEqual([])
    const many: FileNode[] = Array.from({ length: 20 }, (_, i) => ({ name: `f${i}`, path: `/f${i}`, type: 'file' }))
    expect(flattenFileTree(many, 5)).toHaveLength(5)
  })
})

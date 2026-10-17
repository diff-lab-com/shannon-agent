// GB P2-10b — @ file-reference helpers (pure logic).

import { describe, it, expect } from 'vitest'
import {
  activeMentionQuery,
  caretAfterDirectoryMentionInsert,
  caretAfterMentionInsert,
  filterMentionCandidates,
  flattenFileTree,
  flattenFileTreeDirs,
  insertDirectoryMention,
  insertMention,
  mentionTokensIn,
  relativeToWorkingDir,
  removeMentionToken,
  resolveMentionToken,
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

// ── AI-chat-ux C2/C3/C4 — committed-mention helpers ──────────────────────

describe('mentionTokensIn / resolveMentionToken / removeMentionToken (C2/C3)', () => {
  it('extracts every @token with its exact span, emails untouched', () => {
    expect(mentionTokensIn('@a.ts')).toEqual([{ raw: 'a.ts', start: 0, end: 5 }])
    const spans = mentionTokensIn('see @src/a.ts and mail user@example.com then @b')
    expect(spans.map(t => t.raw)).toEqual(['src/a.ts', 'b'])
    // Spans point AT the `@` and end after the token.
    const text = 'see @src/a.ts'
    expect(text.slice(spans[0].start, spans[0].end)).toBe('@src/a.ts')
  })

  it('resolves relative and verbatim tokens against the candidates', () => {
    const candidates = ['/w/src/a.ts', '/outside/x.md']
    expect(resolveMentionToken('src/a.ts', candidates, '/w')).toBe('/w/src/a.ts')
    expect(resolveMentionToken('/outside/x.md', candidates, '/w')).toBe('/outside/x.md')
    expect(resolveMentionToken('missing.ts', candidates, '/w')).toBeNull()
  })

  it('removeMentionToken splices the @token plus one space per side', () => {
    // Preceding + trailing space both swallowed.
    expect(removeMentionToken('see @a.ts now', { raw: 'a.ts', start: 4, end: 9 })).toBe('see now')
    // Token at start: only the trailing space goes.
    expect(removeMentionToken('@a.ts now', { raw: 'a.ts', start: 0, end: 5 })).toBe('now')
    // No spaces around: clean cut.
    expect(removeMentionToken('@a.ts', { raw: 'a.ts', start: 0, end: 5 })).toBe('')
  })
})

describe('insertDirectoryMention (C4)', () => {
  const query = { token: 'doc', startPos: 0, endPos: 4 }
  it('inserts @dir/ with NO trailing space (the query keeps matching)', () => {
    expect(insertDirectoryMention('@doc', query, '/w/docs', '/w')).toBe('@docs/')
  })
  it('computes the caret inside the trailing slash', () => {
    expect(caretAfterDirectoryMentionInsert(query, '/w/docs', '/w')).toBe(6)
  })
})

describe('flattenFileTreeDirs (C4)', () => {
  const tree: FileNode[] = [
    {
      name: 'w', path: '/w', type: 'directory',
      children: [
        { name: 'src', path: '/w/src', type: 'directory', children: [
          { name: 'a.ts', path: '/w/src/a.ts', type: 'file' },
        ] },
        { name: 'node_modules', path: '/w/node_modules', type: 'directory', children: [] },
      ],
    },
  ]
  it('collects directory paths parent-first and prunes ignored dirs', () => {
    expect(flattenFileTreeDirs(tree)).toEqual(['/w', '/w/src'])
  })
})

describe('filterMentionCandidates recency (C4)', () => {
  it('reorders within a fuzzy band without beating a stronger band', () => {
    const paths = ['/w/old-report.md', '/w/new-report.md', '/w/repotools']
    // new-report is the recent one: inside the same basename-substring band
    // it overtakes old-report…
    const recency = new Map([['/w/new-report.md', 10]])
    expect(filterMentionCandidates(paths, 'report', 8, recency)[0]).toBe('/w/new-report.md')
    // …but recency never lifts a substring hit over a basename PREFIX.
    expect(filterMentionCandidates(paths, 'repo', 8, recency)[0]).toBe('/w/repotools')
  })
  it('empty query sorts by recency', () => {
    const paths = ['/w/a', '/w/b']
    expect(filterMentionCandidates(paths, '', 8, new Map([['/w/b', 5]]))[0]).toBe('/w/b')
  })
})

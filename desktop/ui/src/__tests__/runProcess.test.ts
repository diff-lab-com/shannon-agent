// GB P2-3 — the 「过程四要素」 aggregation state machine (pure reducer).
// These are the event→state transitions the dock's 运行 tab renders.

import { describe, it, expect } from 'vitest'
import {
  beginRun,
  endRun,
  extractMessageRefs,
  noteToolProgress,
  noteToolStart,
} from '@/lib/runProcess'

describe('beginRun', () => {
  it('opens a running turn seeded with @refs and attachments, wiping the previous run', () => {
    const prev = endRun(
      noteToolStart(beginRun({ at: 1, message: 'x' }), 'write_file', { path: '/old' }, 2),
      3,
      false,
    )
    expect(prev.status).toBe('done')
    expect(prev.outputs).toEqual(['/old'])

    const next = beginRun({
      at: 10,
      message: 'please check @src/app.ts and @docs',
      attachments: ['/w/report.pdf'],
    })
    expect(next.status).toBe('running')
    expect(next.startedAt).toBe(10)
    expect(next.sources).toEqual(['src/app.ts', 'docs', '/w/report.pdf'])
    expect(next.outputs).toEqual([]) // previous run's outputs die here (「下一轮开始」)
    expect(next.toolCount).toBe(0)
  })

  it('empty message and no attachments → empty sources, still running', () => {
    const next = beginRun({ at: 5 })
    expect(next.status).toBe('running')
    expect(next.sources).toEqual([])
  })
})

describe('extractMessageRefs', () => {
  it('extracts whitespace-anchored @tokens, ignores emails and CJK punctuation tails', () => {
    expect(extractMessageRefs('see @src/a.ts and mail me@x.com，then @docs/ok。')).toEqual([
      'src/a.ts',
      'docs/ok',
    ])
  })
})

describe('noteToolStart', () => {
  it('read-like tools land their path inputs in sources', () => {
    let state = beginRun({ at: 1 })
    state = noteToolStart(state, 'read_file', { path: '/w/src/main.rs' }, 2)
    expect(state.sources).toContain('/w/src/main.rs')
    expect(state.outputs).toEqual([])
    expect(state.toolCount).toBe(1)
    expect(state.summary).toBe('read_file')
    expect(state.lastTool).toBe('read_file')
  })

  it('write-like tools land their path inputs in outputs', () => {
    let state = beginRun({ at: 1 })
    state = noteToolStart(state, 'write_file', { path: '/w/out/report.md' }, 2)
    expect(state.outputs).toEqual(['/w/out/report.md'])
    expect(state.sources).toEqual([])
  })

  it('unclassified tools with path-carrying input count as sources (never outputs)', () => {
    let state = beginRun({ at: 1 })
    state = noteToolStart(state, 'some_mcp_thing', { file_path: '/w/data.csv' }, 2)
    expect(state.sources).toContain('/w/data.csv')
    expect(state.outputs).toEqual([])
  })

  it('dedupes and caps the lists', () => {
    let state = beginRun({ at: 1 })
    for (let i = 0; i < 60; i++) {
      state = noteToolStart(state, 'read_file', { path: `/w/f${i}` }, 2)
    }
    expect(state.sources.length).toBeLessThanOrEqual(50)
    state = noteToolStart(state, 'read_file', { path: '/w/f0' }, 3)
    expect(state.sources.filter(p => p === '/w/f0')).toHaveLength(1)
  })
})

describe('noteToolProgress', () => {
  it('the latest progress line becomes the one-line summary', () => {
    let state = beginRun({ at: 1 })
    state = noteToolStart(state, 'run_command', { command: 'ls' }, 2)
    state = noteToolProgress(state, 'compiling 42%…')
    expect(state.summary).toBe('compiling 42%…')
    // identical text → same object (no state churn)
    expect(noteToolProgress(state, 'compiling 42%…')).toBe(state)
  })
})

describe('endRun', () => {
  it('settles into done/failed and keeps the aggregated content', () => {
    let state = beginRun({ at: 1, attachments: ['/a'] })
    state = noteToolStart(state, 'write_file', { path: '/b' }, 2)
    state = endRun(state, 9, false)
    expect(state.status).toBe('done')
    expect(state.endedAt).toBe(9)
    expect(state.sources).toEqual(['/a'])
    expect(state.outputs).toEqual(['/b'])

    const failed = endRun({ ...state, status: 'running' }, 10, true)
    expect(failed.status).toBe('failed')
  })

  it('does not resurrect a settled run', () => {
    const done = endRun(beginRun({ at: 1 }), 2, false)
    expect(endRun(done, 3, false)).toBe(done)
  })
})

// D5 方案① — `hadChanges` feeds the post-run 「提交这些改动」 chip rule:
// ≥1 Changes-bucket tool call (the same classifyTool lib/toolGrouping.ts
// uses) qualifies the run. Sticky within a run, reset by beginRun.
describe('hadChanges (D5 方案①)', () => {
  it('a file-mutating tool call marks the run, and the flag survives the settle', () => {
    let state = beginRun({ at: 1 })
    expect(state.hadChanges).toBe(false)
    state = noteToolStart(state, 'read_file', { path: '/w/src/main.rs' }, 2)
    expect(state.hadChanges).toBe(false)
    state = noteToolStart(state, 'edit', { file_path: '/w/src/main.rs' }, 3)
    expect(state.hadChanges).toBe(true)
    state = endRun(state, 9, false)
    expect(state.status).toBe('done')
    expect(state.hadChanges).toBe(true, 'the chip rule reads the settled snapshot')
  })

  it('a read-only run never qualifies', () => {
    let state = beginRun({ at: 1 })
    state = noteToolStart(state, 'read_file', { path: '/w/a' }, 2)
    state = noteToolStart(state, 'grep_search', { pattern: 'x' }, 3)
    state = endRun(state, 9, false)
    expect(state.hadChanges).toBe(false)
  })

  it('the next beginRun resets the flag (chips dismiss on the new send)', () => {
    const changed = noteToolStart(beginRun({ at: 1 }), 'write', { path: '/b' }, 2)
    expect(changed.hadChanges).toBe(true)
    expect(beginRun({ at: 10 }).hadChanges).toBe(false)
  })
})

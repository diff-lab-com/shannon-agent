// Settings R3 T11 (C6) — lib/toolGrouping unit tests: classification (map
// hit / heuristic fallback / all three kinds), the sequence→segments
// projection (adjacent merge, kind split, switch-off passthrough,
// non-groupable interruption), and the localStorage pref read/write.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import {
  classifyTool,
  groupToolSegments,
  normalizeToolName,
  readGroupingPrefs,
  writeGroupingPref,
  setToolReadOnlyMap,
  resetToolReadOnlyMap,
  getToolReadOnlyMapVersion,
  subscribeToolReadOnlyMap,
  ensureToolReadOnlyMap,
  GROUPING_PREF_KEYS,
  type GroupingPrefs,
} from '@/lib/toolGrouping'
import type { ToolCall } from '@/types'

const ALL_ON: GroupingPrefs = { explore: true, terminal: true, changes: true }

function tc(name: string, id = name): ToolCall {
  return { tool_use_id: id, tool_name: name, tool_input: {}, status: 'completed' }
}

describe('normalizeToolName', () => {
  it('folds casing and separators so engine and wire names agree', () => {
    expect(normalizeToolName('Bash')).toBe('bash')
    expect(normalizeToolName('terminal_command')).toBe('terminalcommand')
    expect(normalizeToolName('MultiEdit')).toBe('multiedit')
    expect(normalizeToolName('run-background')).toBe('runbackground')
  })
})

describe('classifyTool — read-only map hit (authoritative)', () => {
  beforeEach(() => resetToolReadOnlyMap())

  it('routes map read-only tools to explore regardless of name', () => {
    setToolReadOnlyMap([
      { name: 'Read', read_only: true },
      { name: 'web_search', read_only: true },
      { name: 'custom_lookup', read_only: true },
    ])
    expect(classifyTool('Read')).toBe('explore')
    expect(classifyTool('web_search')).toBe('explore')
    expect(classifyTool('custom_lookup')).toBe('explore')
  })

  it('routes map non-read-only shell names to terminal', () => {
    setToolReadOnlyMap([
      { name: 'Bash', read_only: false },
      { name: 'PowerShell', read_only: false },
    ])
    expect(classifyTool('Bash')).toBe('terminal')
    expect(classifyTool('PowerShell')).toBe('terminal')
  })

  it('routes map non-read-only non-shell tools to changes (all three kinds covered)', () => {
    setToolReadOnlyMap([{ name: 'Write', read_only: false }])
    expect(classifyTool('Write')).toBe('changes')
  })

  it('keeps a read-only shell variant in explore (terminal is 非只读 only)', () => {
    setToolReadOnlyMap([{ name: 'bash', read_only: true }])
    expect(classifyTool('bash')).toBe('explore')
  })

  it('lookup survives casing/separator drift between map keys and calls', () => {
    setToolReadOnlyMap([{ name: 'write_file', read_only: false }])
    expect(classifyTool('Write_File')).toBe('changes')
  })
})

describe('classifyTool — heuristic fallback (map miss)', () => {
  beforeEach(() => resetToolReadOnlyMap())

  it('FILE_MUTATING names → changes', () => {
    for (const name of ['write_file', 'edit_file', 'apply_patch', 'str_replace_editor', 'replace', 'Write', 'MultiEdit']) {
      expect(classifyTool(name)).toBe('changes')
    }
  })

  it('shell list → terminal', () => {
    for (const name of ['bash', 'shell', 'terminal_command', 'run_command', 'PowerShell', 'Bash']) {
      expect(classifyTool(name)).toBe('terminal')
    }
  })

  it('everything else → explore', () => {
    for (const name of ['Read', 'Grep', 'Glob', 'WebFetch', 'Task', 'mcp__server__custom']) {
      expect(classifyTool(name)).toBe('explore')
    }
  })
})

describe('groupToolSegments — sequence → segments', () => {
  beforeEach(() => resetToolReadOnlyMap())

  it('merges adjacent same-kind explore calls into one group', () => {
    const segs = groupToolSegments(
      [tc('Read', 'a'), tc('Grep', 'b'), tc('Glob', 'c')].map(x => ({ tc: x, groupable: true })),
      ALL_ON,
    )
    expect(segs).toEqual([{ type: 'group', kind: 'explore', items: expect.any(Array) }])
    expect(segs.length).toBe(1)
    expect((segs[0] as { items: ToolCall[] }).items.map(x => x.tool_use_id)).toEqual(['a', 'b', 'c'])
  })

  it('splits runs when the kind changes (explore → terminal → changes)', () => {
    const segs = groupToolSegments(
      [tc('Read', 'a'), tc('Grep', 'a2'), tc('Bash', 'b'), tc('bash', 'b2'), tc('write_file', 'c'), tc('Write', 'c2')].map(
        x => ({ tc: x, groupable: true }),
      ),
      ALL_ON,
    )
    expect(segs.map(s => (s.type === 'group' ? `group:${s.kind}` : 'single'))).toEqual([
      'group:explore',
      'group:terminal',
      'group:changes',
    ])
  })

  it('passes cards through individually when that kind\u2019s switch is OFF', () => {
    const segs = groupToolSegments(
      [tc('Read', 'a'), tc('Grep', 'b'), tc('Bash', 'c'), tc('bash', 'd')].map(x => ({ tc: x, groupable: true })),
      { explore: false, terminal: true, changes: true },
    )
    expect(segs.map(s => (s.type === 'group' ? `group:${s.kind}` : 'single'))).toEqual([
      'single',
      'single',
      'group:terminal',
    ])
  })

  it('a non-groupable unit (special card) breaks the run and renders alone', () => {
    const segs = groupToolSegments(
      [
        { tc: tc('Read', 'a'), groupable: true },
        { tc: tc('agent_spawn', 'sub'), groupable: false },
        { tc: tc('Read', 'b'), groupable: true },
      ],
      ALL_ON,
    )
    // No run reaches size 2 — the subagent block interrupts both sides.
    expect(segs.map(s => (s.type === 'group' ? `group:${s.kind}` : 'single'))).toEqual([
      'single',
      'single',
      'single',
    ])
  })

  it('switch-off kind between two runs lets each remaining kind group around it', () => {
    const segs = groupToolSegments(
      [
        { tc: tc('Read', 'a'), groupable: true },
        { tc: tc('Grep', 'a2'), groupable: true },
        { tc: tc('Bash', 'b'), groupable: true },
        { tc: tc('Grep', 'c'), groupable: true },
        { tc: tc('Glob', 'c2'), groupable: true },
      ],
      { explore: true, terminal: false, changes: true },
    )
    expect(segs.map(s => (s.type === 'group' ? `group:${s.kind}` : 'single'))).toEqual([
      'group:explore',
      'single',
      'group:explore',
    ])
  })

  it('a lone groupable card stays single (no group of one)', () => {
    const segs = groupToolSegments([{ tc: tc('Read', 'a'), groupable: true }], ALL_ON)
    expect(segs).toEqual([{ type: 'single', tc: expect.any(Object) }])
  })

  it('empty sequence → no segments', () => {
    expect(groupToolSegments([], ALL_ON)).toEqual([])
  })
})

describe('grouping prefs (localStorage)', () => {
  beforeEach(() => {
    localStorage.clear()
    resetToolReadOnlyMap()
  })

  it('defaults all three switches ON when storage is empty', () => {
    expect(readGroupingPrefs()).toEqual({ explore: true, terminal: true, changes: true })
  })

  it('writeGroupingPref persists the exact keys and readGroupingPrefs reflects it', () => {
    writeGroupingPref('explore', false)
    writeGroupingPref('changes', false)
    expect(localStorage.getItem(GROUPING_PREF_KEYS.explore)).toBe('false')
    // terminal untouched → no storage entry, read falls back to ON.
    expect(localStorage.getItem(GROUPING_PREF_KEYS.terminal)).toBeNull()
    expect(localStorage.getItem(GROUPING_PREF_KEYS.changes)).toBe('false')
    expect(readGroupingPrefs()).toEqual({ explore: false, terminal: true, changes: false })
  })

  it('garbage stored values fall back to ON', () => {
    localStorage.setItem(GROUPING_PREF_KEYS.terminal, 'yes')
    expect(readGroupingPrefs().terminal).toBe(true)
  })
})

describe('read-only map store', () => {
  beforeEach(async () => {
    localStorage.clear()
    resetToolReadOnlyMap()
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.getTools).mockResolvedValue([])
  })

  it('notifies subscribers and bumps the version on mutation', async () => {
    const seen: number[] = []
    const unsub = subscribeToolReadOnlyMap(() => seen.push(getToolReadOnlyMapVersion()))
    setToolReadOnlyMap([{ name: 'Read', read_only: true }])
    unsub()
    setToolReadOnlyMap([{ name: 'Write', read_only: false }])
    expect(seen.length).toBe(1)
  })

  it('ensureToolReadOnlyMap seeds from getTools once and classifies with it', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.getTools).mockResolvedValue([
      { name: 'mcp__notes__add', description: '', enabled: true, read_only: false },
      { name: 'mcp__notes__list', description: '', enabled: true, read_only: true },
    ])
    await ensureToolReadOnlyMap()
    expect(classifyTool('mcp__notes__add')).toBe('changes')
    expect(classifyTool('mcp__notes__list')).toBe('explore')
    await ensureToolReadOnlyMap()
    expect(vi.mocked(api.getTools).mock.calls.length).toBe(1)
  })

  it('ensureToolReadOnlyMap swallows failures and resets the latch for a retry', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.getTools).mockRejectedValueOnce(new Error('not ready'))
    await ensureToolReadOnlyMap()
    expect(classifyTool('anything_unknown')).toBe('explore')
    vi.mocked(api.getTools).mockResolvedValue([
      { name: 'mcp__x__rm', description: '', enabled: true, read_only: false },
    ])
    await ensureToolReadOnlyMap()
    expect(classifyTool('mcp__x__rm')).toBe('changes')
  })
})

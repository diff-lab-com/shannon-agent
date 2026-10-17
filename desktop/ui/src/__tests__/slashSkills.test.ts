// G1 P0-2.2 — slash-menu skill merge + fallback tests.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { SLASH_COMMANDS, filterSlashCommands } from '@/lib/slash/commands'
import {
  fetchSlashSkills,
  mergeSlashMenu,
  toSlashSkillEntry,
  type SlashSkillEntry,
} from '@/lib/slash/skills'
import * as api from '@/lib/tauri-api'

vi.mock('@/lib/tauri-api', () => ({
  listSkills: vi.fn(),
}))

function skill(name: string, trigger?: string): SlashSkillEntry {
  return {
    name,
    description: `${name} skill`,
    trigger: trigger ?? `/${name}`,
    source: 'User',
  }
}

const commandNames = () => filterSlashCommands('').map(i => i.name)

describe('mergeSlashMenu', () => {
  it('returns only static commands when no skills are installed', () => {
    const items = mergeSlashMenu([], '')
    expect(items.every(i => i.kind === 'command')).toBe(true)
    expect(items.map(i => (i.kind === 'command' ? i.command.name : ''))).toEqual(commandNames())
  })

  it('appends installed skills after the static commands', () => {
    const items = mergeSlashMenu([skill('pdf-reader'), skill('code-review')], '')
    const skills = items.filter(i => i.kind === 'skill')
    expect(skills.map(i => (i.kind === 'skill' ? i.skill.name : ''))).toEqual([
      'pdf-reader',
      'code-review',
    ])
    // Commands stay first.
    expect(items[0].kind).toBe('command')
  })

  it('skills never shadow a static command name or alias', () => {
    const items = mergeSlashMenu([skill('context'), skill('usage-session')], '')
    const skillNames = items
      .filter(i => i.kind === 'skill')
      .map(i => (i.kind === 'skill' ? i.skill.name : ''))
    expect(skillNames).not.toContain('context')
    expect(skillNames).not.toContain('usage-session')
  })

  it('filters skills by query prefix and substring, matching triggers too', () => {
    const skills = [skill('pdf-reader'), skill('jupyter-session'), skill('analyze', '/analyze')]
    const names = (q: string) =>
      mergeSlashMenu(skills, q)
        .filter(i => i.kind === 'skill')
        .map(i => (i.kind === 'skill' ? i.skill.name : ''))

    expect(names('/pdf')).toEqual(['pdf-reader'])
    expect(names('/pdf-')).toEqual(['pdf-reader'])
    // Substring match on the name.
    expect(names('/reader')).toEqual(['pdf-reader'])
    // Full trigger matches the raw query.
    expect(names('/analyze')).toEqual(['analyze'])
    expect(names('/zzz')).toEqual([])
  })

  it('matches skills by in-order subsequence too (H2 P1-1 parity with commands)', () => {
    const skills = [skill('pdf-reader'), skill('jupyter-session')]
    const names = (q: string) =>
      mergeSlashMenu(skills, q)
        .filter(i => i.kind === 'skill')
        .map(i => (i.kind === 'skill' ? i.skill.name : ''))

    // j…r subsequence of "jupyter-session" — no prefix/substring hit.
    expect(names('/jr')).toEqual(['jupyter-session'])

    // A fuzzy command hit (/ct → cost/context/compact via substring and
    // subsequence) never hides the skill half of the merge: commands stay
    // first, the subsequence-matching skill still follows.
    const merged = mergeSlashMenu([skill('corrector')], '/ct')
    expect(merged[0].kind).toBe('command')
    expect(merged.some(i => i.kind === 'skill' && i.skill.name === 'corrector')).toBe(true)
  })

  it('deduplicates skills by name', () => {
    const items = mergeSlashMenu([skill('dup'), skill('dup')], '')
    const skills = items.filter(i => i.kind === 'skill')
    expect(skills).toHaveLength(1)
  })

  it('merges against the live SLASH_COMMANDS table (fallback path)', () => {
    // Even with skills present, the static table is always included.
    const items = mergeSlashMenu([skill('pdf-reader')], '')
    const commandCount = items.filter(i => i.kind === 'command').length
    expect(commandCount).toBe(SLASH_COMMANDS.length)
  })
})

describe('fetchSlashSkills', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('maps backend SkillInfo rows into menu entries', async () => {
    vi.mocked(api.listSkills).mockResolvedValue([
      { name: 'my-skill', description: 'Does things', trigger: '/my-skill', source: 'User', category: null },
    ])
    const { entries, degraded } = await fetchSlashSkills()
    expect(entries).toEqual([
      { name: 'my-skill', description: 'Does things', trigger: '/my-skill', source: 'User' },
    ])
    // A clean fetch is NOT degraded (C1: the menu only announces real
    // fetch failures, never an honest "no skills installed").
    expect(degraded).toBe(false)
  })

  it('falls back to [] + degraded when the backend rejects (static table stays usable)', async () => {
    vi.mocked(api.listSkills).mockRejectedValue(new Error('backend down'))
    const { entries, degraded } = await fetchSlashSkills()
    expect(entries).toEqual([])
    expect(degraded).toBe(true)
    // The merge still yields the full static table.
    expect(mergeSlashMenu(entries, '')).toHaveLength(SLASH_COMMANDS.length)
  })

  it('falls back to [] + degraded on timeout', async () => {
    vi.mocked(api.listSkills).mockImplementation(
      () => new Promise(() => {}), // never settles
    )
    const { entries, degraded } = await fetchSlashSkills(25)
    expect(entries).toEqual([])
    expect(degraded).toBe(true)
  }, 5000)

  it('toSlashSkillEntry repairs a missing trigger', () => {
    expect(toSlashSkillEntry({ name: 'x', description: '', trigger: '', source: '', category: null }).trigger).toBe(
      '/x',
    )
  })
})

import { describe, it, expect, vi } from 'vitest'
import {
  SLASH_COMMANDS,
  filterSlashCommands,
  isSlashQuery,
  parseSlashInput,
  type SlashCommandContext,
} from '@/lib/slash/commands'

function makeCtx(overrides: Partial<SlashCommandContext> = {}): SlashCommandContext {
  return {
    navigate: vi.fn(),
    sessionId: 'sess-1',
    workingDir: '/repo',
    sessions: [],
    createSession: vi.fn().mockResolvedValue(undefined),
    showResult: vi.fn(),
    toastError: vi.fn(),
    t: (id: string) => id,
    ...overrides,
  }
}

describe('slash registry', () => {
  it('exposes only backend-backed or navigation commands', () => {
    const names = SLASH_COMMANDS.map(c => c.name)
    // Desktop counterparts of the REPL session commands.
    for (const n of ['context', 'cost', 'diff', 'export', 'new']) expect(names).toContain(n)
    // Engine-side feature the desktop does not implement yet.
    expect(names).not.toContain('compact')
  })

  it('filterSlashCommands matches prefixes and substrings', () => {
    expect(filterSlashCommands('').map(c => c.name)).toEqual(SLASH_COMMANDS.map(c => c.name))
    expect(filterSlashCommands('/co').map(c => c.name)).toEqual(['context', 'cost'])
    expect(filterSlashCommands('/task').map(c => c.name)).toEqual(['tasks'])
  })

  it('isSlashQuery is single-token only', () => {
    expect(isSlashQuery('/')).toBe(true)
    expect(isSlashQuery('/con')).toBe(true)
    expect(isSlashQuery('/con ')).toBe(true)
    expect(isSlashQuery('/context now')).toBe(false)
    expect(isSlashQuery('hello /world')).toBe(false)
    expect(isSlashQuery('')).toBe(false)
  })

  it('parseSlashInput resolves bare known commands and aliases', () => {
    expect(parseSlashInput('/context')?.name).toBe('context')
    expect(parseSlashInput('  /DIFF  ')?.name).toBe('diff')
    expect(parseSlashInput('/save')?.name).toBe('export')
    expect(parseSlashInput('/clear')?.name).toBe('new')
  })

  it('parseSlashInput returns null for unknown names, paths, and multi-token input', () => {
    // Unknown single tokens are usually pasted absolute paths — plain text.
    expect(parseSlashInput('/usr/local/bin')).toBeNull()
    expect(parseSlashInput('/compact')).toBeNull()
    expect(parseSlashInput('/context now')).toBeNull()
    expect(parseSlashInput('plain question')).toBeNull()
  })
})

describe('slash command execution', () => {
  it('/context without a session shows the needs-session notice', async () => {
    const cmd = parseSlashInput('/context')!
    const ctx = makeCtx({ sessionId: null })
    await cmd.run(ctx)
    expect(ctx.showResult).toHaveBeenCalledWith({ kind: 'error', messageKey: 'slash.needsSession' })
  })

  it('/diff without a working directory reports it instead of calling the backend', async () => {
    const cmd = parseSlashInput('/diff')!
    const ctx = makeCtx({ workingDir: '' })
    await cmd.run(ctx)
    expect(ctx.showResult).toHaveBeenCalledWith({ kind: 'error', messageKey: 'slash.card.diff.noWorkingDir' })
  })

  it('/new starts a session', async () => {
    const cmd = parseSlashInput('/new')!
    const ctx = makeCtx()
    await cmd.run(ctx)
    expect(ctx.createSession).toHaveBeenCalledTimes(1)
  })

  it('navigation commands route', async () => {
    const ctx = makeCtx()
    await parseSlashInput('/tasks')!.run(ctx)
    await parseSlashInput('/memory')!.run(ctx)
    expect(ctx.navigate).toHaveBeenNthCalledWith(1, '/tasks')
    expect(ctx.navigate).toHaveBeenNthCalledWith(2, '/memory')
  })
})

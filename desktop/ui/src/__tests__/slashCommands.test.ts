import { describe, it, expect, vi, beforeEach } from 'vitest'
import {
  SLASH_COMMANDS,
  filterSlashCommands,
  isSlashQuery,
  parseSlashInput,
  type SlashCommandContext,
} from '@/lib/slash/commands'
import * as api from '@/lib/tauri-api'

// The dream / detect-skills entries report through sonner toasts; mock the
// module so assertions don't depend on jsdom rendering.
const toastSuccess = vi.hoisted(() => vi.fn())
const toastInfo = vi.hoisted(() => vi.fn())
vi.mock('sonner', () => ({
  toast: {
    success: toastSuccess,
    info: toastInfo,
    error: vi.fn(),
    message: vi.fn(),
  },
}))

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
    for (const n of ['context', 'cost', 'diff', 'export', 'new', 'compact']) expect(names).toContain(n)
  })

  it('filterSlashCommands matches prefixes and substrings', () => {
    expect(filterSlashCommands('').map(c => c.name)).toEqual(SLASH_COMMANDS.map(c => c.name))
    expect(filterSlashCommands('/co').map(c => c.name)).toEqual(['context', 'cost', 'compact'])
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

  it('/compact calls the chat-slice action and shows the summary', async () => {
    const compact = parseSlashInput('/compact')!
    const ctx = { ...makeCtx(), compactSession: vi.fn().mockResolvedValue({
      performed: true, nothing_to_compact: false,
      original_tokens: 100, compacted_tokens: 20, reduction_ratio: 0.8,
      messages_removed: 3, kept_turns: 1, messages: [],
    }) }
    await compact.run(ctx)
    expect(ctx.compactSession).toHaveBeenCalledWith('sess-1')
    expect(ctx.showResult).toHaveBeenCalledWith(expect.objectContaining({ kind: 'compact' }))
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

describe('/dream and /detect-skills', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('registers both commands and resolves them from bare /name input', () => {
    const names = SLASH_COMMANDS.map(c => c.name)
    expect(names).toContain('dream')
    expect(names).toContain('detect-skills')
    expect(parseSlashInput('/dream')?.name).toBe('dream')
    expect(parseSlashInput('/detect-skills')?.name).toBe('detect-skills')
    // The parser is single-token: /dream with a days arg stays plain text
    // (the backend's days parameter remains for future callers).
    expect(parseSlashInput('/dream 7')).toBeNull()
    expect(filterSlashCommands('/d').map(c => c.name)).toEqual(expect.arrayContaining(['dream', 'detect-skills']))
  })

  it('/dream runs a pass with the default window and toasts the counts', async () => {
    vi.mocked(api.runDreamPass).mockResolvedValue({
      skipped_reason: null,
      scanned_sessions: 3,
      projects: ['web-app'],
      merge_proposed: 2,
      remove_proposed: 1,
      add_proposed: 1,
      candidates_detected: 2,
      candidates_refined: 1,
      proposal_ids: ['proposal-1'],
      report_path: '/tmp/report.md',
      duration_ms: 1200,
    })
    const ctx = makeCtx()
    await parseSlashInput('/dream')!.run(ctx)
    expect(api.runDreamPass).toHaveBeenCalledWith(null)
    expect(toastSuccess).toHaveBeenCalledWith(
      'slash.toast.dream.done',
      expect.objectContaining({ description: 'slash.toast.dream.doneHint' }),
    )
  })

  it('/dream surfaces the skip reason instead of counts', async () => {
    vi.mocked(api.runDreamPass).mockResolvedValue({
      skipped_reason: 'disabled',
      scanned_sessions: 0,
      projects: [],
      merge_proposed: 0,
      remove_proposed: 0,
      add_proposed: 0,
      candidates_detected: 0,
      candidates_refined: 0,
      proposal_ids: [],
      report_path: null,
      duration_ms: 0,
    })
    const ctx = makeCtx()
    await parseSlashInput('/dream')!.run(ctx)
    expect(toastInfo).toHaveBeenCalledWith('slash.toast.dream.disabled')
    expect(toastSuccess).not.toHaveBeenCalled()
  })

  it('/detect-skills calls the backend and toasts the appended count', async () => {
    vi.mocked(api.detectSkillsSlash).mockResolvedValue(2)
    const ctx = makeCtx()
    await parseSlashInput('/detect-skills')!.run(ctx)
    expect(api.detectSkillsSlash).toHaveBeenCalledTimes(1)
    expect(toastSuccess).toHaveBeenCalledWith('slash.toast.detectSkills.done')
  })

  it('reports backend failures through toastError', async () => {
    vi.mocked(api.runDreamPass).mockRejectedValue(new Error('boom'))
    const ctx = makeCtx()
    await parseSlashInput('/dream')!.run(ctx)
    expect(ctx.toastError).toHaveBeenCalledWith('slash.card.error.title', expect.any(Error))
  })
})

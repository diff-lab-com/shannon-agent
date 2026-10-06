// Integration tests: ChatScript seed injection into the mock handlers
// (R1 chat-testing infra, brief §C). When no script is armed every handler
// must behave byte-identically to the pre-scripted defaults; armed, the
// seed answers for list_sessions / get_conversation / get_config /
// get_provider_status / get_session_budget / get_session_usage — and, since
// R3, list_checkpoints / rewind_session (the edit-rewind journey's
// affordances) plus tool_calls and the spentUsd ledger shape.

import { afterEach, describe, expect, it } from 'vitest'
import { handlers } from '@/lib/mock/handlers'
import { MOCK_SESSIONS } from '@/lib/mock/data/core'
import { MOCK_CONFIG } from '@/lib/mock/data/config'
import { recordSeedUserSend, resetRecordedSends, setScriptSeed } from '../seed'
import type { ScriptSeed } from '../schema'

const seed: ScriptSeed = {
  config: { provider: 'openai-compatible', hasKey: false, budgetUsd: 5 },
  sessions: [
    {
      id: 'script-sess-1',
      title: 'Scripted session',
      messages: [
        { role: 'user', content: 'hello' },
        { role: 'assistant', content: 'hi there' },
      ],
    },
    { id: 'script-sess-2', title: 'Second', messages: [] },
  ],
}

const toolSeed: ScriptSeed = {
  config: { budgetUsd: 5, spentUsd: 6.4 },
  sessions: [
    {
      id: 'script-sess-tool',
      title: 'Tooled',
      messages: [
        { role: 'user', content: 'write the file' },
        {
          role: 'assistant',
          content: 'done',
          toolCalls: [
            {
              toolUseId: 'tc-1',
              toolName: 'write_file',
              toolInput: { file_path: '/Users/demo/workspace/todo.md' },
              result: 'wrote 3 lines',
              isError: false,
            },
            {
              toolUseId: 'tc-2',
              toolName: 'Bash',
              toolInput: { command: 'rm -rf /' },
              result: 'denied',
              isError: true,
              meta: { classification: 'sandbox_denied' },
            },
          ],
        },
      ],
    },
    { id: 'script-sess-plain', title: 'Plain', messages: [{ role: 'user', content: 'hi' }] },
  ],
}

afterEach(() => {
  // Handlers are module-level singletons — never leak a seed across tests.
  setScriptSeed(null)
  // S-4 overlay: the recorded send tails are realm-global too.
  resetRecordedSends()
})

describe('unarmed handlers keep the default demo behavior', () => {
  it('list_sessions returns the demo roster', async () => {
    const sessions = await handlers.list_sessions({})
    expect(sessions).toHaveLength(MOCK_SESSIONS.length)
  })

  it('get_conversation returns the demo messages', async () => {
    const messages = await handlers.get_conversation({})
    expect(messages.length).toBeGreaterThan(0)
    expect(messages[0]).toHaveProperty('content')
  })

  it('get_provider_status mirrors the demo roster', async () => {
    const status = await handlers.get_provider_status({})
    expect(status).toMatchObject({ active_provider_id: 'prov-anthropic', has_api_key: true })
  })

  it('get_session_budget falls back to the demo budget (null)', async () => {
    expect(await handlers.get_session_budget({ sessionId: 'script-sess-1' })).toBeNull()
  })

  it('list_checkpoints / rewind_session keep the demo defaults (empty, full demo log)', async () => {
    expect(await handlers.list_checkpoints({ sessionId: 'script-sess-1' })).toEqual([])
    const msgs = await handlers.rewind_session({ sessionId: 'script-sess-1', turnIndex: 0 })
    expect(msgs.length).toBeGreaterThan(0)
    expect(msgs[0]).toHaveProperty('content')
  })
})

describe('armed handlers answer from the script seed', () => {
  it('list_sessions returns the seeded sessions', async () => {
    setScriptSeed(seed)
    const sessions = await handlers.list_sessions({})
    expect(sessions.map((s: { id: string }) => s.id)).toEqual(['script-sess-1', 'script-sess-2'])
    expect(sessions[0]).toMatchObject({ title: 'Scripted session', message_count: 2 })
  })

  it('get_conversation answers with the first seeded session', async () => {
    setScriptSeed(seed)
    const messages = await handlers.get_conversation({})
    expect(messages).toHaveLength(2)
    expect(messages[0]).toMatchObject({ role: 'user', content: 'hello' })
  })

  it('get_config applies the provider swap and strips the key when hasKey:false', async () => {
    setScriptSeed(seed)
    const config = await handlers.get_config({})
    expect(config).toMatchObject({ provider: 'openai-compatible' })
    expect(config.api_key).toBeUndefined()
  })

  it('get_provider_status reports the keyless shape', async () => {
    setScriptSeed(seed)
    const status = await handlers.get_provider_status({})
    expect(status).toMatchObject({
      active_provider_id: 'openai-compatible',
      has_api_key: false,
      display_name: null,
    })
  })

  it('get_session_budget answers the seeded cap for seeded sessions only', async () => {
    setScriptSeed(seed)
    expect(await handlers.get_session_budget({ sessionId: 'script-sess-1' })).toBe(5)
    expect(await handlers.get_session_budget({ sessionId: 'unknown-sess' })).toBeNull()
  })

  it('get_session_usage reports a pristine ledger for seeded sessions', async () => {
    setScriptSeed(seed)
    const usage = await handlers.get_session_usage({ sessionId: 'script-sess-1' })
    expect(usage).toEqual({
      input_tokens: 0,
      output_tokens: 0,
      cache_creation_tokens: 0,
      cache_read_tokens: 0,
      cost_usd: 0,
      events: 2,
    })
    // Non-seeded ids keep the demo data.
    const demo = await handlers.get_session_usage({ sessionId: 'totally-other' })
    expect(demo.cost_usd).toBeGreaterThan(0)
  })

  it('clearing the seed restores every default', async () => {
    setScriptSeed(seed)
    setScriptSeed(null)
    const sessions = await handlers.list_sessions({})
    expect(sessions).toHaveLength(MOCK_SESSIONS.length)
    expect(await handlers.get_session_budget({ sessionId: 'script-sess-1' })).toBeNull()
  })

  // ── R3: tool_calls history, spentUsd ledger, checkpoints, rewind ────────

  it('seeded toolCalls surface as snake_case tool_calls with a derived status', async () => {
    setScriptSeed(toolSeed)
    const messages = await handlers.get_conversation({ sessionId: 'script-sess-tool' })
    const assistant = messages.find(m => m.role === 'assistant')!
    const tcs = assistant.tool_calls!
    expect(tcs).toHaveLength(2)
    expect(tcs[0]).toEqual({
      tool_use_id: 'tc-1',
      tool_name: 'write_file',
      tool_input: { file_path: '/Users/demo/workspace/todo.md' },
      result: 'wrote 3 lines',
      is_error: false,
      status: 'completed',
    })
    expect(tcs[1]).toMatchObject({
      tool_use_id: 'tc-2',
      is_error: true,
      status: 'error',
      meta: { classification: 'sandbox_denied' },
    })
    // load_session / switch_session share the same mapping.
    const loaded = await handlers.load_session({ id: 'script-sess-tool' })
    expect(loaded.find(m => m.role === 'assistant')!.tool_calls).toHaveLength(2)
  })

  it('seeded spentUsd lands on get_session_usage.cost_usd (budget-banner re-derivation)', async () => {
    setScriptSeed(toolSeed)
    const usage = await handlers.get_session_usage({ sessionId: 'script-sess-tool' })
    expect(usage).toMatchObject({ cost_usd: 6.4 })
    // spentUsd is config-level (like budgetUsd): every seeded session's
    // ledger reports the same spend. Without it the ledger stays pristine
    // (events still counted).
    expect((await handlers.get_session_usage({ sessionId: 'script-sess-plain' })).cost_usd).toBe(6.4)
    // The cap+spend combo is exactly the exceeded shape (6.4 ≥ 5).
    expect(await handlers.get_session_budget({ sessionId: 'script-sess-tool' })).toBe(5)
  })

  it('list_checkpoints derives one checkpoint per user turn of the seeded session', async () => {
    setScriptSeed(toolSeed)
    const cps = await handlers.list_checkpoints({ sessionId: 'script-sess-tool' })
    expect(cps).toEqual([
      { turn_index: 0, timestamp: expect.any(Number), description: 'Turn 1', files_changed: [], prompt_preview: 'write the file' },
    ])
    // Unknown ids are NOT seeded — the demo default ([]) applies.
    expect(await handlers.list_checkpoints({ sessionId: 'other' })).toEqual([])
  })

  it('rewind_session truncates the seeded conversation to the checkpoint boundary', async () => {
    setScriptSeed(toolSeed)
    // Turn 0 = truncate before the first user message → empty conversation.
    expect(await handlers.rewind_session({ sessionId: 'script-sess-tool', turnIndex: 0 })).toEqual([])
    // Unknown session id → demo default.
    const demo = await handlers.rewind_session({ sessionId: 'unknown', turnIndex: 0 })
    expect(demo.length).toBeGreaterThan(0)
  })

  // ── S-4 fix (R4 group 3): sent-message durability ────────────────────────

  it('a recorded send survives switch_session / load_session (S-4)', async () => {
    setScriptSeed(seed)
    // The send lands in ITS session's tail (null id = the first seeded
    // session — the scripted "current conversation").
    recordSeedUserSend('script-sess-2', '在途的那条')
    recordSeedUserSend(null, '主窗的当前会话消息')

    for (const read of [
      () => handlers.switch_session({ id: 'script-sess-2' }),
      () => handlers.load_session({ id: 'script-sess-2' }),
    ]) {
      const messages = await read()
      expect(messages).toHaveLength(1)
      expect(messages[0]).toMatchObject({ role: 'user', content: '在途的那条' })
    }
    // The first session's tail is separate (session-scoped, not global).
    const first = await handlers.get_conversation({})
    expect(first.map(m => m.content)).toEqual(['hello', 'hi there', '主窗的当前会话消息'])
  })

  it('a send to an unknown session records nothing (S-4)', async () => {
    setScriptSeed(seed)
    recordSeedUserSend('not-a-seeded-id', '迷路的消息')
    expect(await handlers.switch_session({ id: 'script-sess-2' })).toEqual([])
    // Unarmed (demo) sends are no-ops too.
    setScriptSeed(null)
    recordSeedUserSend('script-sess-1', 'demo 的消息')
    const demo = await handlers.get_conversation({})
    expect(demo.every(m => m.content !== 'demo 的消息')).toBe(true)
  })

  it('rewind_session drops the session\u2019s recorded tail (S-4 consistency)', async () => {
    setScriptSeed(seed)
    recordSeedUserSend('script-sess-2', '将被回滚的尾巴')
    await handlers.rewind_session({ sessionId: 'script-sess-2', turnIndex: 0 })
    expect(await handlers.switch_session({ id: 'script-sess-2' })).toEqual([])
  })

  // ── W2 (journeys #17/#19/#20): session model override, approvalMode, ─────
  //    session-lifecycle mutations, seeded search, save_text_file store.

  it('seeded modelOverride answers get_session_model and the player stamps it on sends', async () => {
    setScriptSeed({
      config: { hasKey: true },
      sessions: [
        { id: 'script-sess-model', title: 'Model', messages: [], modelOverride: { provider: 'anthropic', model: 'claude-haiku-4-5-20251001' } },
        { id: 'script-sess-plain-model', title: 'Plain', messages: [] },
      ],
    })
    expect(await handlers.get_session_model({ sessionId: 'script-sess-model' }))
      .toEqual({ provider: 'anthropic', model: 'claude-haiku-4-5-20251001' })
    // A seeded session without an override keeps the null shape.
    expect(await handlers.get_session_model({ sessionId: 'script-sess-plain-model' })).toBeNull()
    // null sessionId resolves to the FIRST seeded session (the scripted
    // "current conversation" rule).
    expect(await handlers.get_session_model({})).toEqual({ provider: 'anthropic', model: 'claude-haiku-4-5-20251001' })

    // A chip switch updates the registry; a reset drops it (the
    // "back to inheriting" shape the journey asserts through sends).
    await handlers.set_session_model({ sessionId: 'script-sess-plain-model', provider: 'openai', model: 'gpt-5' })
    expect(await handlers.get_session_model({ sessionId: 'script-sess-plain-model' }))
      .toEqual({ provider: 'openai', model: 'gpt-5' })
    await handlers.clear_session_model({ sessionId: 'script-sess-plain-model' })
    expect(await handlers.get_session_model({ sessionId: 'script-sess-plain-model' })).toBeNull()

    // Re-arming the SAME seed restores the seeded override (fresh lifecycle).
    setScriptSeed({
      config: { hasKey: true },
      sessions: [
        { id: 'script-sess-model', title: 'Model', messages: [], modelOverride: { provider: 'anthropic', model: 'claude-haiku-4-5-20251001' } },
      ],
    })
    expect(await handlers.get_session_model({ sessionId: 'script-sess-model' }))
      .toEqual({ provider: 'anthropic', model: 'claude-haiku-4-5-20251001' })
  })

  it('unarmed session-model handlers keep the demo behavior (byte-identical)', async () => {
    // Unarmed: set/get/clear on the demo map, seed registry untouched.
    await handlers.set_session_model({ sessionId: 'demo-x', provider: 'openai', model: 'gpt-5' })
    expect(await handlers.get_session_model({ sessionId: 'demo-x' })).toEqual({ provider: 'openai', model: 'gpt-5' })
    await handlers.clear_session_model({ sessionId: 'demo-x' })
    expect(await handlers.get_session_model({ sessionId: 'demo-x' })).toBeNull()
  })

  it('seeded config.approvalMode lands on get_config.approval_mode (any engine value)', async () => {
    setScriptSeed({ config: { hasKey: true, approvalMode: 'bypass_permissions' }, sessions: [{ id: 's', title: 'S', messages: [] }] })
    expect(await handlers.get_config({})).toMatchObject({ approval_mode: 'bypass_permissions' })
    setScriptSeed(null)
    // Unarmed: the demo default is untouched by the armed branch.
    expect((await handlers.get_config({})).approval_mode).toBe(MOCK_CONFIG.approval_mode)
  })

  it('seeded session lifecycle: rename/delete/archive/restore project into the roster reads', async () => {
    const lifecycleSeed: ScriptSeed = {
      config: { hasKey: true },
      sessions: [
        { id: 'script-sess-life', title: 'Lifecycle', messages: [] },
        { id: 'script-sess-life-2', title: 'Second', messages: [] },
      ],
    }
    setScriptSeed(lifecycleSeed)

    // Rename: the armed list_sessions / search read the new title.
    await handlers.rename_session({ id: 'script-sess-life', title: 'Renamed!' })
    let rows = await handlers.list_sessions({})
    expect(rows.find((r: { id: string }) => r.id === 'script-sess-life')).toMatchObject({ title: 'Renamed!' })
    const hits = await handlers.search_sessions({ query: 'renam' })
    expect(hits.map((h: { id: string }) => h.id)).toEqual(['script-sess-life'])

    // Archive: the row leaves the rail and surfaces in the archived list.
    await handlers.archive_session({ id: 'script-sess-life' })
    rows = await handlers.list_sessions({})
    expect(rows.map((r: { id: string }) => r.id)).toEqual(['script-sess-life-2'])
    expect(await handlers.list_archived_sessions({}))
      .toEqual([expect.objectContaining({ id: 'script-sess-life', title: 'Renamed!' })])

    // Restore: back on the rail, archived list empty again.
    await handlers.unarchive_session({ id: 'script-sess-life' })
    rows = await handlers.list_sessions({})
    expect(rows).toHaveLength(2)
    expect(await handlers.list_archived_sessions({})).toEqual([])

    // Delete: the row leaves for good.
    await handlers.delete_session({ id: 'script-sess-life-2' })
    rows = await handlers.list_sessions({})
    expect(rows.map((r: { id: string }) => r.id)).toEqual(['script-sess-life'])

    // A cleared seed restores every default read.
    setScriptSeed(null)
    expect(await handlers.list_archived_sessions({})).toEqual([])
  })

  it('deleteFails fixture refuses the delete for exactly the marked session', async () => {
    setScriptSeed({
      config: { hasKey: true },
      sessions: [
        { id: 'script-sess-cursed', title: 'Cursed', messages: [], deleteFails: true },
        { id: 'script-sess-fine', title: 'Fine', messages: [] },
      ],
    })
    await expect(handlers.delete_session({ id: 'script-sess-cursed' })).rejects.toThrow(/refused/)
    // The marked row survives; the unmarked one deletes.
    let rows = await handlers.list_sessions({})
    expect(rows.map((r: { id: string }) => r.id)).toEqual(['script-sess-cursed', 'script-sess-fine'])
    await handlers.delete_session({ id: 'script-sess-fine' })
    rows = await handlers.list_sessions({})
    expect(rows.map((r: { id: string }) => r.id)).toEqual(['script-sess-cursed'])
  })

  it('seeded workingDir rides list_sessions.working_dir (the plan-panel journey surface)', async () => {
    setScriptSeed({
      config: { hasKey: true },
      sessions: [{ id: 'script-sess-plan', title: 'Planned', messages: [], workingDir: '/Users/demo/workspace/shannon-demo' }],
    })
    const rows = await handlers.list_sessions({})
    expect(rows[0]).toMatchObject({ working_dir: '/Users/demo/workspace/shannon-demo' })
  })

  it('save_text_file records writes the plan read serves back; the failure fixture rejects', async () => {
    setScriptSeed({
      config: { hasKey: true },
      sessions: [{ id: 'script-sess-plan', title: 'Planned', messages: [], workingDir: '/w/demo' }],
    })
    const planPath = '/w/demo/.shannon/plans/demo-plan.md'
    await handlers.save_text_file({ path: planPath, content: '# Plan: T\nCreated: c\nStatus: pending\n\n- [x] ticked\n' })
    // The written header parses back into the SessionPlan shape.
    expect(await handlers.get_session_plan({ workingDir: '/w/demo' })).toEqual({
      id: 'demo-plan',
      title: 'T',
      status: 'pending',
      created_at: 'c',
      content: '- [x] ticked\n',
    })
    // A different working dir keeps the demo plan.
    expect(await handlers.get_session_plan({ workingDir: '/w/other' })).toMatchObject({ id: 'demo-plan', title: 'Q3 roadmap execution plan' })

    // The failure fixture: the write rejects and NOTHING is recorded.
    // Re-arming the seed is a fresh lifecycle — the earlier write's store is
    // cleared with it, so the DEMO plan (engine truth) answers again.
    setScriptSeed({
      config: { hasKey: true, saveTextFileFails: true },
      sessions: [{ id: 'script-sess-plan', title: 'Planned', messages: [], workingDir: '/w/demo' }],
    })
    await expect(handlers.save_text_file({ path: '/w/demo/.shannon/plans/other.md', content: 'x' })).rejects.toThrow(/failed/)
    expect(await handlers.get_session_plan({ workingDir: '/w/demo' })).toMatchObject({ title: 'Q3 roadmap execution plan' })

    // Unarmed: the demo twin records into the store (demo plan writes now
    // succeed instead of throwing "not available in demo mode").
    setScriptSeed(null)
    await expect(handlers.save_text_file({ path: '/tmp/notes.md', content: 'hello' })).resolves.toBe(true)
  })

  it('an unarmed save keeps get_session_plan on the demo plan', async () => {
    expect(await handlers.get_session_plan({ workingDir: '/w/demo' }))
      .toMatchObject({ id: 'demo-plan', title: 'Q3 roadmap execution plan', status: 'approved' })
    expect(await handlers.get_session_plan({})).toBeNull()
  })
})

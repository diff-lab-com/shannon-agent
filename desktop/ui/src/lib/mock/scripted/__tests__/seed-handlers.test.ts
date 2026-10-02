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
})

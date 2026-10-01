// Integration tests: ChatScript seed injection into the mock handlers
// (R1 chat-testing infra, brief §C). When no script is armed every handler
// must behave byte-identically to the pre-scripted defaults; armed, the
// seed answers for list_sessions / get_conversation / get_config /
// get_provider_status / get_session_budget / get_session_usage.

import { afterEach, describe, expect, it } from 'vitest'
import { handlers } from '@/lib/mock/handlers'
import { MOCK_SESSIONS } from '@/lib/mock/data/core'
import { setScriptSeed } from '../seed'
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

afterEach(() => {
  // Handlers are module-level singletons — never leak a seed across tests.
  setScriptSeed(null)
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
})

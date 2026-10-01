// Unit tests for the ScriptedBackend player (R1 chat-testing infra).
// Pure logic — events are captured through the injected PlayerRuntime and
// time is driven with vitest fake timers; no DOM, no Tauri, no bridge.
//
// Covered (brief §E): schema validation, chunk timing + speed scaling,
// waitFor:'ui' parking/resume, permission-request auto-park +
// respond_permission resume, onCancel branches (default + custom),
// script-exhausted fallback, payload auto-fill (incl. the budget camelCase
// special case), multi-turn sequencing, and reset semantics.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { DEFAULT_CHUNK_DELAY_MS, ScriptPlayer } from '../player'
import type { ScriptSeed } from '../schema'
import { validateScript } from '../schema'

interface CapturedEvent {
  event: string
  payload: Record<string, unknown>
}

function makeHarness() {
  const events: CapturedEvent[] = []
  const seeds: Array<ScriptSeed | null> = []
  const scheduledDelays: number[] = []
  const player = new ScriptPlayer({
    emit: (event, payload) => events.push({ event, payload }),
    onSeed: (seed) => seeds.push(seed),
    schedule: (delayMs, fn) => {
      scheduledDelays.push(delayMs)
      const t = setTimeout(fn, delayMs)
      return () => clearTimeout(t)
    },
  })
  const names = () => events.map(e => e.event)
  return { player, events, seeds, scheduledDelays, names }
}

const baseScript = {
  name: 'test-script',
  turns: [
    { user: 'hi', script: [{ event: 'query:completed' }] },
  ],
}

beforeEach(() => {
  vi.useFakeTimers()
})

afterEach(() => {
  vi.useRealTimers()
})

describe('schema validation', () => {
  it('accepts the minimal happy-path script', () => {
    const result = validateScript(baseScript)
    expect(result.ok).toBe(true)
  })

  it('accepts a full script with seed, chunks, waitFor and onCancel', () => {
    const result = validateScript({
      name: 'full',
      description: 'd',
      seed: {
        config: { provider: 'anthropic', hasKey: true, budgetUsd: 5 },
        sessions: [{ id: 'sess-a', title: 't', messages: [{ role: 'user', content: 'hi' }] }],
      },
      turns: [{
        user: 'u',
        attachments: ['/tmp/a.md'],
        script: [
          { event: 'query:text', chunks: ['a', 'b'], chunkDelayMs: 10 },
          { waitFor: 'ui' },
          { event: 'permission-request', payload: { request_id: 'pr-1' } },
          { event: 'query:completed' },
        ],
      }],
      onCancel: { emit: [{ event: 'query:cancelled' }] },
    })
    expect(result.ok).toBe(true)
  })

  it('rejects missing turns / unknown events / steps with neither event nor waitFor', () => {
    expect(validateScript({ name: 'x' }).ok).toBe(false)
    expect(validateScript({ name: 'x', turns: [] }).ok).toBe(false)
    expect(validateScript({
      name: 'x',
      turns: [{ user: 'u', script: [{ event: 'made-up-event' }] }],
    }).ok).toBe(false)
    expect(validateScript({
      name: 'x',
      turns: [{ user: 'u', script: [{ payload: { a: 1 } }] }],
    }).ok).toBe(false)
  })

  it('accepts the R3 schema surface (toolCalls seed, spentUsd, rejectedAttachments, subagent events)', () => {
    expect(validateScript({
      name: 'r3',
      seed: {
        config: { budgetUsd: 5, spentUsd: 6.4 },
        sessions: [{
          id: 's',
          title: 't',
          messages: [{
            role: 'assistant',
            content: 'done',
            toolCalls: [{ toolUseId: 'tc-1', toolName: 'write_file', toolInput: { file_path: '/tmp/a.md' }, result: 'ok', isError: false }],
          }],
        }],
      },
      turns: [{
        user: 'u',
        attachments: ['/tmp/a.md'],
        rejectedAttachments: [{ path: '/tmp/a.md', reason: 'out_of_working_dir' }],
        script: [
          { event: 'subagent:start', payload: { agentId: 'sa-1', agentName: 'r', team: null } },
          { event: 'subagent:stop', payload: { agentId: 'sa-1' } },
          { event: 'query:completed' },
        ],
      }],
    }).ok).toBe(true)
    // Malformed variants stay rejected.
    expect(validateScript({
      name: 'bad-tool',
      turns: [{ user: 'u', script: [{ event: 'query:completed' }] }],
      seed: { sessions: [{ id: 's', title: 't', messages: [{ role: 'assistant', content: 'x', toolCalls: [{ toolName: 'w' }] }] }] },
    }).ok).toBe(false)
    expect(validateScript({
      name: 'bad-reject',
      turns: [{ user: 'u', rejectedAttachments: [{ path: '/p' }], script: [{ event: 'query:completed' }] }],
    }).ok).toBe(false)
  })

  it('load() refuses invalid scripts without arming', () => {
    const { player, seeds } = makeHarness()
    const result = player.load({ name: 'bad' })
    expect(result.ok).toBe(false)
    expect(result.errors.length).toBeGreaterThan(0)
    expect(player.snapshot().phase).toBe('idle')
    expect(seeds).toEqual([])
  })
})

describe('chunk timing and speed', () => {
  it('emits chunks on the chunkDelayMs cadence, then the next step', async () => {
    const { player, events, names } = makeHarness()
    player.load({
      name: 'chunks',
      turns: [{
        user: 'u',
        script: [
          { event: 'query:text', chunks: ['海', '浪', '🌊'], chunkDelayMs: 100 },
          { event: 'query:completed' },
        ],
      }],
    })
    const resp = player.handleSendMessage({ sessionId: 'sess-1' })
    expect(resp).toEqual({ query_id: 'q-0' })
    // First chunk is immediate.
    expect(names()).toEqual(['query:text'])
    expect(events[0].payload).toMatchObject({ content: '海', query_id: 'q-0', session_id: 'sess-1' })

    await vi.advanceTimersByTimeAsync(100)
    expect(names()).toEqual(['query:text', 'query:text'])
    expect(events[1].payload).toMatchObject({ content: '浪' })

    await vi.advanceTimersByTimeAsync(100)
    // Last chunk emitted → the turn settles synchronously (next step runs).
    expect(names()).toEqual(['query:text', 'query:text', 'query:text', 'query:completed'])
    expect(events[2].payload).toMatchObject({ content: '🌊' })
    expect(player.snapshot().phase).toBe('done')
  })

  it('defaults the chunk gap to 30ms', async () => {
    const { player, names, scheduledDelays } = makeHarness()
    player.load({
      name: 'default-gap',
      turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['1', '2'] }] }],
    })
    player.handleSendMessage({})
    expect(scheduledDelays[0]).toBe(DEFAULT_CHUNK_DELAY_MS)
    await vi.advanceTimersByTimeAsync(DEFAULT_CHUNK_DELAY_MS)
    expect(names()).toEqual(['query:text', 'query:text', 'query:completed'])
  })

  it('speed scales chunk gaps down (2× → half the delay)', async () => {
    const { player, scheduledDelays } = makeHarness()
    player.load({
      name: 'speed',
      turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['1', '2', '3'], chunkDelayMs: 100 }] }],
    })
    // Speed applies when a chunk step starts — set it before the send.
    player.speed = 2
    player.handleSendMessage({})
    expect(scheduledDelays[0]).toBe(50)
    await vi.advanceTimersByTimeAsync(100) // one 50ms gap + one more
    expect(player.snapshot().phase).toBe('done')
  })
})

describe('waitFor: ui parking', () => {
  it('parks at the marker and resumes to the following step', async () => {
    const { player, names } = makeHarness()
    player.load({
      name: 'wait',
      turns: [{
        user: 'u',
        script: [
          { event: 'query:text', chunks: ['a'] },
          { waitFor: 'ui' },
          { event: 'query:usage', payload: { input_tokens: 1, output_tokens: 2, cost_usd: 0.01 } },
          { event: 'query:completed' },
        ],
      }],
    })
    player.handleSendMessage({})
    expect(names()).toEqual(['query:text'])
    expect(player.snapshot().phase).toBe('waitingUi')

    // resume() while parked must be idempotent-safe.
    player.resume()
    expect(names()).toEqual(['query:text', 'query:usage', 'query:completed'])
    expect(player.snapshot().phase).toBe('done')

    // Stray resume with nothing parked is a no-op.
    player.resume()
    expect(names()).toEqual(['query:text', 'query:usage', 'query:completed'])
  })
})

describe('permission-request auto-park + respond_permission resume', () => {
  const permissionScript = {
    name: 'permission',
    turns: [{
      user: 'u',
      script: [
        { event: 'permission-request', payload: { tool: 'Bash', risk: 'high', request_id: 'pr-1', input: { command: 'ls' } } },
        { event: 'query:tool-start', payload: { tool_use_id: 't1', tool_name: 'Bash', tool_input: {} } },
        { event: 'query:completed' },
      ],
    }],
  }

  it('parks automatically after emitting and resumes on respond_permission', () => {
    const { player, events, names } = makeHarness()
    player.load(permissionScript)
    player.handleSendMessage({ sessionId: 'sess-9' })
    expect(names()).toEqual(['permission-request'])
    expect(events[0].payload).toMatchObject({
      tool: 'Bash',
      request_id: 'pr-1',
      query_id: 'q-0',
      session_id: 'sess-9',
    })
    expect(player.snapshot().phase).toBe('waitingPermission')

    const resumed = player.handleRespondPermission({ requestId: 'pr-1', allow: true, note: null, scope: null })
    expect(resumed).toBe(true)
    expect(names()).toEqual(['permission-request', 'query:tool-start', 'query:completed'])
    expect(player.snapshot().phase).toBe('done')
    expect(player.snapshot().permissionLog).toEqual([
      { requestId: 'pr-1', allow: true, note: null, scope: null, at: expect.any(Number) },
    ])
  })

  it('records respond_permission arriving outside a park without resuming', () => {
    const { player, names } = makeHarness()
    player.load(permissionScript)
    expect(player.handleRespondPermission({ requestId: 'stray', allow: false })).toBe(false)
    player.handleSendMessage({})
    expect(names()).toEqual(['permission-request'])
    expect(player.snapshot().permissionLog).toHaveLength(1)
  })

  it('resume() also releases a permission park (console escape hatch)', () => {
    const { player, names } = makeHarness()
    player.load(permissionScript)
    player.handleSendMessage({})
    player.resume()
    expect(names()).toEqual(['permission-request', 'query:tool-start', 'query:completed'])
  })
})

describe('onCancel branches', () => {
  const midStreamScript = (onCancel?: { emit: Array<Record<string, unknown>> }) => ({
    name: 'cancel',
    ...(onCancel ? { onCancel } : {}),
    turns: [{
      user: 'u',
      script: [
        { event: 'query:text', chunks: ['long', 'stream'], chunkDelayMs: 10_000 },
        { event: 'query:completed' },
      ],
    }],
  })

  it('default: emits query:cancelled immediately and settles the turn', () => {
    const { player, events, names } = makeHarness()
    player.load(midStreamScript())
    player.handleSendMessage({ sessionId: 's' })
    expect(player.handleCancelQuery()).toBe(true)
    expect(names()).toEqual(['query:text', 'query:cancelled'])
    expect(events[1].payload).toMatchObject({ query_id: 'q-0', session_id: 's' })
    expect(player.snapshot().phase).toBe('done')
    // Second cancel (idle) is a no-op, like the real backend's taken token.
    expect(player.handleCancelQuery()).toBe(false)
  })

  it('custom onCancel steps replay verbatim (with terminal suppression)', () => {
    const { player, events, names } = makeHarness()
    player.load(midStreamScript({
      emit: [
        { event: 'query:text', chunks: ['partial'] },
        { event: 'query:cancelled', payload: { reason: 'user' } },
      ],
    }))
    player.handleSendMessage({})
    player.handleCancelQuery()
    expect(names()).toEqual(['query:text', 'query:text', 'query:cancelled'])
    expect(events[1].payload).toMatchObject({ content: 'partial' })
    expect(events[2].payload).toMatchObject({ reason: 'user' })
    expect(player.snapshot().phase).toBe('done')
  })

  it('custom onCancel without a terminal event still settles via query:cancelled', () => {
    const { player, names } = makeHarness()
    player.load(midStreamScript({ emit: [{ event: 'query:text', chunks: ['half'] }] }))
    player.handleSendMessage({})
    player.handleCancelQuery()
    expect(names()).toEqual(['query:text', 'query:text', 'query:cancelled'])
  })

  it('cancel while parked at waitFor/permission also works', () => {
    const { player, names } = makeHarness()
    player.load({
      name: 'cancel-parked',
      turns: [{
        user: 'u',
        script: [
          { event: 'permission-request', payload: { request_id: 'pr-1' } },
          { event: 'query:completed' },
        ],
      }],
    })
    player.handleSendMessage({})
    expect(player.snapshot().phase).toBe('waitingPermission')
    expect(player.handleCancelQuery()).toBe(true)
    expect(names()).toEqual(['permission-request', 'query:cancelled'])
  })
})

describe('fallback and multi-turn sequencing', () => {
  it('send_message without a script falls through (null)', () => {
    const { player } = makeHarness()
    expect(player.handleSendMessage({ sessionId: null })).toBeNull()
  })

  it('exhausted script falls back to the default behavior', () => {
    const { player } = makeHarness()
    player.load(baseScript)
    expect(player.handleSendMessage({})).toEqual({ query_id: 'q-0' })
    expect(player.snapshot().phase).toBe('done')
    expect(player.handleSendMessage({})).toBeNull()
  })

  it('send_message responses carry the turn\'s rejected_attachments (R3 journey #8)', () => {
    const { player } = makeHarness()
    player.load({
      name: 'rejected',
      turns: [
        {
          user: 'u',
          rejectedAttachments: [{ path: '/outside/a.md', reason: 'out_of_working_dir' }],
          script: [{ event: 'query:completed' }],
        },
        { user: 'clean', script: [{ event: 'query:completed' }] },
      ],
    })
    const first = player.handleSendMessage({ message: 'u', filePaths: ['/outside/a.md'] })
    expect(first).toEqual({
      query_id: 'q-0',
      rejected_attachments: [{ path: '/outside/a.md', reason: 'out_of_working_dir' }],
    })
    // A turn without refusals returns the bare shape (no empty-array noise).
    expect(player.handleSendMessage({ message: 'clean' })).toEqual({ query_id: 'q-1' })
  })

  it('snapshot().sends logs each scripted send\'s args (R3 bypass/attachment anchors)', () => {
    const { player } = makeHarness()
    player.load({
      name: 'logged',
      turns: [
        { user: 'first', script: [{ event: 'query:completed' }] },
        { user: 'second', script: [{ event: 'query:completed' }] },
      ],
    })
    player.handleSendMessage({ message: 'first', filePaths: ['/a.md'], budgetBypass: true, sessionId: 'sess-a' })
    player.handleSendMessage({ message: 'second' })
    expect(player.snapshot().sends).toEqual([
      { turnIndex: 0, message: 'first', attachments: ['/a.md'], budgetBypass: true, sessionId: 'sess-a' },
      { turnIndex: 1, message: 'second', attachments: null, budgetBypass: false, sessionId: null },
    ])
    // Post-exhaustion sends fall through — never logged.
    player.handleSendMessage({ message: 'ghost' })
    expect(player.snapshot().sends).toHaveLength(2)
    player.reset()
    expect(player.snapshot().sends).toEqual([])
  })

  it('turns consume in order with incrementing query ids', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'multi',
      turns: [
        { user: 'first', script: [{ event: 'query:completed' }] },
        { user: 'second', script: [{ event: 'query:failed', payload: { error: 'boom' } }] },
      ],
    })
    expect(player.handleSendMessage({ sessionId: 'sess-a' })).toEqual({ query_id: 'q-0' })
    expect(player.snapshot().phase).toBe('armed')
    expect(player.handleSendMessage({ sessionId: 'sess-a' })).toEqual({ query_id: 'q-1' })
    expect(events.map(e => e.event)).toEqual(['query:completed', 'query:failed'])
    expect(events[1].payload).toMatchObject({ error: 'boom', query_id: 'q-1' })
    expect(player.snapshot().phase).toBe('done')
  })

  it('auto-completes a turn whose steps lack a terminal event', () => {
    const { player, names } = makeHarness()
    player.load({
      name: 'no-terminal',
      turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['x'] }] }],
    })
    player.handleSendMessage({})
    expect(names()).toEqual(['query:text', 'query:completed'])
  })
})

describe('payload auto-fill', () => {
  it('fills query_id/session_id and lets payload shallow-merge over them', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'fill',
      turns: [{
        user: 'u',
        script: [
          { event: 'query:notice', payload: { kind: 'failover', message: 'switched' } },
          { event: 'query:text', chunks: ['c'], payload: { content: 'OVERRIDE', extra: 1 } },
          { event: 'query:completed' },
        ],
      }],
    })
    player.handleSendMessage({ sessionId: 'sess-z' })
    expect(events[0].payload).toEqual({ query_id: 'q-0', session_id: 'sess-z', kind: 'failover', message: 'switched' })
    expect(events[1].payload).toEqual({ query_id: 'q-0', session_id: 'sess-z', content: 'OVERRIDE', extra: 1 })
  })

  it('budget events use the camelCase shape without query_id', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'budget',
      seed: { config: { budgetUsd: 10 } },
      turns: [{
        user: 'u',
        script: [
          { event: 'budget:warning' },
          { event: 'budget:exceeded', payload: { spentUsd: 11 } },
          { event: 'query:completed' },
        ],
      }],
    })
    player.handleSendMessage({ sessionId: 'sess-b' })
    expect(events[0].payload).toEqual({ sessionId: 'sess-b', spentUsd: 8.4, budgetUsd: 10 })
    expect(events[1].payload).toEqual({ sessionId: 'sess-b', spentUsd: 11, budgetUsd: 10 })
  })

  it('budget events fall back to neutral defaults without a seeded cap', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'budget-default',
      turns: [{ user: 'u', script: [{ event: 'budget:warning' }, { event: 'query:completed' }] }],
    })
    player.handleSendMessage({})
    expect(events[0].payload).toEqual({ sessionId: null, spentUsd: 4.2, budgetUsd: 5 })
  })

  it('subagent:* events ride the auto payload (R3 journey #12) — consumers ignore the extra fields', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'subagent',
      turns: [{
        user: 'u',
        script: [
          { event: 'subagent:start', payload: { agentId: 'sa-1', agentName: 'researcher', team: 'alpha' } },
          { event: 'subagent:stop', payload: { agentId: 'sa-1' } },
          { event: 'query:completed' },
        ],
      }],
    })
    player.handleSendMessage({ sessionId: 'sess-s' })
    expect(events[0]).toMatchObject({
      event: 'subagent:start',
      payload: { agentId: 'sa-1', agentName: 'researcher', team: 'alpha', session_id: 'sess-s' },
    })
    expect(events[1].payload).toMatchObject({ agentId: 'sa-1' })
  })

  it('budget events report the seeded spentUsd (R3 journey #7 over-budget shape)', () => {
    const { player, events } = makeHarness()
    player.load({
      name: 'over-budget',
      seed: { config: { budgetUsd: 5, spentUsd: 6.4 } },
      turns: [{ user: 'u', script: [{ event: 'budget:exceeded' }, { event: 'query:cancelled' }] }],
    })
    player.handleSendMessage({ sessionId: 'sess-b' })
    expect(events[0].payload).toEqual({ sessionId: 'sess-b', spentUsd: 6.4, budgetUsd: 5 })
    // A payload override still wins over the seed.
  })
})

describe('seed and reset semantics', () => {
  it('load arms the seed, reset clears it and restores defaults', () => {
    const { player, seeds } = makeHarness()
    const seed: ScriptSeed = { config: { hasKey: false }, sessions: [{ id: 's1', title: 't', messages: [] }] }
    player.load({ ...baseScript, seed })
    expect(seeds).toEqual([seed])
    expect(player.snapshot().phase).toBe('armed')

    player.speed = 3
    player.reset()
    expect(seeds).toEqual([seed, null])
    expect(player.snapshot().phase).toBe('idle')
    expect(player.snapshot().speed).toBe(1)
    expect(player.handleSendMessage({})).toBeNull()
  })

  it('reset mid-turn stops pending chunk timers', async () => {
    const { player, names } = makeHarness()
    player.load({
      name: 'reset-mid',
      turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['a', 'b'], chunkDelayMs: 5_000 }] }],
    })
    player.handleSendMessage({})
    player.reset()
    await vi.advanceTimersByTimeAsync(10_000)
    expect(names()).toEqual(['query:text'])
    expect(player.handleSendMessage({})).toBeNull()
  })

  it('re-loading mid-turn cancels the in-flight turn', async () => {
    const { player, names } = makeHarness()
    player.load({
      name: 'first',
      turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['a', 'b'], chunkDelayMs: 5_000 }] }],
    })
    player.handleSendMessage({})
    player.load(baseScript)
    await vi.advanceTimersByTimeAsync(10_000)
    expect(names()).toEqual(['query:text'])
    // New script is armed from its first turn.
    expect(player.handleSendMessage({})).toEqual({ query_id: 'q-0' })
  })
})

// R2 chat-testing plan §A — the knownIssue marker anchors tracked bugs in a
// journey: the marked step is SKIPPED (no emission) and annotated via
// console.info; removing the marker (after the fix lands) flips the journey
// and the spec assertions around it.
describe('knownIssue markers (R2 §A)', () => {
  it('accepts the marker in the schema (turn steps and onCancel steps)', () => {
    expect(validateScript({
      name: 'marked',
      turns: [{ user: 'u', script: [{ event: 'query:completed', knownIssue: 'A-1' }] }],
      onCancel: { emit: [{ event: 'query:cancelled', knownIssue: 'A-19' }] },
    }).ok).toBe(true)
    expect(validateScript({
      name: 'bad-marker',
      turns: [{ user: 'u', script: [{ event: 'query:completed', knownIssue: '' }] }],
    }).ok).toBe(false)
  })

  it('skips a marked step, annotates via console.info, and keeps the rest', () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {})
    try {
      const { player, names } = makeHarness()
      player.load({
        name: 'marked',
        turns: [{
          user: 'u',
          script: [
            { event: 'query:text', chunks: ['before'] },
            { event: 'query:text', chunks: ['A-3 翻转后恢复'], knownIssue: 'A-3' },
            { event: 'query:completed' },
          ],
        }],
      })
      player.handleSendMessage({})
      expect(names()).toEqual(['query:text', 'query:completed'])
      expect(info).toHaveBeenCalledWith(expect.stringContaining('knownIssue A-3'))
      expect(info).toHaveBeenCalledWith(expect.stringContaining('skipped'))
    } finally {
      info.mockRestore()
    }
  })

  it('skips a marked onCancel step; the terminal fallback still settles', () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => {})
    try {
      const { player, names } = makeHarness()
      player.load({
        name: 'cancel-marked',
        turns: [{ user: 'u', script: [{ event: 'query:text', chunks: ['half', 'more'], chunkDelayMs: 10_000 }] }],
        onCancel: { emit: [{ event: 'query:cancelled', knownIssue: 'A-19' }] },
      })
      player.handleSendMessage({})
      expect(player.handleCancelQuery()).toBe(true)
      // The marked query:cancelled never emits; the player's settle fallback
      // produces an identical terminal so the journey behavior is unchanged.
      expect(names()).toEqual(['query:text', 'query:cancelled'])
      expect(player.snapshot().phase).toBe('done')
      expect(info).toHaveBeenCalledWith(expect.stringContaining('knownIssue A-19'))
    } finally {
      info.mockRestore()
    }
  })

  it('snapshot() counts scripted send_message consumption in sentTurns', () => {
    const { player } = makeHarness()
    expect(player.snapshot().sentTurns).toBe(0)
    player.load({
      name: 'counted',
      turns: [
        { user: 'one', script: [{ event: 'query:completed' }] },
        { user: 'two', script: [{ event: 'query:completed' }] },
      ],
    })
    player.handleSendMessage({ sessionId: 's' })
    expect(player.snapshot().sentTurns).toBe(1)
    player.handleSendMessage({ sessionId: 's' })
    expect(player.snapshot().sentTurns).toBe(2)
    // Post-exhaustion sends fall through — they are not scripted turns.
    expect(player.handleSendMessage({})).toBeNull()
    expect(player.snapshot().sentTurns).toBe(2)
    player.reset()
    expect(player.snapshot().sentTurns).toBe(0)
  })
})

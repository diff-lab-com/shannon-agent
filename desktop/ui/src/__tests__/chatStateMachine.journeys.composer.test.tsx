// wave-2 composer/input L1 — chatStateMachine.journeys.composer.test.tsx
//
// Independent file (the shared chatStateMachine.scripts.test.tsx stays
// untouched — parallel-PR discipline). Two layers live here:
//
//   1. The state-machine half of the G13 stream-notice contract, replayed
//      through the REAL ScriptPlayer into a REAL AppProvider (captured-listen
//      harness, same paradigm as the shared file): both notice kinds project,
//      survive the run's completion, reset on the session's next send, and
//      the per-session bucket caps at 20.
//   2. The `get_session_git_diff` scripted fixture gate (wave-2 J15): armed
//      seeds with the `diff:<case>` sentinel session id select the canned
//      GitDiffSummary; unarmed/demo and normal seeds keep the historical
//      not-repo default verbatim (CONTRIBUTING iron rule 4).

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { renderHook, act, waitFor } from '@testing-library/react'

import { parse } from 'yaml'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'
import { ScriptPlayer } from '@/lib/mock/scripted/player'
import { handlers } from '@/lib/mock/handlers'
import { setScriptSeed } from '@/lib/mock/scripted/seed'
import { validateScript, type ChatScript } from '@/lib/mock/scripted/schema'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'

// --- captured-listen harness (same paradigm as chatStateMachine.scripts) ---
const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h(payload)
  }
  return { captured, flush }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push((payload) => handler({ payload }))
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
  emit: vi.fn(),
}))

const yamlRoot = resolve(process.cwd(), 'e2e/scripts')

function loadYaml(name: string): ChatScript {
  return parse(readFileSync(resolve(yamlRoot, `${name}.yaml`), 'utf8')) as ChatScript
}

interface Harness {
  result: ReturnType<typeof useApp>
  player: ScriptPlayer
}

const activeHarnesses: Harness[] = []

async function makeHarness(): Promise<Harness> {
  const player = new ScriptPlayer({
    emit: (event, payload) => flush(event, payload),
    onSeed: () => {},
    schedule: (delayMs, fn) => {
      const t = setTimeout(fn, Math.min(delayMs, 60))
      return () => clearTimeout(t)
    },
  })
  const { result } = renderHook(() => useApp(), { wrapper })
  await waitFor(() => expect(result.current.loading).toBe(false))
  await waitFor(() => {
    expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
  })
  await act(async () => { await result.current.createSession() })
  expect(result.current.currentSessionId).toBe(SESSION_A)
  const harness: Harness = { result, player }
  activeHarnesses.push(harness)
  return harness
}

async function sendAndPlay(
  h: Harness,
  script: ChatScript,
  turnIndex: number,
): Promise<void> {
  vi.mocked(api.sendMessage).mockResolvedValue({ query_id: `q-${turnIndex}` })
  await act(async () => {
    await h.result.current.sendMessage(script.turns[turnIndex]!.user)
  })
  await act(async () => {
    expect(h.player.handleSendMessage({ sessionId: SESSION_A })).toEqual({ query_id: `q-${turnIndex}` })
  })
}

async function awaitSettled(h: Harness, timeout = 10_000): Promise<void> {
  await waitFor(() => expect(h.result.current.isQuerying).toBe(false), { timeout })
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

afterEach(() => {
  for (const h of activeHarnesses) h.player.reset()
  activeHarnesses.length = 0
})

describe('L1 state machine — stream notices (G13, stream-notices journey)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    for (const key of Object.keys(captured)) delete captured[key]
    vi.mocked(api.listSessions).mockResolvedValue([])
    vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
    vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1' })
    vi.mocked(api.switchSession).mockResolvedValue([])
  })

  it('both kinds project, survive completion, and reset on the session\'s next send', { timeout: 30_000 }, async () => {
    const script = loadYaml('stream-notices')
    expect(validateScript(script).ok).toBe(true)
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, script, 0)

    // Both kinds land in the visible session's projected slate, in order.
    await waitFor(() => expect(h.result.current.streamNotices).toHaveLength(2))
    expect(h.result.current.streamNotices.map(n => n.kind)).toEqual(['failover', 'key_rotation'])
    expect(h.result.current.streamNotices[0]!.message).toBe('upstream 429 — retried on the fallback model')

    // The run settles normally (notice ≠ error) and the lines survive it.
    await awaitSettled(h)
    expect(h.result.current.streamNotices).toHaveLength(2)

    // The next send resets the slate (the failover lines of turn 1 must not
    // bleed into turn 2's readout).
    await sendAndPlay(h, script, 1)
    await awaitSettled(h)
    await waitFor(() => expect(h.result.current.streamNotices).toHaveLength(0))
  })

  it('the per-session notice bucket caps at 20 (oldest spill out first)', { timeout: 30_000 }, async () => {
    const h = await makeHarness()
    // Raw bridge events with no query_id (old backend shape) always pass the
    // staleness filter — the same surface control.emitNow drives in the e2e.
    act(() => {
      for (let i = 0; i < 22; i++) {
        flush(EVENT_NAMES.QUERY_NOTICE, { kind: 'failover', message: `notice-${String(i).padStart(2, '0')}` })
      }
    })
    await waitFor(() => expect(h.result.current.streamNotices).toHaveLength(20))
    expect(h.result.current.streamNotices[0]!.message).toBe('notice-02')
    expect(h.result.current.streamNotices[19]!.message).toBe('notice-21')
  })
})

// ── wave-2 J15: the get_session_git_diff scripted fixture gate ──────────────

describe('mock handler — get_session_git_diff scripted fixture (J15)', () => {
  afterEach(() => {
    setScriptSeed(null)
  })

  it('keeps the not-repo default when no script is armed (demo path, byte-identical)', async () => {
    setScriptSeed(null)
    await expect(handlers.get_session_git_diff({ workingDir: '/Users/demo/workspace/my-startup' })).resolves.toEqual({
      is_repo: false,
      files: [],
      patch: '',
      truncated: false,
    })
  })

  it('answers the diff:<case> fixture for a sentinel first seeded session', async () => {
    setScriptSeed({
      sessions: [{ id: 'diff:patch', title: 'sentinel', messages: [] }],
    })
    const diff = await handlers.get_session_git_diff({ workingDir: '/Users/demo/workspace/my-startup' })
    expect(diff).toMatchObject({ is_repo: true, truncated: false })
    expect(diff.files).toHaveLength(2)
    expect(diff.patch).toContain('diff --git a/src/main.rs')

    setScriptSeed({
      sessions: [{ id: 'diff:nochanges', title: 'sentinel', messages: [] }],
    })
    await expect(handlers.get_session_git_diff({ workingDir: '/x' })).resolves.toMatchObject({
      is_repo: true,
      files: [],
      truncated: false,
    })
  })

  it('keeps the default for normal seeds and unknown diff: tags', async () => {
    setScriptSeed({
      sessions: [{ id: 'script-sess-slash', title: 'normal', messages: [] }],
    })
    await expect(handlers.get_session_git_diff({ workingDir: '/x' })).resolves.toEqual({
      is_repo: false,
      files: [],
      patch: '',
      truncated: false,
    })
    setScriptSeed({
      sessions: [{ id: 'diff:unknown-tag', title: 'sentinel', messages: [] }],
    })
    await expect(handlers.get_session_git_diff({ workingDir: '/x' })).resolves.toMatchObject({ is_repo: false })
  })
})

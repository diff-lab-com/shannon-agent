// R2 L1 状态机层 — chatStateMachine.scripts.test.tsx
//
// The six core user-journey ChatScripts (e2e/scripts/*.yaml, matrix #1-#6 of
// the chat-testing plan) replayed through the REAL ScriptPlayer into a REAL
// AppProvider via the captured-listen harness — the same event streams the
// browser journeys deliver, asserted at the AppContext projection level
// (streamingText accumulation, tool lifecycle, progress normalization,
// permission pending/clear, error classification, cancel/fail cleanup,
// per-session bucket isolation).
//
// Two-layer parity: each journey has a JSON fixture view of its YAML
// (src/__tests__/fixtures/scripts/<name>.json); the parity describe pins
// YAML↔JSON equality (schema-validated on both sides) so the two layers
// cannot drift apart.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { renderHook, act, waitFor } from '@testing-library/react'

import { parse } from 'yaml'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'
import { ScriptPlayer } from '@/lib/mock/scripted/player'
import { validateScript, type ChatScript } from '@/lib/mock/scripted/schema'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'
const SESSION_B = 'bbbb2222-0000-4000-8000-00000000000b'

// --- captured-listen harness (same paradigm as AppContextStreaming.test) ---
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

// --- script loading: fixtures (JSON) + the YAML they mirror ---

const SCRIPT_NAMES = [
  'first-chat',
  'multi-turn-stream',
  'tool-task-file',
  'approval-allow',
  'approval-deny',
  'auth-error',
  'mid-stream-fail',
  'cancel-text-stream',
  // R3 journeys (#7-#14) + the cancel-matrix scripts (§4.1) + the
  // input-persistence double-session seed (§4.2).
  'budget-exceeded',
  'attachments',
  'queue-steer',
  'edit-rewind',
  'session-switch-race',
  'subagent-run',
  'cross-page',
  'context-panels',
  'input-persistence',
  'cancel-tool-run',
  'cancel-approval-wait',
  'cancel-then-resend',
  'cancel-background',
] as const

type ScriptName = (typeof SCRIPT_NAMES)[number]

// vitest root is the desktop/ui package (vitest.config root), so cwd-anchored
// paths reach both layers (same pattern as i18nCheck.test).
const fixturesRoot = resolve(process.cwd(), 'src/__tests__/fixtures/scripts')
const yamlRoot = resolve(process.cwd(), 'e2e/scripts')

function loadFixture(name: ScriptName): ChatScript {
  return JSON.parse(readFileSync(resolve(fixturesRoot, `${name}.json`), 'utf8')) as ChatScript
}

function loadYaml(name: ScriptName): ChatScript {
  return parse(readFileSync(resolve(yamlRoot, `${name}.yaml`), 'utf8')) as ChatScript
}

// --- the player-to-AppProvider bridge ---

interface Harness {
  result: ReturnType<typeof useApp>
  player: ScriptPlayer
  events: Array<{ event: string; payload: Record<string, unknown> }>
}

// Every harness registers here so afterEach can halt any still-armed player —
// a test that ends mid-turn must not leave chunk timers flushing stale
// SESSION_A events into the NEXT test's provider.
const activeHarnesses: Harness[] = []

/**
 * A ScriptPlayer wired to flush every emission into the captured listeners
 * (i.e. the AppProvider). The scheduler caps real chunk gaps at 60ms — just
 * above the context's ~50ms streaming-projection throttle — so chunked turns
 * play out over macrotasks exactly like the browser journeys: mid-run
 * projections are observable AND the settle paths run for real.
 */
async function makeHarness(): Promise<Harness> {
  const events: Array<{ event: string; payload: Record<string, unknown> }> = []
  const player = new ScriptPlayer({
    emit: (event, payload) => {
      events.push({ event, payload })
      flush(event, payload)
    },
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
  // Materialize the visible session — the scripted player stamps every
  // payload with session_id = SESSION_A, and the context's visible-key
  // routing only projects events whose session matches the current one.
  await act(async () => { await result.current.createSession() })
  expect(result.current.currentSessionId).toBe(SESSION_A)
  const harness: Harness = { result, player, events }
  activeHarnesses.push(harness)
  return harness
}

/** Release waitFor:'ui' parks — the journeys' spec side calls control.resume(). */
async function drainWaits(player: ScriptPlayer): Promise<void> {
  for (let guard = 0; guard < 100 && player.snapshot().phase === 'waitingUi'; guard++) {
    await act(async () => { player.resume() })
  }
}

/**
 * Send one scripted turn: optimistic UI, then the player replays the turn's
 * events over macrotasks. Returns while the turn is still playing — the test
 * drives the waits (and parks) so mid-run projections can be asserted.
 */
async function sendAndPlay(
  h: Harness,
  opts: { text: string; attachments?: string[]; expectedQueryId: string },
): Promise<void> {
  await act(async () => {
    await h.result.current.sendMessage(opts.text, opts.attachments)
  })
  await act(async () => {
    expect(h.player.handleSendMessage({ sessionId: SESSION_A })).toEqual({ query_id: opts.expectedQueryId })
  })
}

/** Wait for a turn to settle (isQuerying falls for the visible session). */
async function awaitSettled(h: Harness, timeout = 10_000): Promise<void> {
  await waitFor(() => expect(h.result.current.isQuerying).toBe(false), { timeout })
}

function textChunksOf(script: ChatScript, turnIndex: number): string[] {
  return script.turns[turnIndex]!
    .script
    .filter(s => s.event === 'query:text' && s.chunks)
    .flatMap(s => s.chunks!)
}

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

afterEach(() => {
  // reset() halts in-flight chunk timers and disarms the player.
  for (const h of activeHarnesses) h.player.reset()
  activeHarnesses.length = 0
})

beforeEach(() => {
  vi.clearAllMocks()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.sendMessage).mockResolvedValue({ query_id: 'q1' })
  vi.mocked(api.switchSession).mockResolvedValue([])
})

// ─────────────────────────── YAML ↔ JSON parity ───────────────────────────

describe('ChatScript fixtures — YAML ↔ JSON two-layer parity (R2 §C)', () => {
  for (const name of SCRIPT_NAMES) {
    it(`${name}: fixture mirrors the YAML exactly and both validate`, () => {
      const yaml = loadYaml(name)
      const json = loadFixture(name)
      // Both views pass the SAME ajv schema the runtime arms against.
      expect(validateScript(yaml).ok).toBe(true)
      expect(validateScript(json).ok).toBe(true)
      // Key-field diff is empty — deep equality of the whole script.
      expect(json).toEqual(yaml)
    })
  }

  it('knownIssue markers survive the YAML→JSON round-trip (A-3 / A-19 anchors)', () => {
    const fail = loadFixture('mid-stream-fail')
    expect(fail.turns[1]!.script[0]!.knownIssue).toBe('A-3')
    const cancel = loadFixture('cancel-text-stream')
    expect(cancel.onCancel?.emit[0]!.knownIssue).toBe('A-19')
  })
})

// ───────────────────────────── the six journeys ─────────────────────────────

describe('L1 state machine — first-chat (journey #1)', () => {
  it('streams chunks into streamingText, announces usage, commits on completed', async () => {
    const script = loadFixture('first-chat')
    const chunks = textChunksOf(script, 0)
    expect(chunks.length).toBeGreaterThanOrEqual(5) // ≥5 mixed chunks, per the journey spec
    const reply = chunks.join('')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    expect(h.result.current.isQuerying).toBe(true)

    // The streaming projection accumulates the chunks (throttled ~50ms; the
    // harness caps gaps at 60ms so the projections lag the bucket by at most
    // one chunk — mid-stream the visible text is a growing prefix).
    await waitFor(
      () => expect(h.result.current.streamingText).toContain(chunks[0]! + chunks[1]!),
      { timeout: 10_000 },
    )
    // The usage event landed on the visible session's readout (it rides the
    // turn's tail, right before completion — poll for it).
    await waitFor(() => expect(h.result.current.usage?.cost_usd).toBe(0.0018))
    expect(h.result.current.error).toBeNull()

    // The script's own query:completed commits the bubble and resets.
    await awaitSettled(h)
    expect(h.result.current.streamingText).toBe('')
    const assistants = h.result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1)
    expect(assistants[0]!.content).toBe(reply)
  })
})

describe('L1 state machine — multi-turn-stream (journey #2)', () => {
  it('commits 60 chunks losslessly; the second send increments the query id; buckets never mix', { timeout: 30_000 }, async () => {
    const script = loadFixture('multi-turn-stream')
    expect(textChunksOf(script, 0).length).toBeGreaterThanOrEqual(60)
    const reply1 = textChunksOf(script, 0).join('')
    const reply2 = textChunksOf(script, 1).join('')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    // Accumulation: the projection grows over the 60-chunk stream (a leading
    // slice of the reply is visible mid-run; losslessness is pinned on the
    // committed bubble below, since completion cancels the tail projection).
    await waitFor(
      () => expect(h.result.current.streamingText.length).toBeGreaterThan(200),
      { timeout: 15_000 },
    )
    // A background session's tokens interleave on the wire but never reach
    // the visible projection (per-session buckets, §P2-18).
    const visibleAtLeak = h.result.current.streamingText
    act(() => {
      flush(EVENT_NAMES.QUERY_TEXT, { content: 'B-leak', session_id: SESSION_B })
    })
    // The leak never mixed in, and the visible text stays a pure prefix of
    // the reply (the projection trails the bucket mid-run).
    expect(h.result.current.streamingText).toBe(visibleAtLeak)
    expect(visibleAtLeak).not.toContain('B-leak')
    expect(reply1.startsWith(visibleAtLeak)).toBe(true)

    await awaitSettled(h, 20_000)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')[0]!.content).toBe(reply1)
    expect(h.result.current.streamingText).toBe('')

    // Second turn — the q-id increments (q-0 → q-1), the same increment the
    // E2E pins through the player snapshot's turnIndex.
    await sendAndPlay(h, { text: script.turns[1]!.user, expectedQueryId: 'q-1' })
    // The tail projection is cancelled by completion (the committed bubble
    // below pins losslessness), so containment anchors on the first chunk.
    await waitFor(
      () => expect(h.result.current.streamingText).toContain(textChunksOf(script, 1)[0]!),
      { timeout: 10_000 },
    )
    await awaitSettled(h)
    const assistants = h.result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants.map(m => m.content)).toEqual([reply1, reply2])
    expect(h.player.snapshot().sentTurns).toBe(2)
  })
})

describe('L1 state machine — tool-task-file (journey #3)', () => {
  it('tracks the tool lifecycle, normalizes progress to 0-100 and settles clean', async () => {
    const script = loadFixture('tool-task-file')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })

    // tool-start: the card is live in its running form.
    await waitFor(() => expect(h.result.current.activeToolCalls).toHaveLength(1))
    const live = h.result.current.activeToolCalls[0]!
    expect(live).toMatchObject({
      tool_use_id: 'tool-todo-1',
      tool_name: 'Bash',
      status: 'running',
    })
    expect(live.tool_input).toEqual({ command: "printf 'buy milk\\nwrite tests\\n' > todo.md" })

    // First progress event (the script parks after it): the backend fraction
    // 0.4 normalized to 40% (P2-19 single normalization point).
    await waitFor(() => expect(h.result.current.toolProgress).toEqual({ progress: 40, message: 'writing todo.md' }))
    await drainWaits(h.player) // release the waitFor:'ui' park

    // Second progress event: 0.8 → 80% + its own message.
    await waitFor(() => expect(h.result.current.toolProgress).toEqual({ progress: 80, message: 'flushing buffer' }))

    // tool-result (ok): the card converges to the completed form.
    await waitFor(() => expect(h.result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tool-todo-1',
      status: 'completed',
      is_error: false,
      result: 'wrote 2 lines to todo.md',
    }))

    // Completion: cards leave with the run (P2-4), progress clears, reply
    // commits with ONLY the text chunks (tool payloads are not reply text).
    await awaitSettled(h)
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.toolProgress).toBeNull()
    const reply = textChunksOf(script, 0).join('')
    expect(h.result.current.messages.filter(m => m.role === 'assistant')[0]!.content).toBe(reply)
  })
})

describe('L1 state machine — approval journeys (#4)', () => {
  it('allow: permissionRequest pends, respondPermission clears it and the run resumes', async () => {
    const script = loadFixture('approval-allow')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    // The player parked itself after emitting; the app holds the request.
    await waitFor(() => expect(h.player.snapshot().phase).toBe('waitingPermission'))
    expect(h.result.current.permissionRequest).toMatchObject({
      request_id: 'pr-allow-1',
      tool: 'Bash',
      risk: 'high',
    })

    // The user allows: the app calls respond_permission, clears the prompt…
    await act(async () => { await h.result.current.respondPermission('pr-allow-1', true) })
    expect(h.result.current.permissionRequest).toBeNull()
    expect(api.sendMessage).toBeTruthy()
    expect(api.respondPermission).toHaveBeenCalledWith('pr-allow-1', true, undefined)
    // …and the player resumes into the tool run, parking at the scripted
    // waitFor right after the ok tool-result.
    await act(async () => {
      expect(h.player.handleRespondPermission({ requestId: 'pr-allow-1', allow: true })).toBe(true)
    })
    expect(h.player.snapshot().phase).toBe('waitingUi')
    expect(h.result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tool-ls-1',
      status: 'completed',
      is_error: false,
    })
    expect(h.player.snapshot().permissionLog).toEqual([
      expect.objectContaining({ requestId: 'pr-allow-1', allow: true }),
    ])
    await drainWaits(h.player)

    await awaitSettled(h)
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(1)
  })

  it('deny: the tool settles into the error form and the session stays usable', async () => {
    const script = loadFixture('approval-deny')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.player.snapshot().phase).toBe('waitingPermission'))
    expect(h.result.current.permissionRequest).toMatchObject({ request_id: 'pr-deny-1', risk: 'critical' })

    await act(async () => { await h.result.current.respondPermission('pr-deny-1', false) })
    expect(h.result.current.permissionRequest).toBeNull()
    expect(api.respondPermission).toHaveBeenCalledWith('pr-deny-1', false, undefined)
    await act(async () => {
      expect(h.player.handleRespondPermission({ requestId: 'pr-deny-1', allow: false })).toBe(true)
    })
    // The resume replays tool-start → tool-result(error) synchronously and
    // parks at the scripted waitFor — the error-form card is observable now,
    // before the park release settles the run.
    expect(h.player.snapshot().phase).toBe('waitingUi')
    // The denied tool lands as an error-form card…
    expect(h.result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tool-rm-1',
      status: 'error',
      is_error: true,
      result: 'Permission denied: user rejected the Bash command',
    })
    await drainWaits(h.player)
    // …but the SESSION never enters the error state (a denial is a tool
    // error, not a failed run).
    expect(h.result.current.error).toBeNull()
    await awaitSettled(h)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(1)
  })
})

describe('L1 state machine — failure journeys (#5)', () => {
  it('auth-error classifies errorKind "auth" and cleans the stream', async () => {
    const script = loadFixture('auth-error')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.error).toBe('Authentication failed: invalid x-api-key (HTTP 401)'))
    expect(h.result.current.errorKind).toBe('auth')
    // B0 P1-2: no ghost bubble, no residual stream state.
    expect(h.result.current.streamingText).toBe('')
    expect(h.result.current.isQuerying).toBe(false)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
  })

  it('mid-stream-fail: "other" classification + retry resends TEXT only (A-3 anchored)', async () => {
    const script = loadFixture('mid-stream-fail')
    const h = await makeHarness()
    h.player.load(script)

    // The original turn goes out WITH its attachment path…
    await sendAndPlay(h, {
      text: script.turns[0]!.user,
      attachments: script.turns[0]!.attachments,
      expectedQueryId: 'q-0',
    })
    expect(api.sendMessage).toHaveBeenLastCalledWith(
      script.turns[0]!.user,
      ['/Users/demo/Downloads/story-notes.md'],
      undefined,
      SESSION_A,
    )
    await waitFor(() => expect(h.result.current.error).toBe('upstream connection reset while streaming'))
    expect(h.result.current.errorKind).toBe('other')
    expect(h.result.current.streamingText).toBe('')
    expect(h.result.current.isQuerying).toBe(false)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
    expect(h.player.snapshot().sentTurns).toBe(1)

    // Retry — exactly what ComposerRetryButton does: sendMessage(lastUser
    // content) with NO attachments. A-3 current behavior: the attachment is
    // DROPPED. Flip this block to "attachments preserved" when R4 lands and
    // remove the knownIssue marker on the retry turn in the YAML.
    await act(async () => { await h.result.current.sendMessage(script.turns[0]!.user) })
    expect(api.sendMessage).toHaveBeenLastCalledWith(
      script.turns[0]!.user,
      undefined,
      undefined,
      SESSION_A,
    )
    await act(async () => {
      expect(h.player.handleSendMessage({ sessionId: SESSION_A })).toEqual({ query_id: 'q-1' })
    })
    await drainWaits(h.player)
    // The retry turn is consumed (its knownIssue'd chunk step is skipped —
    // the player annotates and settles it immediately).
    expect(h.player.snapshot().sentTurns).toBe(2)
    expect(h.result.current.error).toBeNull()
  })
})

describe('L1 state machine — cancel-text-stream (journey #6)', () => {
  it('cancel settles via query:cancelled and discards the partial text (A-19 anchored)', async () => {
    const script = loadFixture('cancel-text-stream')
    const h = await makeHarness()
    h.player.load(script)

    // Park at step 1 (after the chunk step) so the stream is still live when
    // the user stops — mirroring the 5s chunk gaps the browser journey uses.
    h.player.pauseAt(1)
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.streamingText).toBe(textChunksOf(script, 0).join('')))
    expect(h.player.snapshot().phase).toBe('waitingUi')

    // Stop — the player's onCancel path (its only step is knownIssue'd and
    // skipped, so the settle fallback emits the identical query:cancelled).
    await act(async () => { await h.result.current.cancelQuery() })
    await act(async () => { expect(h.player.handleCancelQuery()).toBe(true) })

    await awaitSettled(h)
    // A-19 current behavior: the half-streamed text is DISCARDED (B0 P1-2
    // ghost-bubble cleanup). Flip to "partial text commits as the assistant
    // bubble" when R4 lands and remove the knownIssue marker in the YAML.
    expect(h.result.current.streamingText).toBe('')
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
    expect(h.result.current.error).toBeNull()
    expect(h.player.snapshot().phase).toBe('done')
  })
})

// ───────────────────────── R3 journeys (#7 – #14) ─────────────────────────

describe('L1 state machine — budget-exceeded (journey #7)', () => {
  it('budget:exceeded auto-cancels the run; Continue once resends with budgetBypass and drops attachments (A-2 anchored)', async () => {
    const script = loadFixture('budget-exceeded')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    // The exceeded event carries the seeded over-budget pair in the frozen
    // camelCase budget shape — the payload the page's BudgetBanner listener
    // (useBudgetGuard) filters by sessionId.
    await waitFor(() => {
      const budgetEvent = h.events.find(e => e.event === 'budget:exceeded')
      expect(budgetEvent?.payload).toEqual({ sessionId: SESSION_A, spentUsd: 6.4, budgetUsd: 5 })
    })
    // Budget-cap auto-cancel (same cancel token as Stop, commands.rs:2199):
    // the run settles via query:cancelled — no ghost bubble, no error state.
    await awaitSettled(h)
    expect(h.result.current.streamingText).toBe('')
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(0)
    expect(h.result.current.error).toBeNull()
    expect(h.player.snapshot().sentTurns).toBe(1)

    // "Continue once" — exactly what Chat.tsx's continuePastBudget does:
    // resend the last user message with the bypass flag. The harness passes
    // the same invoke args the real send_message carries.
    await act(async () => {
      await h.result.current.sendMessage(script.turns[0]!.user, undefined, { budgetBypass: true })
    })
    await act(async () => {
      expect(h.player.handleSendMessage({
        sessionId: SESSION_A,
        message: script.turns[0]!.user,
        filePaths: null,
        budgetBypass: true,
      })).toEqual({ query_id: 'q-1' })
    })
    await awaitSettled(h)
    const sends = h.player.snapshot().sends
    expect(sends[1]).toMatchObject({ turnIndex: 1, budgetBypass: true, sessionId: SESSION_A })
    // A-2 current behavior: the budget-bypass resend DROPS the original
    // message's attachments (seeded user message carries one). Flip to
    // `attachments: ['/Users/demo/Downloads/report-draft.md']` when R4 lands.
    expect(sends[1]!.attachments).toBeNull()
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(1)
  })
})

describe('L1 state machine — attachments (journey #8)', () => {
  it('sends ride attachment paths; the turn returns rejected receipts (P0 anchor) and the optimistic bubble stays bare (A-4 anchored)', async () => {
    const script = loadFixture('attachments')
    const outsidePath = script.turns[0]!.rejectedAttachments![0]!.path
    const h = await makeHarness()
    h.player.load(script)

    await act(async () => {
      await h.result.current.sendMessage(script.turns[0]!.user, [outsidePath])
    })
    expect(api.sendMessage).toHaveBeenLastCalledWith(
      script.turns[0]!.user,
      [outsidePath],
      undefined,
      SESSION_A,
    )
    let resp: { query_id: string; rejected_attachments?: unknown[] } | null = null
    await act(async () => {
      resp = h.player.handleSendMessage({ sessionId: SESSION_A })
    })
    // P0-3 partial success: the send stands, the refusal rides the response
    // (AppContext toasts one "«file» was not sent: «reason»" per path —
    // the R2 walkthrough's out-of-working-dir finding anchor).
    expect(resp).toEqual({
      query_id: 'q-0',
      rejected_attachments: [{ path: outsidePath, reason: 'out_of_working_dir' }],
    })
    await awaitSettled(h)
    // A-4 current behavior: the optimistic user message carries NO
    // file_attachments — attachment previews only come back on reload.
    // Flip to `user.file_attachments` being present when R4 lands.
    const user = h.result.current.messages.find(m => m.role === 'user')
    expect(user?.file_attachments).toBeUndefined()
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(1)
  })
})

describe('L1 state machine — queue-steer (journey #9)', () => {
  it('queue caps at 3, reorders, and drains FIFO after settle (the steer order is pinned E2E-side + by the hook\'s own tests)', async () => {
    const script = loadFixture('queue-steer')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    expect(h.result.current.isQuerying).toBe(true)
    // Three queued prompts fit; the fourth overflows (enqueue returns false
    // and keeps the caller's draft — Chat.tsx keeps the composer text).
    await act(async () => {
      expect(h.result.current.enqueuePrompt('队列第一条', [])).toBe(true)
      expect(h.result.current.enqueuePrompt('队列第二条', [])).toBe(true)
      expect(h.result.current.enqueuePrompt('队列第三条', [])).toBe(true)
    })
    expect(h.result.current.promptQueue.map(i => i.text)).toEqual(['队列第一条', '队列第二条', '队列第三条'])
    await act(async () => {
      expect(h.result.current.enqueuePrompt('第四条（应溢出）', [])).toBe(false)
    })
    expect(h.result.current.promptQueue).toHaveLength(3)

    // The chips' up-control moves 第二条 to the head (sends sooner)…
    const secondId = h.result.current.promptQueue[1]!.id
    act(() => { h.result.current.moveQueuedPrompt(secondId, -1) })
    expect(h.result.current.promptQueue.map(i => i.text)).toEqual(['队列第二条', '队列第一条', '队列第三条'])
    // …and the down-control sends it later again (FIFO restored).
    act(() => { h.result.current.moveQueuedPrompt(secondId, 1) })
    expect(h.result.current.promptQueue.map(i => i.text)).toEqual(['队列第一条', '队列第二条', '队列第三条'])
    // The chip's ✕ drops an item outright (removeQueuedPrompt).
    const thirdId = h.result.current.promptQueue[2]!.id
    act(() => { h.result.current.removeQueuedPrompt(thirdId) })
    expect(h.result.current.promptQueue.map(i => i.text)).toEqual(['队列第一条', '队列第二条'])
    // Guard rails: moving a removed (or unknown) id is a no-op.
    act(() => { h.result.current.moveQueuedPrompt(thirdId, -1) })
    expect(h.result.current.promptQueue.map(i => i.text)).toEqual(['队列第一条', '队列第二条'])

    // The turn settles; the Chat-page drain effect consumes the FIFO head
    // first (page-level loop — its ordering contract is pinned here through
    // dequeuePrompt and E2E-side through the reply-bubble order).
    await awaitSettled(h)
    await act(async () => {
      expect(h.result.current.dequeuePrompt()!.text).toBe('队列第一条')
      expect(h.result.current.dequeuePrompt()!.text).toBe('队列第二条')
      expect(h.result.current.dequeuePrompt()).toBeNull()
    })
  })
})

describe('L1 state machine — edit-rewind (journey #10)', () => {
  it('edit commit rewinds to the checkpoint boundary (A-14: equality included) and the resend streams', async () => {
    vi.mocked(api.rewindSession).mockResolvedValue([])
    vi.mocked(api.listCheckpoints).mockResolvedValue([])
    const script = loadFixture('edit-rewind')
    const h = await makeHarness()
    h.player.load(script)

    // Seeded history = 2 user turns; the armed list_checkpoints derives one
    // checkpoint per turn (shape pinned in seed-handlers.test). A-14
    // boundary evidence: rewindInfoFor accepts a checkpoint whose
    // turn_index EQUALS the edited message's turn (`>=`), so turn 1's own
    // checkpoint makes message index 2 editable. Commit mechanics:
    await act(async () => { await h.result.current.rewindSession(1) })
    expect(api.rewindSession).toHaveBeenCalledWith(SESSION_A, 1)
    expect(h.result.current.error).toBeNull()

    // The rewind+resend flow streams the edited text as a fresh turn.
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await awaitSettled(h)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')[0]!.content)
      .toBe(textChunksOf(script, 0).join(''))
  })
})

describe('L1 state machine — session-switch-race (journey #11)', () => {
  it('buckets stay per-session across a switch; the projection resumes on return; the error banner persists across sessions (A-5 anchored)', async () => {
    const script = loadFixture('session-switch-race')
    const h = await makeHarness()
    h.player.load(script)

    // Park before the failed step so the whole switch dance happens with a
    // deterministic live stream (no event-timing races).
    h.player.pauseAt(1)
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.streamingText).toContain('甲乙'))
    await waitFor(() => expect(h.player.snapshot().phase).toBe('waitingUi'))

    // Switching away parks A's projection: B's own (empty) bucket answers.
    // (The context value renames switchToSession → switchSession.)
    await act(async () => { await h.result.current.switchSession(SESSION_B) })
    expect(h.result.current.currentSessionId).toBe(SESSION_B)
    expect(h.result.current.streamingText).not.toContain('甲乙')
    expect(h.result.current.streamingText).toBe('')
    // A keeps streaming off-screen: its chunk lands in A's bucket and must
    // NOT bleed onto B's screen (per-session isolation, §P2-18).
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: '丁', session_id: SESSION_A }) })
    expect(h.result.current.streamingText).toBe('')

    // Switch back: the OWN bucket re-projects, late chunk included (the
    // resume-the-projection half).
    await act(async () => { await h.result.current.switchSession(SESSION_A) })
    await waitFor(() => expect(h.result.current.streamingText).toContain('丁'))
    expect(h.result.current.streamingText).toContain('甲乙')

    // The turn fails while A is visible → classified error banner on A.
    await act(async () => { h.player.resume() })
    await waitFor(() => expect(h.result.current.error).toBe('upstream exploded after the switch'))
    expect(h.result.current.errorKind).toBe('other')

    // A-5 current behavior: switchToSession does NOT clear error/errorKind —
    // A's failure banner follows the user onto B. Flip BOTH expects to
    // `toBeNull()` when R4 lands.
    await act(async () => { await h.result.current.switchSession(SESSION_B) })
    expect(h.result.current.error).toBe('upstream exploded after the switch')
    expect(h.result.current.errorKind).toBe('other')
  })
})

describe('L1 state machine — subagent-run (journey #12)', () => {
  it('subagent:start/stop drive the live registry; the agent_spawn card converges and leaves with the run', async () => {
    const script = loadFixture('subagent-run')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    // The registry bridge goes live while the spawn card is running (the
    // block renders "registry <id>" from exactly this state, page-side).
    await waitFor(() => expect(h.result.current.subagentLive).toEqual({
      agentId: 'sa-research-1', agentName: 'researcher', team: 'alpha',
    }))
    await waitFor(() => expect(h.result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tool-spawn-1',
      tool_name: 'agent_spawn',
      status: 'running',
    }))

    await drainWaits(h.player)
    await awaitSettled(h)
    // subagent:stop + completion: the registry clears and the card leaves
    // with the run (P2-4 — no lingering cards under the committed reply).
    expect(h.result.current.subagentLive).toBeNull()
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.messages.filter(m => m.role === 'assistant')).toHaveLength(1)
  })
})

describe('L1 state machine — journey-cross-page (#13) + context-panels (#14)', () => {
  it('cross-page: the seeded write_file history is the FileCard/source the pages read', async () => {
    const script = loadFixture('cross-page')
    const h = await makeHarness()
    h.player.load(script)
    // The /files and /timeline pages are pure views over the backend
    // records this journey's seed produces: the armed register_file_index
    // gains the FileCard's path (fired on card mount, browser-side) and
    // trace_timeline keeps its demo projection. At L1 we pin the
    // conversation state the chat half commits.
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await awaitSettled(h)
    // The preloaded write_file tool_calls ride the seeded history (A-layer
    // mapping pinned in seed-handlers.test): the user sees the file card
    // WITHOUT any new run — the reply appends after it.
    const assistants = h.result.current.messages.filter(m => m.role === 'assistant')
    expect(assistants).toHaveLength(1) // seeded assistant stays out of live state; reply committed
    expect(assistants[0]!.content).toBe(textChunksOf(script, 0).join(''))
  })

  it('context-panels: query:usage projects onto the visible session and is the usageTick the panels refetch on', async () => {
    const script = loadFixture('context-panels')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.usage).toMatchObject({
      input_tokens: 1200, output_tokens: 300, cost_usd: 0.012,
    }))
    // Every usage event hands the panels a NEW tick → SessionUsageDialog /
    // ContextBreakdownCard refetch get_session_usage (seeded spentUsd
    // 0.0731 answers; handler level pinned in seed-handlers.test).
    await drainWaits(h.player)
    await awaitSettled(h)
    // The projection survives the settle (usage is a session readout, not
    // run state); the panels' refetch-on-tick is exercised E2E-side, where
    // the dialog actually mounts.
    expect(h.result.current.usage).toMatchObject({ input_tokens: 1200 })
  })
})

// ───────────────────── cancel-matrix L1 view (§4.1) ─────────────────────

describe('L1 state machine — cancel-matrix #2 (tool execution stop)', () => {
  it('cancel during a tool run converges the card; a late tool-result does not resurrect it', async () => {
    const script = loadFixture('cancel-tool-run')
    const h = await makeHarness()
    h.player.load(script)

    h.player.pauseAt(2) // park before the waitFor — tool "executing", no progress events (the A-18 blind spot)
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.activeToolCalls[0]).toMatchObject({
      tool_use_id: 'tool-slow-1', tool_name: 'Bash', status: 'running',
    }))

    await act(async () => { await h.result.current.cancelQuery() })
    await act(async () => { expect(h.player.handleCancelQuery()).toBe(true) })
    await awaitSettled(h)
    // Everything the run held converges away.
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.toolProgress).toBeNull()
    expect(h.result.current.streamingText).toBe('')

    // The old query's tool-result arrives at its next event boundary —
    // AFTER the new idle state. The result's map finds no matching card id
    // → no-op; the card must NOT resurrect.
    act(() => {
      flush(EVENT_NAMES.QUERY_TOOL_RESULT, {
        tool_use_id: 'tool-slow-1', result: 'done much later', is_error: false, session_id: SESSION_A,
      })
    })
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.error).toBeNull()
  })
})

describe('L1 state machine — cancel-matrix #4 (approval-wait stop)', () => {
  it('stop while a permission prompt waits settles the run; the prompt itself stays up (current behavior recorded)', async () => {
    const script = loadFixture('cancel-approval-wait')
    const h = await makeHarness()
    h.player.load(script)

    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.player.snapshot().phase).toBe('waitingPermission'))
    expect(h.result.current.permissionRequest).toMatchObject({ request_id: 'pr-cancel-1', risk: 'critical' })

    // Stop (Escape tier 2 / stop button) — the parked turn cancels.
    await act(async () => { await h.result.current.cancelQuery() })
    await act(async () => { expect(h.player.handleCancelQuery()).toBe(true) })
    await awaitSettled(h)
    expect(h.result.current.activeToolCalls).toHaveLength(0)
    expect(h.result.current.error).toBeNull()

    // CURRENT BEHAVIOR (recorded for the report): QUERY_CANCELLED does not
    // touch permissionRequest — the dialog stays up after the run is gone.
    expect(h.result.current.permissionRequest).toMatchObject({ request_id: 'pr-cancel-1' })
    // A late Allow still goes through: the command resolves and the prompt
    // clears (respondPermissionAction clears it on success).
    await act(async () => { await h.result.current.respondPermission('pr-cancel-1', false) })
    expect(h.result.current.permissionRequest).toBeNull()
    expect(api.respondPermission).toHaveBeenCalledWith('pr-cancel-1', false, undefined)
  })
})

describe('L1 state machine — cancel-matrix #3 (stop → instant resend, A-17)', () => {
  // De-race note (裁定修复波): CI shard2 failed this test with
  // `expected '' to contain '新流丁'` — the chunk flush orchestration (real
  // setTimeout macrotasks, capped at 60ms each) is itself asynchronous and,
  // under 2-core CI CPU contention, slower than a direct assertion. Every
  // streamingText / committed-reply point below is therefore a WAITING
  // assertion (this file's established RTL waitFor — act-integrated, same
  // semantics as vi.waitFor/expect.poll) with an explicit 5s window; the
  // assertion objects and semantics are unchanged.
  it('a late old-turn cancelled WIPES the new turn\'s stream and idles the composer (A-17 pollution, recorded)', async () => {
    const script = loadFixture('cancel-then-resend')
    const h = await makeHarness()
    h.player.load(script)

    // Turn 0 streams; stop settles it (the mock's cancel is synchronous —
    // the RACE is modeled by the late events below, which is exactly how
    // the real backend delivers them: after the latch reopens).
    h.player.pauseAt(1)
    await sendAndPlay(h, { text: script.turns[0]!.user, expectedQueryId: 'q-0' })
    await waitFor(() => expect(h.result.current.streamingText).toContain('旧流'), { timeout: 5_000 })
    await act(async () => { await h.result.current.cancelQuery() })
    await act(async () => { expect(h.player.handleCancelQuery()).toBe(true) })
    await awaitSettled(h)

    // Instant resend — the new turn streams (its own query id).
    await sendAndPlay(h, { text: script.turns[1]!.user, expectedQueryId: 'q-1' })
    await waitFor(() => expect(h.result.current.streamingText).toContain('新流甲'), { timeout: 5_000 })
    expect(h.result.current.isQuerying).toBe(true)

    // The old query's boundary events land INSIDE the new turn's window:
    // a late text chunk (q-0) + the late query:cancelled (q-0), same
    // session — indistinguishable from the new turn's events without
    // query_id filtering (the A-17 gap).
    act(() => { flush(EVENT_NAMES.QUERY_TEXT, { content: '[旧流迟到]', query_id: 'q-0', session_id: SESSION_A }) })
    act(() => { flush(EVENT_NAMES.QUERY_CANCELLED, { query_id: 'q-0', session_id: SESSION_A }) })

    // A-17 CURRENT BEHAVIOR — pollution. The late cancelled clears the
    // SESSION bucket (new stream's text gone), flips isQuerying false while
    // q-1 is still streaming, and the committed reply loses every chunk
    // emitted before the pollution point (and the injected late chunk —
    // both were in the wiped bucket). Flip these to the no-pollution
    // asserts (stream intact, isQuerying stays true, full reply) when R4
    // lands. The flush handlers run synchronously, but the projection
    // wait below rides out any throttled state propagation on a starved CI
    // core instead of asserting against a mid-flight frame.
    await waitFor(() => {
      expect(h.result.current.streamingText).not.toContain('新流甲')
      expect(h.result.current.isQuerying).toBe(false)
    }, { timeout: 5_000 })
    // q-1 keeps streaming into the wiped bucket — it commits WITHOUT the
    // pre-pollution chunks. Waiting (5s) rather than direct: the commit is
    // the tail of the chunk macrotask chain, the exact thing CI contention
    // stretches past a direct read.
    await waitFor(() => {
      const reply = h.result.current.messages.filter(m => m.role === 'assistant').at(-1)
      expect(reply?.content ?? '').toContain('新流丁')
    }, { timeout: 5_000 })
    await waitFor(() => {
      const reply = h.result.current.messages.filter(m => m.role === 'assistant').at(-1)
      expect(reply!.content).not.toContain('新流甲')
      expect(reply!.content).not.toContain('旧流迟到')
    }, { timeout: 5_000 })
  })
})

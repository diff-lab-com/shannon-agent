// W2 L1 状态机层 — chatStateMachine.journeys.chrome.test.tsx
//
// The three chrome/surface journeys of the W2 wave (model-mode-switch
// #17 / session-lifecycle #19 / dock-interactions #20) pinned at the
// ScriptPlayer ↔ seed ↔ handlers integration level: each journey's JSON
// fixture loads into the REAL ScriptPlayer with the REAL seed wiring
// (onSeed = setScriptSeed, exactly the coreMock route), and the assertions
// read the same surfaces the browser journeys do — the send-time `model`
// stamp (next-turn semantics), the session-lifecycle projection, and the
// plan write-back loop with its scripted failure flip.
//
// YAML↔JSON parity for these scripts lives HERE (the shared
// chatStateMachine.scripts.test.tsx keeps its own list — parallel waves
// never edit each other's files); the four EXTENDED scripts
// (approval-allow/deny, tool-task-file, cross-page) stay in that shared
// parity loop, their fixtures updated in place.

import { describe, it, expect, afterEach, vi } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { parse } from 'yaml'
import { ScriptPlayer } from '@/lib/mock/scripted/player'
import { validateScript, type ChatScript } from '@/lib/mock/scripted/schema'
import { handlers } from '@/lib/mock/handlers'
import { setScriptSeed } from '@/lib/mock/scripted/seed'

const fixturesRoot = resolve(process.cwd(), 'src/__tests__/fixtures/scripts')
const yamlRoot = resolve(process.cwd(), 'e2e/scripts')

/** The W2 chrome journeys — new scripts, new fixtures, new parity loop. */
const CHROME_SCRIPTS = ['model-mode-switch', 'session-lifecycle', 'dock-interactions'] as const

function loadFixture(name: string): ChatScript {
  return JSON.parse(readFileSync(resolve(fixturesRoot, `${name}.json`), 'utf8')) as ChatScript
}

function loadYaml(name: string): ChatScript {
  return parse(readFileSync(resolve(yamlRoot, `${name}.yaml`), 'utf8')) as ChatScript
}

function makePlayer(): ScriptPlayer {
  // The production wiring: emissions captured, the seed lands in the REAL
  // store (realm-global) the handlers read.
  return new ScriptPlayer({
    emit: () => {},
    onSeed: (seed) => { setScriptSeed(seed) },
  })
}

afterEach(() => {
  // Disarm whatever a test armed — handlers are module singletons.
  setScriptSeed(null)
})

describe('ChatScript fixtures — W2 chrome journeys YAML ↔ JSON parity', () => {
  for (const name of CHROME_SCRIPTS) {
    it(`${name}: fixture mirrors the YAML exactly and both validate`, () => {
      const yaml = loadYaml(name)
      const json = loadFixture(name)
      expect(validateScript(yaml).ok).toBe(true)
      expect(validateScript(json).ok).toBe(true)
      expect(json).toEqual(yaml)
    })
  }

  it('the new seed capabilities survive the round-trip (W2 schema extension)', () => {
    const model = loadYaml('model-mode-switch')
    // The rawLabel leg mutates the config in memory (the e2e arm) — the
    // mutated object is legal schema input and carries the engine-only value.
    const rawLabelArm: ChatScript = {
      ...model,
      seed: { ...model.seed, config: { ...model.seed?.config, approvalMode: 'bypass_permissions' } },
    }
    expect(validateScript(rawLabelArm).ok).toBe(true)
    expect(rawLabelArm.seed?.config?.approvalMode).toBe('bypass_permissions')
    expect(model.seed?.sessions?.[0]?.modelOverride).toEqual({
      provider: 'anthropic',
      model: 'claude-haiku-4-5-20251001',
    })
    const lifecycle = loadYaml('session-lifecycle')
    expect(lifecycle.seed?.sessions?.some(s => s.deleteFails === true)).toBe(true)
    const dock = loadYaml('dock-interactions')
    expect(dock.seed?.sessions?.[0]?.workingDir).toBe('/Users/demo/workspace/shannon-demo')
    // The failure-flip variant the dock spec builds in code is legal input.
    const flipped: ChatScript = {
      ...dock,
      seed: { ...dock.seed, config: { ...dock.seed?.config, saveTextFileFails: true } },
    }
    expect(validateScript(flipped).ok).toBe(true)
  })
})

describe('L1 — model-mode-switch (journey #17): the next-turn model stamp', () => {
  it('sends carry the seeded override; a chip switch lands on the NEXT send; Reset returns null', async () => {
    const script = loadFixture('model-mode-switch')
    const player = makePlayer()
    expect(player.load(script).ok).toBe(true)

    const seeded = script.seed!.sessions![0]!
    // The armed chip read: the seed answers get_session_model before any UI.
    await expect(handlers.get_session_model({ sessionId: seeded.id })).resolves.toEqual(seeded.modelOverride)

    // Turn 0 — the send-time stamp is the seeded override.
    player.handleSendMessage({ message: script.turns[0]!.user, sessionId: seeded.id })
    expect(player.snapshot().sends[0]).toMatchObject({ turnIndex: 0, model: seeded.modelOverride!.model })
    // Let the turn settle (chunked playback + auto-completed) — the next
    // send is only accepted (and logged) once the player re-arms.
    await vi.waitFor(() => expect(player.snapshot().phase).toBe('armed'))

    // A mid-script chip switch (the exact handler the Select drives)…
    await handlers.set_session_model({ sessionId: seeded.id, provider: 'openai', model: 'gpt-5' })
    await expect(handlers.get_session_model({ sessionId: seeded.id })).resolves.toEqual({ provider: 'openai', model: 'gpt-5' })

    // …only reaches the wire on the NEXT send (next-turn semantics). The
    // turn's text chunks play first, then the scripted permission-request
    // parks the player (R1 semantics) — allow it, then release the waitFor
    // park so the turn settles and the player re-arms.
    player.handleSendMessage({ message: script.turns[1]!.user, sessionId: seeded.id })
    expect(player.snapshot().sends[1]).toMatchObject({ turnIndex: 1, model: 'gpt-5' })
    await vi.waitFor(() => expect(player.snapshot().phase).toBe('waitingPermission'))
    player.handleRespondPermission({ requestId: 'pr-model-1', allow: true })
    await vi.waitFor(() => expect(player.snapshot().phase).toBe('waitingUi'))
    player.resume()
    await vi.waitFor(() => expect(player.snapshot().phase).toBe('armed'))

    // Reset (clear_session_model) → the next send inherits again (null).
    await handlers.clear_session_model({ sessionId: seeded.id })
    player.handleSendMessage({ message: script.turns[2]!.user, sessionId: seeded.id })
    expect(player.snapshot().sends[2]).toMatchObject({ turnIndex: 2, model: null })

    player.reset()
    expect(player.snapshot().sends).toEqual([])
  })
})

describe('L1 — session-lifecycle (journey #19): the seeded roster projects mutations', () => {
  it('62 seeded rows; rename/archive/delete/restore flow through list_sessions + search', async () => {
    const script = loadFixture('session-lifecycle')
    const player = makePlayer()
    expect(player.load(script).ok).toBe(true)

    // The cap journey's roster: 62 seeded sessions answer list_sessions…
    const rows = await handlers.list_sessions({})
    expect(rows).toHaveLength(62)
    // …the working-dir-less sessions stay flat (no working_dir key)…
    expect(rows.every((r: { working_dir?: string }) => r.working_dir === undefined)).toBe(true)
    // …and the ONE turn runs clean through the real player.
    player.handleSendMessage({ message: script.turns[0]!.user, sessionId: 'script-sess-life-main' })
    expect(player.snapshot().sentTurns).toBe(1)

    // Rename → search sees it (title-first backend contract).
    await handlers.rename_session({ id: 'script-sess-life-rename', title: 'Renamed!' })
    expect((await handlers.search_sessions({ query: 'renam' })).map((r: { id: string }) => r.id))
      .toEqual(['script-sess-life-rename'])

    // Archive → leaves the rail, lands in the archived lens; restore reverts.
    await handlers.archive_session({ id: 'script-sess-life-archive' })
    expect((await handlers.list_sessions({})).some((r: { id: string }) => r.id === 'script-sess-life-archive')).toBe(false)
    expect(await handlers.list_archived_sessions({})).toEqual([
      expect.objectContaining({ id: 'script-sess-life-archive', title: 'Archive me' }),
    ])
    await handlers.unarchive_session({ id: 'script-sess-life-archive' })
    expect(await handlers.list_archived_sessions({})).toEqual([])

    // The deleteFails fixture refuses exactly the marked session.
    await expect(handlers.delete_session({ id: 'script-sess-life-cursed' })).rejects.toThrow(/refused/)
    await handlers.delete_session({ id: 'script-sess-life-delete' })
    const after = await handlers.list_sessions({})
    expect(after.some((r: { id: string }) => r.id === 'script-sess-life-cursed')).toBe(true)
    expect(after.some((r: { id: string }) => r.id === 'script-sess-life-delete')).toBe(false)

    player.reset()
  })
})

describe('L1 — dock-interactions (journey #20): the plan write-back loop', () => {
  const WORKING_DIR = '/Users/demo/workspace/shannon-demo'

  it('a human check writes back through save_text_file and the plan read serves it', async () => {
    const script = loadFixture('dock-interactions')
    const player = makePlayer()
    expect(player.load(script).ok).toBe(true)

    // The seeded working_dir is what Chat.tsx resolves as the panel's fetch
    // key; the demo plan answers first (engine truth).
    expect(await handlers.get_session_plan({ workingDir: WORKING_DIR }))
      .toMatchObject({ title: 'Q3 roadmap execution plan' })

    // The exact write PlanPanel's checkbox tick composes (header layout
    // included) lands in the store and the READ serves it back — the tick
    // survives its own refresh.
    const header = '# Plan: Q3 roadmap execution plan\nCreated: c\nStatus: approved\n\n' +
      '- [x] Survey partner API surface\n- [x] Draft the OAuth gallery spec\n- [ ] Load-test the webhook path'
    await expect(handlers.save_text_file({ path: `${WORKING_DIR}/.shannon/plans/demo-plan.md`, content: header })).resolves.toBe(true)
    await expect(handlers.get_session_plan({ workingDir: WORKING_DIR })).resolves.toMatchObject({
      content: expect.stringContaining('- [x] Draft the OAuth gallery spec'),
    })

    player.reset()
  })

  it('the scripted failure flip rejects the write; the demo plan (engine truth) answers', async () => {
    const script = loadFixture('dock-interactions')
    const failing: ChatScript = {
      ...script,
      seed: { ...script.seed, config: { ...script.seed?.config, saveTextFileFails: true } },
    }
    const player = makePlayer()
    expect(player.load(failing).ok).toBe(true)

    await expect(handlers.save_text_file({ path: `${WORKING_DIR}/.shannon/plans/demo-plan.md`, content: 'x' }))
      .rejects.toThrow(/failed/)
    // Nothing recorded → the demo plan (engine truth) answers, so the
    // rolled-back checkbox re-renders the engine's unchecked step.
    await expect(handlers.get_session_plan({ workingDir: WORKING_DIR }))
      .resolves.toMatchObject({ content: expect.stringContaining('- [ ] Draft the OAuth gallery spec') })

    player.reset()
  })
})

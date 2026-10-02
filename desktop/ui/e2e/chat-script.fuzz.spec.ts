// R5 chat-testing plan §5.2 — event fuzz / mutation suite ("find unknown
// problems" machine, part 1).
//
// scripts/fuzz-mutate.mjs (`pnpm fuzz:gen`) derives schema-VALID but
// semantics-hostile mutants from e2e/scripts/fuzz/base.yaml. This spec loads
// every mutant through the ScriptedBackend and asserts the two properties
// the brief demands:
//
//   1. NO CRASH — the console watchdog (pageerror / console.error /
//      unhandledrejection) stays at zero failures, modulo the
//      KNOWN_FUZZ_CRASHES allowlist below (a suspected-bug registry: each
//      entry must carry the tracked finding; entries are REMOVED when the
//      bug is fixed).
//   2. TERMINAL IDEMPOTENCE — loosely, not by exact DOM (brief §A): the
//      player phase settles to armed|done, the composer resets (send button
//      back), the streaming cursor is gone, no tool card is left running,
//      and the transcript stops mutating (bubble count frozen ~0.8s later).
//
// Plus two BRIDGE probes the player cannot express (emitNow bypasses the
// player state machine and fires raw at the event bridge): text arriving
// AFTER completed, a second completed after completed, and a totally
// unknown event name — followed by a real send that must still settle.
//
// NIGHTLY-ONLY: excluded from the PR gate by playwright.config.ts
// testIgnore; run via playwright.chat-nightly.config.ts.
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { mockSnapshot } from './helpers/knownIssues'
import { getConsoleWatchdog, loadChatScriptObject, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { validateScript } from '../src/lib/mock/scripted/schema'

const MUTANT_DIR = fileURLToPath(new URL('./scripts/fuzz/mutants', import.meta.url))

interface FuzzManifest {
  seed: number
  categories: Record<string, number>
  total: number
  mutants: Array<{ file: string, category: string, description: string }>
}

function readManifest(): FuzzManifest {
  return JSON.parse(readFileSync(join(MUTANT_DIR, 'manifest.json'), 'utf8')) as FuzzManifest
}

function readMutants(): Array<{ name: string, category: string, description: string, script: unknown }> {
  const manifest = readManifest()
  return manifest.mutants.map((entry) => {
    const script = JSON.parse(readFileSync(join(MUTANT_DIR, entry.file), 'utf8')) as unknown
    // Fail LOUDLY (schema broke) instead of letting the player's boot
    // validation console.error the page into a watchdog failure — those are
    // different failure classes with different fixes.
    const result = validateScript(script)
    expect(result.ok, `mutant ${entry.file} must stay schema-valid:\n${result.errors.join('\n')}`).toBe(true)
    return { name: entry.file.replace(/\.json$/, ''), category: entry.category, description: entry.description, script }
  })
}

/**
 * Suspected-bug registry (brief §A: real crashes become tracked findings,
 * not red nightlies). Key = mutant name, value = console-error regexes that
 * THIS mutant is allowed to produce while the bug is open. An entry here
 * MUST reference a finding in the R5 report's suspected-bug table. Empty
 * today — every mutant is expected to survive.
 */
const KNOWN_FUZZ_CRASHES: Record<string, RegExp[]> = {}

/**
 * Suspected-WEDGE registry — mutants whose terminal-idempotence violation
 * is a REPRODUCED, tracked finding instead of a red nightly. For these the
 * standard settle assertions are replaced by a precise reproduction of the
 * wedge (the bug's contract), and the test carries a `suspectedBug`
 * annotation. Removing the entry flips the mutant back to asserting the
 * settled contract — the R2/R4 one-marker flip, done from the spec side.
 *
 * Empty today. F-1 (found by this suite, R5 — a terminal `query:completed`
 * whose session_id pointed at ANY other session wedged the sending
 * session's composer latch) was FIXED by the p0a1 round: query:* events
 * route/settle by their query_id's OWNER session (the A-17/G6 send
 * records), so `cross-session-wrong-session-id` now passes the standard
 * settled contract like every other mutant. The frozen repro stays as the
 * regression pin, flipped to the fixed contract:
 * e2e/scripts/fuzz-found-cross-session.yaml + chat-script.fuzz-found.spec.ts.
 */
const KNOWN_FUZZ_WEDGES: Record<string, string> = {}

/** Wait until the player settled the turn (armed = next turn, done = last). */
async function expectSettled(page: import('@playwright/test').Page): Promise<string> {
  let phase = ''
  await expect
    .poll(async () => {
      phase = (await mockSnapshot(page)).phase
      return phase
    }, { timeout: 30_000, intervals: [100] })
    .toMatch(/^(armed|done)$/)
  return phase
}

test.describe('chat-script fuzz — mutated events must never crash nor wedge the UI', () => {
  test('control: the unmutated baseline still plays clean (harness sanity)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    const base = readChatScript('fuzz/base')
    await loadChatScriptObject(page, base, test.info())

    await chat.send('跑一个 fuzz 基线回合')
    await expectSettled(page)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(page.locator('[data-tool-status="running"]')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2)
    await expectNoConsoleErrors(page)
  })

  for (const mutant of readMutants()) {
    test(`mutant [${mutant.category}]: ${mutant.name} — no crash, terminal idempotence`, async ({ page }) => {
      test.setTimeout(90_000)
      const chat = new ChatPage(page)
      await loadChatScriptObject(page, mutant.script, test.info())

      await chat.send('跑一个 fuzz 基线回合')

      // 1. The turn SETTLES on the PLAYER side (loose terminal semantics —
      //    armed or done). The app-side contract is asserted below.
      await expectSettled(page)

      // 2. Terminal idempotence — or, for tracked wedges (KNOWN_FUZZ_WEDGES),
      //    the PRECISE reproduction of the wedge that is the finding.
      const wedgeReason = KNOWN_FUZZ_WEDGES[mutant.name]
      if (wedgeReason) {
        // Tracked-wedge reproduction (generic shape, F-1 was the only
        // occupant): the visible session stays querying forever — stop up,
        // send gone, transcript frozen, no crash. The precise story of the
        // wedge lives in the registry entry's reason text.
        test.info().annotations.push({ type: 'suspectedBug', description: `${mutant.name}: ${wedgeReason}` })
        // eslint-disable-next-line no-console
        console.warn(`[fuzz][suspectedBug] ${mutant.name}: ${wedgeReason}`)
        await expect(chat.stopButton()).toBeVisible({ timeout: 15_000 })
        await expect(chat.sendButton()).toHaveCount(0)
        await expect(chat.streamingCursor()).toHaveCount(0)
        await expect(page.locator('[data-tool-status="running"]')).toHaveCount(0)
        const wedgedBubbles = await chat.messageCount()
        await page.waitForTimeout(800)
        expect(await chat.messageCount()).toBe(wedgedBubbles)
      } else {
        await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
        await expect(chat.streamingCursor()).toHaveCount(0)
        await expect(page.locator('[data-tool-status="running"]')).toHaveCount(0)
        const bubblesAfterSettle = await chat.messageCount()
        await page.waitForTimeout(800)
        expect(
          await chat.messageCount(),
          'transcript must stop mutating after the terminal event (no ghost commits)',
        ).toBe(bubblesAfterSettle)
      }

      // 3. No crash — watchdog failures minus this mutant's tracked-bug
      //    allowlist (empty everywhere today).
      const watchdog = getConsoleWatchdog(page)
      const allowlist = KNOWN_FUZZ_CRASHES[mutant.name] ?? []
      const failures = (watchdog?.failures() ?? []).filter(e => !allowlist.some(re => re.test(e.text)))
      if (failures.length > 0) {
        await test.info().attach('watchdog-failures', {
          body: JSON.stringify(failures, null, 2),
          contentType: 'application/json',
        })
      }
      expect(
        failures,
        `mutant ${mutant.name} crashed:\n${failures.map(f => `  [${f.kind}] ${f.text}`).join('\n')}`
          + (allowlist.length > 0 ? '' : '\n(no KNOWN_FUZZ_CRASHES entry — a NEW finding; freeze it per the R5 report contract)'),
      ).toEqual([])
    })
  }

  test('bridge probe: late text, double completed and unknown events after settle cannot wedge the session', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    const base = readChatScript('fuzz/base')
    await loadChatScriptObject(page, base, test.info())

    await chat.send('跑一个 fuzz 基线回合')
    await expectSettled(page)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    const settledBubbles = await chat.messageCount()

    // Raw bridge noise the player would normally shield: a chunk after the
    // terminal, a SECOND terminal for the same query, and an event name the
    // app never registered a listener for.
    await page.evaluate(() => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { emitNow(name: string, payload?: Record<string, unknown>): void } }
      }).__shannonMock
      if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
      mock.control.emitNow('query:text', { content: '迟到的分片', query_id: 'q-0', session_id: null })
      mock.control.emitNow('query:completed', { query_id: 'q-0', session_id: null })
      mock.control.emitNow('query:totally-unknown-event', { query_id: 'q-0', session_id: null })
    })
    await page.waitForTimeout(800)
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(page.locator('[data-tool-status="running"]')).toHaveCount(0)
    expect(await chat.messageCount()).toBe(settledBubbles)

    // The session must remain USABLE: a second scripted send still settles.
    await chat.send('再补一小句')
    await expectSettled(page)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(settledBubbles + 2)
    await expectNoConsoleErrors(page)
  })

  test('manifest: mutant census is intact and deterministic-seeded', async () => {
    const manifest = readManifest()
    expect(manifest.seed, 'fixed seed — regeneration must be byte-stable').toBe(20261002)
    expect(manifest.total).toBe(Object.values(manifest.categories).reduce((a, b) => a + b, 0))
    // Every category from the brief must stay represented.
    for (const category of ['reorder', 'duplicate', 'cross-session', 'boundary', 'bombardment']) {
      expect(manifest.categories[category] ?? 0, `category ${category} must not go empty`).toBeGreaterThan(0)
    }
    // Every manifest entry has its file on disk (and vice versa).
    const files = readdirSync(MUTANT_DIR).filter(f => f.endsWith('.json') && f !== 'manifest.json')
    expect(files.sort()).toEqual(manifest.mutants.map(m => m.file).sort())
  })
})

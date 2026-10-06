// R3 journey #11★（矩阵#11）— session-switch-race: A streams while the user
// reads B (no cross-session bleed, B stays idle), returning to A resumes the
// projection; the failed turn's banner stays with A (A-5 fixed in R4 group
// 3 — cleared when the switch to B starts); the cold start binds the
// scripted "current conversation" (A-6 fixed — the Header carries the first
// seeded session's title before any click); drafts stay per-session.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('session-switch-race') as ChatScript

/** Click a session row and retry until the switch lands — the rail can
 *  swallow a click during a list re-render (observed under parallel load). */
async function openRaceSession(page: import('@playwright/test').Page, row: string, heading: string): Promise<void> {
  await expect(async () => {
    await page.getByTestId(row).click()
    await expect(page.getByRole('heading', { name: heading })).toBeVisible({ timeout: 5_000 })
  }).toPass({ timeout: 30_000 })
}

test.describe('scripted chat backend — session-switch-race (journey #11)', () => {
  test('cold start binds the active session (A-6), mid-stream switch isolates buckets, the projection resumes, the error banner stays with A (A-5 fixed)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-switch-race', test.info())

    // A-6 fixed (R4 group 3): the main window binds the scripted active
    // session (the FIRST seeded session — get_conversation's answer) at
    // cold start, so the Header carries its title before any click.
    await expect(page.getByRole('banner').locator('h2')).toHaveText('Race A', { timeout: 10_000 })

    // A streams (8 chunks × 500ms — a wide switch window).
    await openRaceSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await chat.send(script.turns[0]!.user)
    await chat.expectStreamingCursor()
    // OBSERVED (report §journey-11): a pure-text stream never shows the
    // rail's Running dot — noteSessionActivity publishes state only for
    // non-'event' kinds (tool-start / permission / end), so text-only runs
    // update the ref alone. Not asserted; related to R2 report §6.3.

    // Mid-stream, switch to B: no bleed, B idle with its own history.
    await openRaceSession(page, 'desktop-session-row-script-sess-race-b', 'Race B')
    await expect(page.getByText('B 的历史回答。')).toBeVisible()
    await expect(page.getByText('甲', { exact: true })).toHaveCount(0)
    await expect(page.getByText('庚', { exact: true })).toHaveCount(0)
    await expect(chat.stopButton()).toHaveCount(0)
    await expect(chat.composer()).toHaveValue('')

    // Back to A: the projection resumes mid-stream. S-4 fixed: the just-sent
    // bubble survives the round trip (the scripted backend records accepted
    // sends at turn start, like the real backend's L0 tee).
    await openRaceSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await expect(chat.bubbleAt(0)).toContainText('A 的长问题')
    await chat.expectStreamingCursor()
    // Draft assertion part 1: A's draft is typed while the run streams.
    await chat.composer().fill('A 的草稿')

    // The run fails while A is visible: the classified banner appears.
    await expect(page.getByRole('alert').filter({ hasText: 'upstream exploded after the switch' })).toBeVisible({ timeout: 15_000 })

    // A-5 fixed (R4 group 3): the banner stays with A — B opens clean.
    await openRaceSession(page, 'desktop-session-row-script-sess-race-b', 'Race B')
    await expect(page.getByRole('alert').filter({ hasText: 'upstream exploded after the switch' })).toHaveCount(0)
    // Draft isolation: B's composer never shows A's draft.
    await expect(chat.composer()).toHaveValue('')

    // Back to A: the draft survived the round trip (R2-W1 anchor half).
    await openRaceSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await expect(chat.composer()).toHaveValue('A 的草稿')
    await expectNoConsoleErrors(page)
  })

  // ── B3-5 pin（plan §七-11 可选项）：R8-③ tee 契约钉 ────────────────────
  //
  // R8-③ closed as "won't happen" with a contract to pin: the user message
  // is written to the L0 log at TURN START (agent_loop records it before the
  // model sees anything, in tee's forced durable-boundary class), so a
  // mid-stream switch away and back ALWAYS projects this turn's user bubble
  // — "reply without a question" cannot exist. The scripted twin is
  // recordSeedUserSend (accepted sends record at turn start, like the L0
  // tee). The race journey above pins this shape for a FAILED run; this pin
  // covers the COMPLETED side: the turn finishes ON A after the return,
  // exactly one user bubble, the full reply committed — the tee never
  // double-writes and the tail is not lost.
  test('R8-③ tee pin: mid-stream round trip keeps the user bubble; the run settles on return with no duplicate tee', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-switch-tee', test.info())

    // A streams (8 chunks × 700ms — two switch windows wide).
    await openRaceSession(page, 'desktop-session-row-script-sess-tee-a', 'Tee A')
    await chat.send('T 的长问题')
    await chat.expectStreamingCursor()

    // Mid-stream, away to B: nothing of A's turn surfaces there.
    await openRaceSession(page, 'desktop-session-row-script-sess-tee-b', 'Tee B')
    await expect(page.getByText('B 的历史回答。')).toBeVisible()
    await expect(page.getByText('T 的长问题')).toHaveCount(0)
    await expect(chat.stopButton()).toHaveCount(0)

    // Back mid-stream: the tee contract — the user bubble is present (the
    // turn-start record), the projection resumes.
    await openRaceSession(page, 'desktop-session-row-script-sess-tee-a', 'Tee A')
    await expect(chat.bubbleAt(0)).toContainText('T 的长问题')
    await chat.expectStreamingCursor()

    // The run completes ON A: exactly one user bubble + the FULL reply
    // (concatenated chunks, tail included) — no duplicate tee, no loss.
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 20_000 })
    await chat.expectBubbleText(1, '甲，乙，丙，丁，戊，己，庚，辛。')
    expect((await mockSnapshot(page)).sentTurns).toBe(1)

    // Harness note (not asserted): a post-settle second round trip would
    // re-project WITHOUT the reply — the scripted tail deliberately records
    // accepted sends and cancelled/failed partials only ("completed replies
    // stay unrecorded", seed.ts — the S-4 pre-existing gap), so a settled
    // turn's reply cannot survive a re-switch in this mock. The REAL backend
    // tee's completed assistant message IS durable; pinning that shape here
    // would need a mock extension (production code — out of scope for this
    // test-only pack).
    await expectNoConsoleErrors(page)
  })
})

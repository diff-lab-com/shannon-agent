// R3 journey #11★（矩阵#11）— session-switch-race: A streams while the user
// reads B (no cross-session bleed, B stays idle), returning to A resumes the
// projection; the failed turn's banner follows the user onto B (known bug
// A-5, current behavior asserted); drafts stay per-session.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues } from './helpers/knownIssues'
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
  test('mid-stream switch isolates buckets, the projection resumes, the error banner persists across sessions (A-5 anchored)', async ({ page }) => {
    test.setTimeout(60_000)
    annotateKnownIssues(test.info(), {
      'A-5': 'switchToSession does not clear error/errorKind (AppContext.tsx), so the previous '
        + 'session\'s failure banner (auth included) follows the user onto the next session. '
        + 'Current behavior asserted below (banner visible on B); flip to "no banner on B" '
        + 'when R4 lands.',
    })
    const chat = new ChatPage(page)
    await loadChatScript(page, 'session-switch-race', test.info())

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

    // Back to A: the projection resumes mid-stream.
    await openRaceSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await chat.expectStreamingCursor()
    // Draft assertion part 1: A's draft is typed while the run streams.
    await chat.composer().fill('A 的草稿')

    // The run fails while A is visible: the classified banner appears.
    await expect(page.getByRole('alert').filter({ hasText: 'upstream exploded after the switch' })).toBeVisible({ timeout: 15_000 })

    // A-5 current behavior: the banner follows the user onto B.
    await openRaceSession(page, 'desktop-session-row-script-sess-race-b', 'Race B')
    await expect(page.getByRole('alert').filter({ hasText: 'upstream exploded after the switch' })).toBeVisible()
    // Draft isolation: B's composer never shows A's draft.
    await expect(chat.composer()).toHaveValue('')

    // Back to A: the draft survived the round trip (R2-W1 anchor half).
    await openRaceSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await expect(chat.composer()).toHaveValue('A 的草稿')
    await expectNoConsoleErrors(page)
  })
})

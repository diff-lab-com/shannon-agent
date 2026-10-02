// R2 journey #6 子集（矩阵#6）— cancel-text-stream: stop mid-stream on the
// player's default onCancel path. The full cancel matrix lands in R3.
//
// D6 (adopted — flipped from the A-19 discard anchor): the half-streamed
// text COMMITS as the assistant bubble with a "stopped" marker, the marked
// partial survives a session-switch round-trip (the scripted backend records
// the interrupted partial into the session tail, mirroring the real
// backend's interrupted-turn finalize in the L0 log), and Regenerate stays
// available on the partial bubble (the cancel path records the turn
// checkpoint, same as completion).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { mockSnapshot } from './helpers/knownIssues'
import { expectNoConsoleErrors } from './helpers/watchdog'

/** Switch sessions and only accept the click when the target heading is up
 *  (the same retry-until-landed loop the cancel-matrix openSession uses). */
async function openSession(page: import('@playwright/test').Page, rowTestId: string, headingName: string): Promise<void> {
  await expect(async () => {
    await page.getByTestId(rowTestId).click()
    await expect(page.getByRole('heading', { name: headingName })).toBeVisible()
  }).toPass({ timeout: 15_000 })
}

test.describe('scripted chat backend — cancel-text-stream (journey #6)', () => {
  test('stop mid-stream settles via query:cancelled and keeps the partial text with a stopped marker (D6)', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-text-stream', test.info())

    await chat.send('写一首关于海的长诗，慢慢写')
    await chat.expectStreamingCursor()
    // First chunk is up; the remaining chunks ride 5s gaps — a stable
    // mid-stream window to stop in.
    await expect(chat.bubbleAt(0)).toContainText('长诗')

    await chat.stop()

    // Convergence: stop→send swap-back, no residual cursor/stream state,
    // run pill settles out (done-not-failed: the pill LEAVES, and the rail
    // never shows the failed badge).
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expect(page.getByRole('img', { name: 'Last run failed' })).toHaveCount(0)
    await expect(page.getByRole('alert')).toHaveCount(0)

    // D6: the half-streamed text commits — user bubble + the partial
    // assistant bubble carrying the emitted chunks (at least the first;
    // the 5s gaps make the stop land mid-stream, and the exact-bytes
    // invariant is pinned at L1), visibly marked as stopped, with
    // Regenerate available (last assistant bubble — the cancel path
    // recorded the turn checkpoint).
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText('海浪拍岸')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await expect(chat.bubbleAt(1).getByRole('button', { name: 'Regenerate response' })).toBeVisible()
    expect((await mockSnapshot(page)).phase).toBe('done')

    // Reload consistency: switch away and back — the marked partial comes
    // back from the session tail projection (the scripted counterpart of
    // the L0 interrupted-turn finalize), text and marker identical.
    await openSession(page, 'desktop-session-row-script-sess-cancel-spare', 'Cancel spare')
    await expect(chat.bubbles()).toHaveCount(0)
    await openSession(page, 'desktop-session-row-script-sess-cancel', 'Cancel stream')
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText('海浪拍岸')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

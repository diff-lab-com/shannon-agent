// R2 journey #6 子集（矩阵#6）— cancel-text-stream: stop mid-stream on the
// player's default onCancel path. The full cancel matrix lands in R3.
//
// Assertions: query:cancelled converges (no residual stream/cursor,
// stop→send swap-back, no failed-run residue) and the partial text is NOT
// committed — today's discard semantics, anchored to known bug A-19.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues, mockSnapshot } from './helpers/knownIssues'

test.describe('scripted chat backend — cancel-text-stream (journey #6)', () => {
  test('stop mid-stream settles via query:cancelled and discards the partial text (A-19 anchored)', async ({ page }) => {
    test.setTimeout(60_000)
    annotateKnownIssues(test.info(), {
      'A-19': 'Cancel discards the streamed partial text (B0 P1-2 ghost-bubble cleanup). '
        + 'Current behavior is asserted below; flip to "partial text commits as the assistant '
        + 'bubble" when R4 lands and remove the knownIssue marker in e2e/scripts/cancel-text-stream.yaml.',
    })
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

    // A-19 current behavior: the half-streamed text is discarded — only the
    // user bubble remains, no assistant bubble with the partial poem.
    await expect(chat.bubbles()).toHaveCount(1)
    expect((await mockSnapshot(page)).phase).toBe('done')
    await expectNoConsoleErrors(page)
  })
})

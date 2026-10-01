// R2 journey #2（矩阵#2）— multi-turn-stream: two conversational turns where
// the first streams 60 chunks of long mixed 中/EN text.
//
// Assertions: bubble text === exact chunk concatenation (no dropped or
// duplicated fragments), stick-to-bottom while streaming (the viewport hugs
// the live tail and no "scroll to latest" FAB appears), and the second send
// increments the player's turn (query_id q-0 → q-1, observed via snapshot).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('multi-turn-stream') as ChatScript
const turn = (i: number) => script.turns[i]!
const chunksOf = (i: number) =>
  turn(i).script.filter(s => 'chunks' in s && s.chunks).flatMap(s => s.chunks!)
const REPLY_1 = chunksOf(0).join('')
const REPLY_2 = chunksOf(1).join('')

/** The chat scroll parent — the overflow container holding the bubbles. */
function scrollParent(page: import('@playwright/test').Page) {
  return page.locator('div.relative.flex-1.overflow-y-auto').filter({ has: page.locator('[data-message-index]') }).first()
}

test.describe('scripted chat backend — multi-turn-stream (journey #2)', () => {
  test('long first stream stays pinned to the bottom; second turn increments the query id', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'multi-turn-stream', test.info())

    await chat.send(turn(0).user)
    await chat.expectStreamingCursor()

    // The first scripted turn plays with turnIndex 0 (query_id q-0).
    await expect.poll(async () => (await mockSnapshot(page)).turnIndex, { timeout: 5_000 }).toBe(0)

    // Stick-to-bottom: while 60 chunks stream for ~1.8s the viewport stays
    // within the app's own NEAR_BOTTOM_PX (80) of the live tail, and the
    // "scroll to latest" FAB (shown when drifted >200px away) never appears.
    const fab = page.getByRole('button', { name: 'Scroll to latest message' })
    await expect
      .poll(
        async () =>
          page.evaluate(() => {
            const el = document.querySelector('[data-message-index]')?.closest('.overflow-y-auto')
            if (!el) return Number.POSITIVE_INFINITY
            return el.scrollHeight - el.scrollTop - el.clientHeight
          }),
        { timeout: 5_000 },
      )
      .toBeLessThanOrEqual(80)
    await expect(fab).toHaveCount(0)

    // Settle: the 60-chunk bubble commits byte-exact (no 丢字/重复).
    await expect(chat.sendButton()).toBeVisible({ timeout: 20_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await chat.expectBubbleText(1, REPLY_1)

    // Second turn: the player arms the next turn — query_id increments.
    await chat.send(turn(1).user)
    await expect(chat.streamingCursor()).toBeVisible()
    await expect
      .poll(async () => {
        const snap = await mockSnapshot(page)
        return snap.phase === 'playing' ? snap.turnIndex : null
      }, { timeout: 5_000 })
      .toBe(1)

    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.bubbles()).toHaveCount(4)
    await chat.expectBubbleText(3, REPLY_2)
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

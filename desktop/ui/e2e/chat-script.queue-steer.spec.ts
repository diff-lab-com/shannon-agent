// R3 journey #9★（矩阵#9）— queue-steer: three prompts queued while the
// session streams (chips with reorder/remove, the 4th overflows with a
// toast and keeps the draft), the FIFO drain order after the run settles,
// and the Ctrl+Enter steer racing a parked queue (stop → settle → the
// steer's send lands BEFORE the drain — Imp-1/Imp-2).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

const ROW = 'desktop-session-row-script-sess-queue'
const UP = 'Move queued message up (sends sooner)'

test.describe('scripted chat backend — queue-steer (journey #9)', () => {
  test('three queued prompts: chips, cap toast keeps the draft, reorder, FIFO drain order', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'queue-steer', test.info())
    await page.getByTestId(ROW).click()
    await expect(page.getByRole('heading', { name: 'Queue and steer' })).toBeVisible({ timeout: 10_000 })

    // Turn 0 streams for ~3.6s (6 chunks × 600ms) — a wide queueing window.
    await chat.send('写一篇关于潮汐的长文')
    await chat.expectStreamingCursor()

    // Queue three prompts while streaming — each accepted send clears the
    // composer and renders a chip.
    for (const text of ['队列第一条', '队列第二条', '队列第三条']) {
      await chat.send(text)
      await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: text })).toBeVisible()
    }
    await expect(page.getByTestId('prompt-queue')).toContainText('3 queued')

    // The 4th overflows: an error toast AND the draft stays in the composer
    // (the caller keeps the text — AppContext.enqueuePrompt contract).
    await chat.send('第四条（应溢出）')
    await expect(
      page.locator('[data-sonner-toast]').filter({ hasText: 'Queue is full (max 3)' }),
    ).toBeVisible()
    await expect(chat.composer()).toHaveValue('第四条（应溢出）')
    await expect(page.getByTestId('prompt-queue-chip')).toHaveCount(3)

    // The chips are steerable: move 第二条 to the head (sends sooner).
    const chip2 = page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第二条' })
    await chip2.getByRole('button', { name: UP }).click()
    const chips = page.getByTestId('prompt-queue-chip')
    await expect(chips.nth(0)).toContainText('队列第二条')
    await expect(chips.nth(1)).toContainText('队列第一条')
    await expect(chips.nth(2)).toContainText('队列第三条')

    // The long turn settles → the drain auto-sends the queue in ITS order:
    // 第二条 → 第一条 → 第三条. Bubbles: [user, reply, then per queued
    // item user+reply].
    await expect(chat.bubbles()).toHaveCount(8, { timeout: 30_000 })
    await chat.expectBubbleText(3, '回复：第二条')
    await chat.expectBubbleText(5, '回复：第一条')
    await chat.expectBubbleText(7, '回复：第三条')
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)
    expect((await mockSnapshot(page)).sentTurns).toBe(4)
    await expectNoConsoleErrors(page)
  })

  test('Ctrl+Enter steer beats a parked queue: stop → settle → send lands before the drain', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'queue-steer', test.info())
    await page.getByTestId(ROW).click()
    await expect(page.getByRole('heading', { name: 'Queue and steer' })).toBeVisible({ timeout: 10_000 })

    await chat.send('写一篇关于潮汐的长文')
    await chat.expectStreamingCursor()
    // Park a queued item first — the steer must jump ahead of it.
    await chat.send('队列第一条')
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第一条' })).toBeVisible()

    // Steer: the interrupt-now send (same path as the composer bolt).
    await chat.composer().fill('加急：先回答这个')
    await chat.composer().press('Control+Enter')
    await expect(chat.composer()).toHaveValue('')

    // The steer cancels turn 0, waits for its settle, then delivers — the
    // queued chip stays parked while the steer's turn streams, and the
    // steer's user bubble appears BEFORE the queued item's.
    await expect(chat.bubbleAt(1)).toContainText('加急：先回答这个', { timeout: 15_000 })
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第一条' })).toBeVisible()
    // The steer's reply commits (player turn 1); the queue drains only
    // after THAT run settles.
    await expect(chat.bubbles()).toHaveCount(5, { timeout: 30_000 })
    // Bubbles: user 长文 / user 加急 / reply 第一条 / user 队列第一条 / reply 第二条.
    await chat.expectBubbleText(2, '回复：第一条')
    await chat.expectBubbleText(4, '回复：第二条')
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)
    expect((await mockSnapshot(page)).sentTurns).toBe(3)
    await expectNoConsoleErrors(page)
  })
})

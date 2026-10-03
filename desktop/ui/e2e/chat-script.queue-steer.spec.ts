// R3 journey #9★（矩阵#9）— queue-steer: three prompts queued while the
// session streams (chips with reorder/remove, the 4th overflows with a
// toast and keeps the draft), the FIFO drain order after the run settles,
// and the Ctrl+Enter steer racing a parked queue (stop → settle → the
// steer's send lands BEFORE the drain — Imp-1/Imp-2).
//
// wave-2 quickwin (D9-b): a third script pins the attachments-only queue
// item — QueueChips.tsx's attachmentsOnly placeholder branch became a live,
// depended-on path after the A-9 fix (纯附件输入入队), so the chip's
// "N attachments" placeholder and the drained send are pinned in
// queue-attachments-only.yaml.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import { emitWebviewDrop } from './helpers/webviewDrop'

const ROW = 'desktop-session-row-script-sess-queue'
const UP = 'Move queued message up (sends sooner)'

// The D9-b attachment (inside the demo working dir — no refusal badge).
const ATT_PATH = '/Users/demo/workspace/my-startup/report-draft.md'
const ATT_NAME = ATT_PATH.split('/').pop()!

test.describe('scripted chat backend — queue-steer (journey #9)', () => {
  test('three queued prompts: chips, cap toast keeps the draft, reorder, FIFO drain order', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'queue-steer', test.info())
    // Retry the row click until the switch lands (the rail can swallow a
    // click during a list re-render under parallel load).
    await expect(async () => {
      await page.getByTestId(ROW).click()
      await expect(page.getByRole('heading', { name: 'Queue and steer' })).toBeVisible({ timeout: 5_000 })
    }).toPass({ timeout: 30_000 })

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

    // The chips are steerable (GB P2-10a): up moves 第二条 to the head…
    const chip2 = page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第二条' })
    const DOWN = 'Move queued message down (sends later)'
    await chip2.getByRole('button', { name: UP }).click()
    const chips = page.getByTestId('prompt-queue-chip')
    await expect(chips.nth(0)).toContainText('队列第二条')
    await expect(chips.nth(1)).toContainText('队列第一条')
    await expect(chips.nth(2)).toContainText('队列第三条')
    // …down sends it later again — the original FIFO order is restored.
    await chip2.getByRole('button', { name: DOWN }).click()
    await expect(chips.nth(0)).toContainText('队列第一条')
    await expect(chips.nth(1)).toContainText('队列第二条')
    await expect(chips.nth(2)).toContainText('队列第三条')
    // The chip's ✕ drops a queued item outright (the overflow draft above
    // stayed in the composer — clear it first so the re-queue below is the
    // only text).
    const chip3 = page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第三条' })
    await chip3.getByRole('button', { name: 'Remove queued message' }).click()
    await expect(page.getByTestId('prompt-queue')).toContainText('2 queued')
    await expect(chip3).toHaveCount(0)
    // Re-queue it: after the removal there is room again, and the item
    // joins at the TAIL — so the drain order below stays 一 → 二 → 三.
    await chat.composer().fill('队列第三条')
    await chat.composer().press('Enter')
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第三条' })).toBeVisible()
    await expect(chips.nth(2)).toContainText('队列第三条')

    // The long turn settles → the drain auto-sends the queue in ITS order:
    // 第一条 → 第二条 → 第三条. The drained ORDER is pinned on the USER
    // bubbles (each carries the text actually sent); the replies come from
    // the script by turn position, so they are order-neutral.
    await expect(chat.bubbles()).toHaveCount(8, { timeout: 30_000 })
    await expect(chat.bubbleAt(2)).toContainText('队列第一条')
    await expect(chat.bubbleAt(4)).toContainText('队列第二条')
    await expect(chat.bubbleAt(6)).toContainText('队列第三条')
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)
    expect((await mockSnapshot(page)).sentTurns).toBe(4)
    await expectNoConsoleErrors(page)
  })

  test('Ctrl+Enter steer beats a parked queue: stop → settle → send lands before the drain', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'queue-steer', test.info())
    // Retry the row click until the switch lands (the rail can swallow a
    // click during a list re-render under parallel load).
    await expect(async () => {
      await page.getByTestId(ROW).click()
      await expect(page.getByRole('heading', { name: 'Queue and steer' })).toBeVisible({ timeout: 5_000 })
    }).toPass({ timeout: 30_000 })

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
    // steer's user bubble appears BEFORE the queued item's. D6: the
    // cancelled turn 0's streamed partial (at least the first 600ms-gap
    // chunk) commits as a stopped-marked bubble at index 1, so the steer's
    // user bubble lands at index 2.
    await expect(chat.bubbleAt(1)).toContainText('潮汐之一')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await expect(chat.bubbleAt(2)).toContainText('加急：先回答这个', { timeout: 15_000 })
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第一条' })).toBeVisible()
    // The steer's reply commits (player turn 1). FINDING S-1 (recorded in
    // the report, not fixed): the delivered turn settles within the same
    // render batch it was sent in (a single-event turn), so the drain's
    // settle commit runs while hasPendingSteer() is still true — and after
    // the gate drops (refs; clearing them never re-renders) no dependency
    // changes again. The parked item stays parked past this run until some
    // other real settle/switch re-runs the drain effect. Current behavior
    // asserted below; the R4-shaped fix is a state-based pendingSteer.
    // Bubbles: user0 + the stopped partial + the steer user + its reply.
    await expect(chat.bubbles()).toHaveCount(4, { timeout: 15_000 })
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: '队列第一条' })).toBeVisible()
    expect((await mockSnapshot(page)).sentTurns).toBe(2)
    await expectNoConsoleErrors(page)
  })
})

test.describe('scripted chat backend — queue-attachments-only (D9-b pin)', () => {
  test('attachments-only input queues with the count-placeholder chip while streaming and drains into a real send', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'queue-attachments-only', test.info())
    await expect(async () => {
      await page.getByTestId('desktop-session-row-script-sess-queue-att').click()
      await expect(page.getByRole('heading', { name: 'Queue attachments only' })).toBeVisible({ timeout: 5_000 })
    }).toPass({ timeout: 30_000 })

    // Turn 0 streams for ~3s — the queueing window.
    await chat.send('写一篇关于潮汐的长文')
    await chat.expectStreamingCursor()

    // Attach one file (demo drag-drop bridge) and clear the text: the
    // composer is now attachments-only. Enter must JOIN the queue (A-9)
    // instead of silently no-oping.
    await emitWebviewDrop(page, [ATT_PATH])
    await expect(page.getByRole('button', { name: `Remove ${ATT_NAME}` })).toBeVisible()
    await chat.composer().fill('')
    await chat.composer().press('Enter')

    // The queued item renders the attachmentsOnly PLACEHOLDER — the count
    // text, not a blank label (the exact branch D9-b pins).
    const chip = page.getByTestId('prompt-queue-chip')
    await expect(chip).toHaveText(/1 attachment/)
    await expect(page.getByTestId('prompt-queue')).toContainText('1 queued')

    // The long turn settles → the drain auto-sends the head: an
    // attachments-only turn (message '' + the path), not a dropped item.
    // Bubble layout: [0] text user, [1] text reply, [2] attachments-only
    // user (its FileCard carries the filename), [3] the drained reply.
    await expect(chat.bubbles()).toHaveCount(4, { timeout: 30_000 })
    await expect(chat.bubbleAt(2)).toContainText(ATT_NAME)
    await chat.expectBubbleText(3, '回复：附件已收到。')
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)

    const sends = (await mockSnapshot(page)).sends
    expect(sends).toHaveLength(2)
    expect(sends[0]).toMatchObject({ message: '写一篇关于潮汐的长文', attachments: null })
    expect(sends[1]).toMatchObject({ message: '', attachments: [ATT_PATH] })
    await expectNoConsoleErrors(page)
  })
})

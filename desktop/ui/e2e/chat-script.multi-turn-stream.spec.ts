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

  // G11 (wave-2): Ctrl+F in-conversation search — the NON-virtualized path
  // (short history). The jump is a scrollIntoView landing on the match and
  // the counter pins 1/1; Esc closes and hands focus back to the composer.
  test('Ctrl+F over a short conversation: single-match landing, wrap, focus return', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'multi-turn-stream', test.info())

    await chat.send(turn(0).user)
    await expect(chat.sendButton()).toBeVisible({ timeout: 20_000 })
    await expect(chat.bubbles()).toHaveCount(2)

    await page.keyboard.press('Control+f')
    const bar = page.getByTestId('chat-search-bar')
    await expect(bar).toBeVisible({ timeout: 5_000 })

    await bar.getByRole('textbox').fill('潮水退去')
    const count = bar.getByTestId('chat-search-count')
    await expect(count).toHaveText('1/1')
    // The match flashes its ring on landing; the bubble is in view.
    await expect(chat.bubbleAt(1)).toBeVisible()

    // Enter / Shift+Enter wrap a single match without leaving it.
    await bar.getByRole('textbox').press('Enter')
    await expect(count).toHaveText('1/1')
    await bar.getByRole('textbox').press('Shift+Enter')
    await expect(count).toHaveText('1/1')

    // A miss renders the error-colored zero state and disables navigation.
    await bar.getByRole('textbox').fill('zzz-no-such-token')
    await expect(count).toHaveText('0')
    await expect(bar.getByRole('button', { name: 'Next match (Enter)' })).toBeDisabled()
    await expect(bar.getByRole('button', { name: 'Previous match (Shift+Enter)' })).toBeDisabled()

    // Esc closes the bar and returns focus to the composer.
    await bar.getByRole('textbox').press('Escape')
    await expect(bar).toHaveCount(0)
    await expect(chat.composer()).toBeFocused()
    await expectNoConsoleErrors(page)
  })

  // G11 (wave-2): the virtualized path — 32 seeded messages exceed
  // VIRTUALIZE_THRESHOLD(30), so only a window of bubbles exists in the DOM
  // and the jump MUST go through virtualizer.scrollToIndex (a plain
  // scrollIntoView could never reach an unmounted row).
  test('Ctrl+F over a virtualized long history: scrollToIndex jump, wrap across matches, empty state', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'search-virtualized', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    // Virtualized: 32 seeded messages, but the DOM only holds a window.
    const rendered = await chat.bubbles().count()
    expect(rendered).toBeGreaterThan(0)
    expect(rendered).toBeLessThan(32)

    await page.keyboard.press('Control+f')
    const bar = page.getByTestId('chat-search-bar')
    await expect(bar).toBeVisible({ timeout: 5_000 })

    // needle-42 lives at index 5 (assistant) and index 28 (user) — far
    // outside the initially rendered window. The first jump lands on it via
    // scrollToIndex: the row MOUNTS into the DOM.
    await bar.getByRole('textbox').fill('needle-42')
    const count = bar.getByTestId('chat-search-count')
    await expect(count).toHaveText('1/2')
    await expect(chat.bubbleAt(5)).toBeVisible({ timeout: 5_000 })

    // Enter wraps forward to the second match; Shift+Enter wraps back.
    await bar.getByRole('textbox').press('Enter')
    await expect(count).toHaveText('2/2')
    await expect(chat.bubbleAt(28)).toBeVisible({ timeout: 5_000 })
    await bar.getByRole('textbox').press('Shift+Enter')
    await expect(count).toHaveText('1/2')
    await expect(chat.bubbleAt(5)).toBeVisible()

    // Zero results: the error-toned counter, navigation disabled.
    await bar.getByRole('textbox').fill('zzz-no-such-token')
    await expect(count).toHaveText('0')
    await expect(bar.getByRole('button', { name: 'Next match (Enter)' })).toBeDisabled()

    await bar.getByRole('textbox').press('Escape')
    await expect(bar).toHaveCount(0)
    await expect(chat.composer()).toBeFocused()
    await expectNoConsoleErrors(page)
  })
})

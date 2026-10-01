// R1 chat-testing infra — first scripted E2E journeys over the ScriptedBackend.
//
// `happy-path` (plan §4 journey #1): welcome → input → 3 streamed chunks
// (中文 + emoji) → usage → completed, asserting the full state machine in a
// real browser: stop button during streaming, typing cursor, bubble commit
// === chunk concatenation, composer reset, zero console errors.
//
// The second test re-runs the SAME script as a variant driven through
// `window.__shannonMock.control` — pauseAt parks the player mid-script and
// the streaming cursor must persist until resume() releases it.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'

const POEM = '帮我写一首关于海的短诗'
const REPLY = '好的，这是一首关于海的短诗：\n\n海浪轻拍沙滩 🌊'

test.describe('scripted chat backend — happy-path', () => {
  test('streams a scripted reply end-to-end and settles clean', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'happy-path', test.info())

    // Welcome state — the seeded session has no messages, so the chat page
    // opens on the composer welcome instead of a conversation.
    await expect(page.getByRole('heading', { name: 'What can I help with?' })).toBeVisible({ timeout: 15_000 })
    await expect(chat.composer()).toBeVisible()

    await chat.send(POEM)

    // Mid-stream state: the send slot swaps to stop, the cursor blinks.
    await expect(chat.stopButton()).toBeVisible({ timeout: 5_000 })
    await chat.expectStreamingCursor()
    // Nothing committed while streaming — only the optimistic user bubble.
    await expect(chat.bubbles()).toHaveCount(1)

    // Completion: stop → send swap, cursor gone, bucket commits as a bubble.
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2)
    await chat.expectBubbleText(1, REPLY)

    // The run status pill settles out of sight and the console stays clean.
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  test('control.pauseAt parks the stream; resume() completes it', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'happy-path')

    // Park before step 1 (query:usage) — i.e. once the chunk step finishes.
    await page.evaluate(() => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { pauseAt(i: number): void } }
      }).__shannonMock
      if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
      mock.control.pauseAt(1)
    })

    await chat.send(POEM)
    await chat.expectStreamingCursor()

    // Parked: cursor + stop button persist and nothing commits, however long.
    await expectMockPhase(page, 'waitingUi')
    await expect(chat.stopButton()).toBeVisible()
    await expect(chat.bubbles()).toHaveCount(1)
    await expect(chat.bubbles()).toHaveCount(1, { timeout: 1_000 }) // still parked

    // Resume drives the remaining steps to completion.
    await page.evaluate(() => {
      (window as unknown as {
        __shannonMock: { control: { resume(): void } }
      }).__shannonMock.control.resume()
    })
    await expect(chat.sendButton()).toBeVisible({ timeout: 10_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await chat.expectBubbleText(1, REPLY)
    await expectNoConsoleErrors(page)
  })
})

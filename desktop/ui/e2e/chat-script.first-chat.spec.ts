// R2 journey #1（矩阵#1）— first-chat, extending R1's happy-path with the
// welcome-entry and aria-live halves of the story:
//   welcome empty state → click an example card → composer prefills →
//   ≥5 streamed chunks (中英混排 + emoji) → StreamStatusRegion announces
//   busy → complete → completion resets the composer.
//
// Every locator anchors on role / aria-label / data-testid (never prose that
// shifts with i18n), except the example-card name and the live-region texts,
// which are pinned by the en-US locale in playwright.config.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const EXAMPLE_PROMPT =
  'Draft a friendly follow-up email to a candidate who went silent after the onsite. Keep it short and warm.'
// Live-region announcements (chat.stream.status.*; pinned by locale en-US).
const STATUS_ACTIVE = 'Generating reply…'
const STATUS_DONE = 'Reply complete'

test.describe('scripted chat backend — first-chat (journey #1)', () => {
  test('welcome example → prefill → mixed-language stream with live announcements', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'first-chat', test.info())

    // Welcome empty state (seeded session has zero messages).
    await expect(page.getByRole('heading', { name: 'What can I help with?' })).toBeVisible({ timeout: 15_000 })

    // Click the email example card → the prompt lands in the composer.
    await page.getByRole('button', { name: /Draft a friendly follow-up email to a candidate/ }).click()
    await expect(chat.composer()).toHaveValue(EXAMPLE_PROMPT)

    // Send via Enter (text already prefilled — do not refill).
    await chat.composer().press('Enter')

    // aria-live region announces the busy transition (P2-17 single status
    // region — role=status + data-testid anchor).
    const statusRegion = page.locator('[role="status"][data-testid="stream-status-region"]')
    await expect(statusRegion).toHaveText(STATUS_ACTIVE, { timeout: 5_000 })

    // Mid-stream: stop swaps in, cursor blinks, nothing committed yet.
    await expect(chat.stopButton()).toBeVisible({ timeout: 5_000 })
    await chat.expectStreamingCursor()
    await expect(chat.bubbles()).toHaveCount(1)

    // The player parks nowhere on this script — assert streaming settles.
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(statusRegion).toHaveText(STATUS_DONE, { timeout: 10_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2)

    // Bubble text === chunk concatenation (no loss / duplication across the
    // 6 mixed 中/EN/emoji chunks — includes a mid-text newline pair).
    const script = readChatScript('first-chat') as ChatScript
    const chunks = script.turns[0]!.script.find(s => 'chunks' in s && s.chunks)
    expect(chunks && 'chunks' in chunks ? chunks.chunks!.length : 0).toBeGreaterThanOrEqual(5)
    const reply = chunks && 'chunks' in chunks ? chunks.chunks!.join('') : ''
    await chat.expectBubbleText(1, reply)

    // Completion resets the composer (send 复位) and the run pill settles.
    await expect(chat.composer()).toHaveValue('')
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  test('the scripted player parks at the waitFor marker only when scripted', async ({ page }) => {
    // Guard test: this script has no waitFor/permission steps, so a send
    // runs straight through to 'done' — pinning that journey #1 needs no
    // manual resume (keeps the spec honest against future script edits).
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'first-chat')
    await chat.send(EXAMPLE_PROMPT)
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expectMockPhase(page, 'done')
    await expectNoConsoleErrors(page)
  })
})

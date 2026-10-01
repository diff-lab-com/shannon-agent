// R2 journey #5（矩阵#5）— auth-error + mid-stream-fail.
//
//   auth: the query:failed payload carries the "authentication failed"
//   wording (the backend classifier keys on it) + error_kind:'auth' → the
//   dedicated provider-key banner (deep link, Update key) — NOT the plain
//   error banner.
//
//   mid-stream: 2 chunks then error_kind:'other' → plain banner + retry, no
//   ghost bubble; clicking Retry re-sends (sentTurns 1 → 2) and the session
//   continues. The retried turn is anchored to known bug A-3 (retry drops
//   the original attachments) — flip notes inline.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues, mockSnapshot } from './helpers/knownIssues'

const STORY = '给我讲一个关于海的故事'

test.describe('scripted chat backend — failure journeys (#5)', () => {
  test('auth failure routes to the dedicated API-key banner, not the plain one', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'auth-error', test.info())

    await chat.send('总结一下这个仓库的用途')
    await chat.expectStreamingCursor()

    // Dedicated auth banner: provider name + Settings → Models deep link.
    const banner = page.getByTestId('auth-error-banner')
    await expect(banner).toBeVisible({ timeout: 10_000 })
    await expect(banner).toContainText('API key rejected by Anthropic')
    await expect(banner).toContainText('Update the key in Settings → Models, then retry.')
    await expect(banner.getByRole('button', { name: 'Update key' })).toBeVisible()
    await expect(banner.getByRole('button', { name: 'Retry' })).toBeVisible()

    // The raw engine line stays out of the UI (that's the point of the
    // dedicated banner) and the composer is free again.
    await expect(page.getByText('invalid x-api-key')).toHaveCount(0)
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expectNoConsoleErrors(page)
  })

  test('mid-stream failure keeps the transcript clean; retry re-sends (A-3 anchored)', async ({ page }) => {
    test.setTimeout(60_000)
    annotateKnownIssues(test.info(), {
      'A-3': 'Retry drops the original attachments (ComposerRetryButton resends text only). '
        + 'Current behavior is asserted below; flip to "attachments preserved" when R4 lands '
        + 'and remove the knownIssue marker on the retry turn in e2e/scripts/mid-stream-fail.yaml.',
    })
    const chat = new ChatPage(page)
    await loadChatScript(page, 'mid-stream-fail', test.info())

    await chat.send(STORY)
    await chat.expectStreamingCursor()

    // Failure banner (plain variant: raw error line + Retry).
    await expect(page.getByText('upstream connection reset while streaming')).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible()

    // No ghost bubble: the partial stream is dropped, nothing committed.
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(1)
    expect((await mockSnapshot(page)).sentTurns).toBe(1)

    // Retry: the last user message is re-sent — the script's second turn is
    // consumed (its knownIssue'd chunk step is skipped, so the turn settles
    // immediately) and the banner clears.
    await page.getByRole('button', { name: 'Retry' }).click()
    await expect.poll(async () => (await mockSnapshot(page)).sentTurns, { timeout: 5_000 }).toBe(2)
    await expect(page.getByText('upstream connection reset while streaming')).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0)

    // The re-sent turn appended a second user bubble with the same text
    // (user bubbles carry chrome, not a .prose body — substring assert).
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText(STORY)

    // A-3 current behavior: the retried bubble carries no attachment chips —
    // the original attachment (story-notes.md) was dropped on the resend.
    // Flip this to toBeVisible()-style presence assertions when R4 fixes it.
    await expect(chat.bubbleAt(1).getByText('story-notes.md')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

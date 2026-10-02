// R2 journey #5（矩阵#5）— auth-error + mid-stream-fail.
//
//   auth: the query:failed payload carries the "authentication failed"
//   wording (the backend classifier keys on it) + error_kind:'auth' → the
//   dedicated provider-key banner (deep link, Update key) — NOT the plain
//   error banner.
//
//   mid-stream: 2 chunks then error_kind:'other' → plain banner + retry, no
//   ghost bubble; clicking Retry re-sends (sentTurns 1 → 2) and the session
//   continues. A-3 fixed (R4 group 1): the retry resend carries the last
//   user message's attachment paths — the retried bubble shows the
//   attachment chip and the wire log proves the preserved path.
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const script = readChatScript('mid-stream-fail') as ChatScript
const STORY = script.turns[0]!.user
const NOTES_PATH = script.turns[0]!.attachments![0]
const NOTES_NAME = NOTES_PATH.split('/').pop()!
const DRAFT_KEY = 'shannon.draft.script-sess-fail'

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

  test('mid-stream failure keeps the transcript clean; retry re-sends with its attachments (A-3 fixed)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    // Pre-seed the session draft with the attachment (plus the turn text):
    // opening the session restores the chip, so the failing turn goes out
    // WITH the attachment — exactly what the Retry resend must preserve.
    await page.addInitScript(([key, path, text]) => {
      localStorage.setItem(key, JSON.stringify({ text, attachments: [path], updatedAt: Date.now() }))
    }, [DRAFT_KEY, NOTES_PATH, STORY] as const)
    await loadChatScript(page, 'mid-stream-fail', test.info())

    // Same mount guard as the attachments journey: the draft-restore effect
    // keys on the visible-session CHANGE, so the Chat page must be up
    // before the row click.
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('desktop-session-row-script-sess-fail').click()
    await expect(page.getByRole('heading', { name: 'Mid-stream failure' })).toBeVisible({ timeout: 10_000 })
    // The draft-restore put the attachment chip back into the composer.
    await expect(page.getByRole('button', { name: `Remove ${NOTES_NAME}` })).toBeVisible({ timeout: 10_000 })

    await chat.send(STORY)
    await chat.expectStreamingCursor()

    // Failure banner (plain variant: raw error line + Retry).
    await expect(page.getByText('upstream connection reset while streaming')).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('button', { name: 'Retry' })).toBeVisible()

    // No ghost bubble: the partial stream is dropped, nothing committed.
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(1)
    const preRetry = await mockSnapshot(page)
    expect(preRetry.sentTurns).toBe(1)
    // The failing turn went out WITH the draft-restored attachment.
    expect(preRetry.sends[0]).toMatchObject({ turnIndex: 0, attachments: [NOTES_PATH] })

    // Retry: the last user message is re-sent — the script's second turn is
    // consumed (its chunk step plays again) and the banner clears.
    await page.getByRole('button', { name: 'Retry' }).click()
    await expect.poll(async () => (await mockSnapshot(page)).sentTurns, { timeout: 5_000 }).toBe(2)
    await expect(page.getByText('upstream connection reset while streaming')).toHaveCount(0)
    await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0)

    // The re-sent turn appended a second user bubble with the same text
    // (user bubbles carry chrome, not a .prose body — substring assert).
    // Its scripted reply replays and commits fast (a single chunk step), so
    // assert the settled transcript directly: user + reply = 3 bubbles.
    await expect(chat.bubbles()).toHaveCount(3, { timeout: 15_000 })
    await expect(chat.bubbleAt(1)).toContainText(STORY)

    // A-3 fixed: the retry resend kept the last user message's attachments —
    // the wire log carries the original path…
    const snapshot = await mockSnapshot(page)
    expect(snapshot.sends[1]).toMatchObject({
      turnIndex: 1,
      attachments: [NOTES_PATH],
    })
    // …and the retried bubble renders the attachment card. The reply bubble
    // (already settled above) carries the replayed scripted text.
    await expect(chat.bubbleAt(1).getByText('story-notes.md')).toBeVisible()
    await expect(chat.bubbleAt(2)).toContainText('重试后的流式回复')
    await expectNoConsoleErrors(page)
  })
})

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

  // A-1 fixed (R4 group 2): an idle direct send whose invoke REJECTS (the
  // scripted counterpart of the backend's pre-turn budget/concurrent/goal
  // guards) used to clear the composer unconditionally — the rejection
  // silently ate the user's input. Now the text comes back (same recovery
  // as the edit flow) and the debounced draft write re-persists it, so the
  // obvious retry (Enter again) sends exactly what was lost.
  test('a rejected send keeps the composer text and draft; resending succeeds (A-1 fixed)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'send-rejected', test.info())

    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await page.getByTestId('desktop-session-row-script-sess-reject').click()
    await expect(page.getByRole('heading', { name: 'Send rejected' })).toBeVisible({ timeout: 10_000 })

    const STORY = '这条会被 pre-turn 守卫拒绝'
    await chat.send(STORY)

    // The invoke rejection surfaces on the plain error banner (String(e) of
    // the thrown guard — no query ever started, so no stream/cursor).
    await expect(page.getByText('send rejected by scripted backend guard')).toBeVisible({ timeout: 10_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    // G13: the invoke rejection happened BEFORE any user message was
    // recorded (seed messages are empty and the optimistic append rolled
    // back) — with no history to re-send, the Retry affordance hides
    // entirely instead of rendering a dead button.
    await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0)

    // A-1 fixed: the composer still holds the rejected text…
    await expect(chat.composer()).toHaveValue(STORY)
    // …and its draft key is back (the send cleared it; the restore's
    // debounced write re-persists it within ~300ms).
    await expect.poll(
      async () => page.evaluate(() => localStorage.getItem('shannon.draft.script-sess-reject')),
      { timeout: 5_000 },
    ).toContain(STORY)

    // The rejected send consumed the turn and logged its args, but played
    // nothing and committed no bubbles (the optimistic one was rolled back).
    const preRetry = await mockSnapshot(page)
    expect(preRetry.sentTurns).toBe(1)
    expect(preRetry.sends).toHaveLength(1)
    await expect(chat.bubbles()).toHaveCount(0)

    // The obvious retry — Enter again on the restored text — consumes the
    // next turn and settles clean: user + reply bubbles, banner gone.
    await chat.send(STORY)
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expect(page.getByText('send rejected by scripted backend guard')).toHaveCount(0)
    await expect(chat.bubbleAt(1)).toContainText('重发成功的流式回复')
    expect((await mockSnapshot(page)).sentTurns).toBe(2)
    await expectNoConsoleErrors(page)
  })

  // G13 (wave-2): query:notice — the engine's self-healed recoveries
  // (failover / key rotation) surface as muted in-stream lines, NOT error
  // banners. They survive the run's completion until the session's next
  // send, and the bucket caps at 20 lines.
  test('stream notices render both kinds, survive completion, reset on the next send, and cap at 20', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'stream-notices', test.info())

    await expect(page.getByRole('heading', { name: 'Stream notices' })).toBeVisible({ timeout: 10_000 })
    const failover = page.getByTestId('stream-notice-failover')
    const rotation = page.getByTestId('stream-notice-key_rotation')
    await chat.send('讲讲故障转移时发生了什么')
    await chat.expectStreamingCursor()

    await expect(failover).toBeVisible({ timeout: 10_000 })
    await expect(failover).toContainText('Model fallback')
    await expect(failover).toContainText('upstream 429 — retried on the fallback model')
    await expect(rotation).toBeVisible()
    await expect(rotation).toContainText('API key rotation')
    await expect(rotation).toContainText('API key #1 rejected — rotated to key #2')

    // Settled (completed + committed): the lines are still there — the run
    // never failed, so nothing replaces them until the next send.
    await expect(chat.sendButton()).toBeVisible({ timeout: 20_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(failover).toBeVisible()
    await expect(rotation).toBeVisible()

    // The next send resets the notice slate for the session.
    await chat.send('第二条消息应当清掉 notice 行')
    await expect(chat.bubbles()).toHaveCount(4, { timeout: 15_000 })
    await expect(page.getByTestId('stream-notice-failover')).toHaveCount(0)
    await expect(page.getByTestId('stream-notice-key_rotation')).toHaveCount(0)

    // Cap: 22 injected notices keep only the newest 20 (the oldest spill
    // out of the bucket). emitNow bypasses the player state — same bridge
    // the script steps ride.
    await page.evaluate(() => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { emitNow(name: string, payload?: Record<string, unknown>): void } }
      }).__shannonMock
      for (let i = 0; i < 22; i++) {
        mock?.control.emitNow('query:notice', { kind: 'failover', message: `notice-${String(i).padStart(2, '0')}` })
      }
    })
    const lines = page.locator('[data-testid^="stream-notice-"]')
    await expect(lines).toHaveCount(20, { timeout: 10_000 })
    await expect(page.getByText('notice-00')).toHaveCount(0)
    await expect(page.getByText('notice-01')).toHaveCount(0)
    await expect(page.getByText('notice-02')).toBeVisible()
    await expect(page.getByText('notice-21')).toBeVisible()
    await expectNoConsoleErrors(page)
  })

  // G13 (wave-2): the goal-owned guard. The demo goal registry keeps a
  // RUNNING goal bound to sess-002, so the session seeded here is
  // goal-owned — any send is refused CLIENT-side before send_message, with
  // the goal banner (not a stream error), the composer text restored, and
  // — with no recorded history — no Retry button.
  test('a goal-owned session blocks sends with the goal banner; nothing reaches the player', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'goal-owned', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Goal owned' })).toBeVisible({ timeout: 10_000 })

    const BLOCKED = '这条会被 goal 守卫拦截'
    await chat.send(BLOCKED)

    const banner = page.getByText('A goal run is active on this session', { exact: false })
    await expect(banner).toBeVisible({ timeout: 10_000 })
    // The guard runs before any query exists: composer text comes back
    // (A-1 recovery), no bubbles, and the player never saw the send.
    await expect(chat.composer()).toHaveValue(BLOCKED)
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(0)
    const snapshot = await mockSnapshot(page)
    expect(snapshot.sentTurns).toBe(0)
    expect(snapshot.sends).toHaveLength(0)
    // No user message exists → nothing to re-send → no Retry affordance.
    await expect(page.getByRole('button', { name: 'Retry' })).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

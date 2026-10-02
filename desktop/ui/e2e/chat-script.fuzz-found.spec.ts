// R5 fuzz HARVEST — suspected bug F-1, frozen as a fixed journey per the
// task-5 brief §A ("发现的真实崩溃 → 转固定剧本并记入疑似 bug 清单").
//
// F-1: when a terminal `query:completed` carries a session_id that points
// at ANY other session, AppContext's QUERY_COMPLETED handler keys the
// settle on `p.session_id ?? visibleKey` — it releases the WRONG session's
// querying latch. The send latched the SENDING session (sendMessage →
// setSessionQuerying(targetSessionId, true)), so the sending session's
// composer never resets: stop button stuck up, no send button, no crash.
//
// This journey is the MINIMAL repro: text streams normally into the
// current session; ONLY the terminal is re-stamped with the other seeded
// session's id — so the wedge sits on top of a visibly normal stream.
//
// FLIP CONDITION (when the fix lands): the settle must key on the query's
// OWNER (or fall back to the sending session) — flip the wedge assertions
// below to the settled contract (send button back, bubble committed,
// watchdog clean), strike F-1 from the suspected-bug list in
// task-5-report.md, and drop the mutant entry from KNOWN_FUZZ_WEDGES in
// chat-script.fuzz.spec.ts.
//
// NIGHTLY-ONLY (runs with the scripted family under
// playwright.chat-nightly.config.ts).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

test.describe('chat-script fuzz harvest — suspected bug F-1 (frozen repro)', () => {
  test('F-1: a completed re-stamped with a foreign session_id wedges the sending session (tracked)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'fuzz-found-cross-session', test.info())
    test.info().annotations.push({
      type: 'suspectedBug',
      description: 'F-1: terminal query:completed with a foreign session_id settles the wrong session — the sending session\'s composer latch never releases (AppContext QUERY_COMPLETED keys on p.session_id ?? visibleKey)',
    })

    await chat.send('跑一个终态被错路由的回合')

    // The stream itself is normal: text lands in the visible bucket with
    // the cursor up.
    await chat.expectStreamingCursor()

    // The player settles its turn (terminal emitted; the single-turn script
    // lands phase 'done')…
    await expect
      .poll(async () => (await mockSnapshot(page)).phase, { timeout: 15_000, intervals: [100] })
      .toMatch(/^(armed|done)$/)

    // …but the APP never settles the visible session: the terminal was
    // routed to fuzz-sess-b, so THIS session's latch stays on. The wedge
    // IS the assertion — reproduced precisely, not idly awaited:
    await expect(chat.stopButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.sendButton()).toHaveCount(0)
    // The stream freezes mid-reply: cursor + partial text persist, nothing
    // commits (the terminal belonged to "someone else").
    await chat.expectStreamingCursor()
    await expect(chat.bubbles()).toHaveCount(1)
    await page.waitForTimeout(1_000)
    await expect(chat.bubbles()).toHaveCount(1)
    await expect(chat.bubbleAt(0)).toContainText('跑一个终态被错路由的回合')

    // No crash — F-1 is a latch leak, not an exception: the watchdog stays
    // silent throughout the wedge.
    await expectNoConsoleErrors(page)
  })
})

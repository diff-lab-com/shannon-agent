// R5 fuzz HARVEST — suspected bug F-1, frozen as a fixed journey per the
// task-5 brief §A ("发现的真实崩溃 → 转固定剧本并记入疑似 bug 清单").
// FLIPPED by the p0a1 fix round: the wedge is now the FIXED contract.
//
// F-1 (pre-fix): a terminal `query:completed` carrying a session_id that
// points at ANY other session keyed the settle on `p.session_id ?? visibleKey`
// — it released the WRONG session's querying latch while the send latched
// the SENDING session, so the sending composer never reset (stop stuck up,
// no send button, no crash).
//
// FIXED CONTRACT (p0a1): every query:* event belongs to the session THAT
// SENT the query — AppContext resolves the query_id against the A-17/G6
// send records and settles/routes there; the payload's session_id is only
// the fallback for unresolvable ids (old shapes, another window's runs).
// A lying stamp is an anomaly: exactly one console.warn per such event
// (warn, not error — the watchdog stays green).
//
// This journey pins the minimal repro end-to-end: turn 1 streams normally
// into the current session with ONLY the terminal re-stamped to the other
// seeded session — the sending composer must unlock, the bubble must
// commit, and the session must be immediately reusable (turn 2 plays and
// settles with correctly-stamped events).
//
// The same contract is asserted at the L1 state-machine layer
// (chatStateMachine.scripts.test.tsx, "fuzz-found-cross-session (F-1
// fixed)") so the two layers cannot drift.
//
// NIGHTLY-ONLY (runs with the scripted family under
// playwright.chat-nightly.config.ts).
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

test.describe('chat-script fuzz harvest — F-1 fixed (flipped frozen repro)', () => {
  test('F-1: a completed re-stamped with a foreign session_id settles the SENDING session (composer unlocks, bubble commits, one warn)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'fuzz-found-cross-session', test.info())
    test.info().annotations.push({
      type: 'regression',
      description: 'F-1 fix: a terminal query:completed with a foreign session_id must settle the session that SENT the query (query_id owner routing) — composer unlocks, bubble commits, exactly one console.warn',
    })

    // Collect the anomaly trace: one warn per lying event, never an error.
    // (CDP names the level 'warning' — the watchdog's 'error' spelling is
    // the exception, not the rule.)
    const mismatchWarns: string[] = []
    page.on('console', (msg) => {
      if (msg.type() === 'warning' && msg.text().includes('session_id mismatch')) {
        mismatchWarns.push(msg.text())
      }
    })

    // ── Turn 1: normal stream, terminal re-stamped to fuzz-sess-b ──
    await chat.send('跑一个终态被错路由的回合')

    // The stream itself is normal: text lands in the visible bucket with
    // the cursor up.
    await chat.expectStreamingCursor()

    // The player settles its turn (terminal emitted; the single-turn-then-
    // next script lands phase 'armed').
    await expect
      .poll(async () => (await mockSnapshot(page)).phase, { timeout: 15_000, intervals: [100] })
      .toBe('armed')

    // FIXED: the app settles the SENDING session — the wrong-sid terminal
    // released THIS session's latch (pre-fix: stop stuck up, send gone
    // forever). Composer fully reset, stream committed.
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.stopButton()).toHaveCount(0)
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText(
      '流式文本正常进入当前会话，但终态会被盖成别人的 session_id。',
    )

    // The lying stamp is an anomaly — exactly one warn, nothing more.
    expect(mismatchWarns).toHaveLength(1)
    expect(mismatchWarns[0]).toContain('query:completed')
    expect(mismatchWarns[0]).toContain('"fuzz-sess-b"')

    // No crash — the fix ABSORBS the bad event (a latch leak pre-fix, an
    // exception never): the watchdog stays silent throughout.
    await expectNoConsoleErrors(page)

    // ── Turn 2: the composer is immediately reusable — correctly-stamped
    // events stream and settle normally, with no additional warns. (No
    // mid-stream cursor assert here: a 2-chunk stream gives the throttled
    // projection only a few hundred ms of visibility — the settle + the
    // committed bubbles are the reusable proof.) ──
    await chat.send('立即再发一轮（composer 已解锁）')
    await expect
      .poll(async () => (await mockSnapshot(page)).phase, { timeout: 15_000, intervals: [100] })
      .toBe('done')

    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.stopButton()).toHaveCount(0)
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(4)
    await expect(chat.bubbleAt(3)).toContainText('第二轮正常流转，composer 全程可用。')
    expect(mismatchWarns).toHaveLength(1)
    await expectNoConsoleErrors(page)
  })
})

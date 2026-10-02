// Wave-2 journey #21 — multi-window（方案 §9.4 J21 / G5）. NIGHTLY-ONLY
// (D8): playwright.config.ts testIgnore keeps this family out of the PR
// gate; playwright.chat-nightly.config.ts re-includes it.
//
// Two-page model: pageA is the main window (loadChatScript → /chat, cold
// start binds the first seeded session s1 — A-6 fix); pageB boots as a
// SESSION WINDOW at /chat?windowSession=<s2-id> with its own armed copy of
// the same script. Each page gets its own mock realm + event bridge, so
// every assertion here is "each page's OWN filter projection":
//   - pageB ignores s1 events (inject via control.emitNow with an explicit
//     session_id — the raw-bridge probe of AppContext's
//     isEventForCurrentWindow filter) while answering its own s2 stream;
//   - pageA renders both sessions' streams across switches;
//   - the permission alertdialog is filtered BY WINDOW (AppContext:1292-1300
//     gates on isEventForCurrentWindow only): the main window pops the
//     dialog for ANY session even while displaying another — pinned as-is;
//   - the reveal affordances (window badge + "open in main window") are
//     pinned as PRESENT but not clicked: reveal_session_in_main is
//     allowlisted-unmocked ("demo has one window"), so a click would only
//     produce an error toast; the true cross-window handoff is L3.
import { expect, test, type Page, type TestInfo } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { attachConsoleWatchdog, expectNoConsoleErrors } from './helpers/watchdog'
import { loadChatScript, readChatScript } from './helpers/scriptLoader'
import type { ChatScript } from '../src/lib/mock/scripted/schema'

const S1_ID = '11111111-1111-4111-8111-111111111111'
const S2_ID = '22222222-2222-4222-8222-222222222222'
const SESSION_WINDOW_URL = `/chat?windowSession=${S2_ID}`

const script = readChatScript('multi-window') as ChatScript

/**
 * pageB twin of loadChatScript with a custom URL: arm the script via
 * addInitScript (the seed must land before the app's first fetch), attach
 * the console watchdog BEFORE the goto, then boot straight into the
 * session-window URL — parseWindowSession only accepts well-formed UUIDs.
 */
async function loadChatScriptAt(page: Page, url: string, testInfo?: TestInfo): Promise<void> {
  await page.addInitScript((value) => {
    (window as unknown as { __SHANNON_SCRIPT__?: unknown }).__SHANNON_SCRIPT__ = value
  }, script)
  attachConsoleWatchdog(page)
  await page.goto(url)
  if (testInfo) {
    await testInfo.attach('chat-script', { body: JSON.stringify(script, null, 2), contentType: 'application/json' })
  }
}

/** Fire one event through pageB's own bridge, bypassing its player state. */
function emitNow(page: Page, event: string, payload: Record<string, unknown>): Promise<void> {
  return page.evaluate(
    ([name, inner]) => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { emitNow(name: string, payload?: Record<string, unknown>): void } }
      }).__shannonMock
      if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
      mock.control.emitNow(name as string, inner as Record<string, unknown>)
    },
    [event, payload],
  )
}

test.describe('scripted chat backend — multi-window (journey #21, nightly-only)', () => {
  /** Click a session row and retry until the switch lands — the rail can
   *  swallow a click during a list re-render (same guard as journey #11). */
  async function openSession(page: Page, id: string, heading: string): Promise<void> {
    await expect(async () => {
      await page.getByTestId(`desktop-session-row-${id}`).click()
      await expect(page.getByRole('banner').locator('h2')).toHaveText(heading, { timeout: 5_000 })
    }).toPass({ timeout: 30_000 })
  }

  test('main window renders both seeded sessions across switches (buckets never cross)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'multi-window', test.info())

    // A-6 fixed: the main window cold-start binds the FIRST seeded session.
    await expect(page.getByRole('banner').locator('h2')).toHaveText('Main Session One', { timeout: 10_000 })

    // Turn 0 → auto payload session_id = send target = s1.
    await chat.send('第一轮问题')
    await expect(chat.bubbleAt(0)).toContainText('第一轮问题')
    await expect(chat.bubbleAt(1)).toContainText('第一轮流三段', { timeout: 15_000 })

    // Switch to s2; its own turn renders under its own session.
    await openSession(page, S2_ID, 'Window Session Two')
    await chat.send('第二轮问题')
    await expect(chat.bubbleAt(0)).toContainText('第二轮问题')
    await expect(chat.bubbleAt(1)).toContainText('第二轮流三段', { timeout: 15_000 })

    // Back to s1: the S-4 send tail survives the reload (user turn), and
    // neither session's stream bleeds into the other. NOTE (mock fidelity,
    // pinned): the scripted backend records only USER sends — an assistant
    // reply commits to React state when its session is visible but has no
    // scripted commit path on reload, so the s1 reply is gone after the
    // round trip. The real engine records turns backend-side; this is the
    // known scripted-world projection, not asserted as product behavior.
    await openSession(page, S1_ID, 'Main Session One')
    await expect(chat.bubbleAt(0)).toContainText('第一轮问题')
    await expect(chat.bubbles()).toHaveCount(1)
    await expect(page.getByText('第一轮流三段')).toHaveCount(0)
    await expect(page.getByText('第二轮流三段')).toHaveCount(0)

    await expectNoConsoleErrors(page)
  })

  test('session window filters foreign-session events and renders its own (reveal chrome pinned)', async ({ browser }) => {
    test.setTimeout(60_000)
    const context = await browser.newContext()
    const pageA = await context.newPage()
    const pageB = await context.newPage()
    const chatA = new ChatPage(pageA)

    await loadChatScript(pageA, 'multi-window')
    await loadChatScriptAt(pageB, SESSION_WINDOW_URL)

    // Window-mode chrome: the badge and the two window controls exist; the
    // sidebar rail does NOT. Not clicking the controls — see file header.
    const header = pageB.getByRole('banner')
    await expect(header.getByText('Session window')).toBeVisible({ timeout: 10_000 })
    await expect(header.getByRole('button', { name: 'Open this session in the main window' })).toBeVisible()
    await expect(header.getByRole('button', { name: 'Close this session window' })).toBeVisible()
    await expect(pageB.getByRole('complementary')).toHaveCount(0)

    // Foreign-session stream into the session window: s1 text must not
    // render, must not lock the composer (no stop button — the send slot is
    // not hijacked — and the composer still accepts typing), and must leave
    // the message list empty.
    await emitNow(pageB, 'query:text', { query_id: 'q-foreign-1', session_id: S1_ID, content: 's1 流一段 ' })
    await emitNow(pageB, 'query:text', { query_id: 'q-foreign-1', session_id: S1_ID, content: 's1 流二段 ' })
    await expect(pageB.locator('.streaming-cursor')).toHaveCount(0)
    await expect(pageB.getByRole('button', { name: 'Stop generation' })).toHaveCount(0)
    await expect(pageB.locator('[data-message-index]')).toHaveCount(0)
    await expect(pageB.getByText('s1 流一段')).toHaveCount(0)
    // Positive "not locked" proof: the composer accepts input and the send
    // button re-enables (an empty composer is disabled BY DESIGN — the
    // probe is that a foreign stream cannot keep it that way).
    await pageB.getByRole('textbox', { name: 'Message' }).fill('composer alive')
    await expect(pageB.getByRole('button', { name: 'Send message' })).toBeEnabled()

    // A own-session send renders: the player stamps the auto payload with
    // the send's target (= the window's pinned s2) and pageB streams it.
    const chatB = new ChatPage(pageB)
    await chatB.send('第一轮问题')
    await expect(chatB.bubbleAt(0)).toContainText('第一轮问题')
    await expect(chatB.bubbleAt(1)).toContainText('第一轮流三段', { timeout: 15_000 })
    await expect(pageB.getByText('s1 流一段')).toHaveCount(0)

    // The main window is unaffected (independent realm) and still works.
    await expect(pageA.getByRole('banner').locator('h2')).toHaveText('Main Session One', { timeout: 10_000 })
    await chatA.send('第一轮问题')
    await expect(chatA.bubbleAt(1)).toContainText('第一轮流三段', { timeout: 15_000 })

    await expectNoConsoleErrors(pageA)
    // Window mode's Layout syncs the NATIVE window title
    // (getCurrentWindow().setTitle → plugin:window|set_title), which demo
    // mode has no handler for — an allowlisted demo gap (OS window
    // surface), caught by the app's best-effort catch. Unrelated to the
    // event-filter behavior under test.
    await expectNoConsoleErrors(pageB, [/plugin:window\|set_title/])
    await context.close()
  })

  test('permission alertdialog follows the window filter (main window pops for any session — pinned as-is)', async ({ browser }) => {
    test.setTimeout(60_000)
    const context = await browser.newContext()
    const pageA = await context.newPage()
    const pageB = await context.newPage()

    await loadChatScript(pageA, 'multi-window')
    await loadChatScriptAt(pageB, SESSION_WINDOW_URL)
    await expect(pageB.getByRole('banner').getByText('Session window')).toBeVisible({ timeout: 10_000 })

    // Session window + foreign (s1) request → filtered, no dialog.
    await emitNow(pageB, 'permission-request', {
      request_id: 'pr-mw-s1',
      tool: 'Bash',
      risk: 'high',
      session_id: S1_ID,
      input: { command: 'echo foreign' },
      reason: { source: 'rule', ruleName: 'shell-command' },
    })
    await expect(pageB.getByRole('alertdialog')).toHaveCount(0)

    // Session window + own (s2) request → the dialog pops here.
    await emitNow(pageB, 'permission-request', {
      request_id: 'pr-mw-s2',
      tool: 'Bash',
      risk: 'high',
      session_id: S2_ID,
      input: { command: 'echo own' },
      reason: { source: 'rule', ruleName: 'shell-command' },
    })
    const dialogB = pageB.getByRole('alertdialog')
    await expect(dialogB).toBeVisible()
    await expect(dialogB.getByText('echo own')).toBeVisible()
    // Deny to clear the prompt (also the respond_permission ledger path).
    await dialogB.getByRole('button', { name: 'Deny' }).click()
    await expect(dialogB).toHaveCount(0)

    // PINNED CURRENT STATE: the main window (windowSessionId === null)
    // accepts EVERY permission request regardless of which session it is
    // DISPLAYING — the filter is window-level, not display-level. pageA is
    // showing s1; an s2 request still pops the dialog here.
    await emitNow(pageA, 'permission-request', {
      request_id: 'pr-mw-a-s2',
      tool: 'Bash',
      risk: 'high',
      session_id: S2_ID,
      input: { command: 'echo cross' },
      reason: { source: 'rule', ruleName: 'shell-command' },
    })
    const dialogA = pageA.getByRole('alertdialog')
    await expect(dialogA).toBeVisible()
    await expect(dialogA.getByText('echo cross')).toBeVisible()
    await dialogA.getByRole('button', { name: 'Deny' }).click()
    await expect(dialogA).toHaveCount(0)

    await expectNoConsoleErrors(pageA)
    // Same window-title demo gap as above (pageB boots in window mode).
    await expectNoConsoleErrors(pageB, [/plugin:window\|set_title/])
    await context.close()
  })
})

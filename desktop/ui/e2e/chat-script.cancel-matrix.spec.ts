// R3 §4.1 — cancel-matrix，全部 9 场景逐行（一个 spec，一个场景一个 test）。
//
// 行动方案：场景 1/5/7/8 复用既有 journey YAML；场景 2/3/4/9 用专用
// cancel-*.yaml；迟到事件（旧查询在事件边界的迟到投递、工具迟到 result）
// 用 control.emitNow 注入——播放器的 turn 机制表达不了「旧 turn 的终态
// 晚于新 turn」的交错，这正是 emitNow 的设计用途（bypass 播放器状态）。
//
// A-17（场景 3）是本轮最高价值产出：确认「stop 后立刻重发」时迟到的
// query:cancelled 会清空新流并误停 isQuerying。复现序列（同报告 §A-17）：
//   send(q-0 流中) → stop → cancelled 收敛 → 立刻 send(q-1 流中)
//   → 注入 q-0 迟到 text + q-0 迟到 cancelled → 新流被清空、composer 误复位。
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues, mockSnapshot } from './helpers/knownIssues'

type Page = import('@playwright/test').Page

/** Emit a raw event through the bridge, bypassing the player state. */
function emitNow(page: Page, event: string, payload: Record<string, unknown>): void {
  void page.evaluate(([name, pl]) => {
    (window as unknown as {
      __shannonMock: { control: { emitNow(name: string, payload: Record<string, unknown>): void } }
    }).__shannonMock.control.emitNow(name, pl)
  }, [event, payload] as const)
}

/** Invoke a Tauri command through the mock internals (explicit routing). */
function invokeMock(page: Page, cmd: string, args: Record<string, unknown>): void {
  void page.evaluate(([command, a]) => {
    (window as unknown as {
      __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> }
    }).__TAURI_INTERNALS__.invoke(command, a)
  }, [cmd, args] as const)
}

test.describe('scripted chat backend — cancel-matrix (§4.1, 9 scenarios)', () => {
  // ── 1. 文本流中 stop ──────────────────────────────────────────────────
  test('1. stop in a text stream: cancelled converges, stop→send swaps back, no residue', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-text-stream', test.info())
    await chat.send('写一首关于海的长诗，慢慢写')
    await chat.expectStreamingCursor()
    await expect(chat.bubbleAt(0)).toContainText('长诗')

    await chat.stop()
    // ≤2 chunk 内收敛：swap-back、无残留光标/状态、无失败痕迹。
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  // ── 2. 工具执行中 stop（无 progress 事件 = A-18 取消盲区形态）─────────
  test('2. stop during a tool run: the card converges and a late tool-result does not resurrect it', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-tool-run', test.info())
    await page.getByTestId('desktop-session-row-script-sess-cancel-tool').click()
    await expect(page.getByRole('heading', { name: 'Cancel tool run' })).toBeVisible({ timeout: 10_000 })

    await chat.send('跑一个很慢的命令')
    await expect(page.locator('[data-tool-name="Bash"][data-tool-status="running"]')).toBeVisible({ timeout: 5_000 })
    await expect(chat.runStatusLine()).toBeVisible()

    await chat.stop()
    // The run's UI converges away: card, pill, cursor.
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('[data-tool-name="Bash"]')).toHaveCount(0)
    await expect(chat.runStatusLine()).toHaveCount(0)
    await expect(chat.streamingCursor()).toHaveCount(0)

    // The old query's tool-result lands at its next event boundary — after
    // the run is gone. The result must NOT resurrect the card.
    emitNow(page, 'query:tool-result', {
      tool_use_id: 'tool-slow-1', result: 'done much later', is_error: false,
      session_id: 'script-sess-cancel-tool',
    })
    await expect(page.locator('[data-tool-name="Bash"]')).toHaveCount(0)
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  // ── 3. stop 后立刻重发（A-17 核心）────────────────────────────────────
  test('3. instant resend after stop: the late old-turn cancelled WIPES the new stream (A-17 pollution, recorded)', async ({ page }) => {
    test.setTimeout(60_000)
    annotateKnownIssues(test.info(), {
      'A-17': 'stop releases the querying latch immediately (commands_chat.rs) but the old loop '
        + 'only exits at its next engine event, and AppContext never filters by query_id — the '
        + 'late query:cancelled clears the session bucket and idles the composer while the NEW '
        + 'turn is still streaming. Repro: send q-0 → stop → cancelled settles → instantly send '
        + 'q-1 → inject late q-0 text + q-0 cancelled (emitNow). Current pollution asserted '
        + 'below; flip to "new stream intact, isQuerying stays true, full reply" when R4 lands.',
    })
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-then-resend', test.info())
    await page.getByTestId('desktop-session-row-script-sess-resend').click()
    await expect(page.getByRole('heading', { name: 'Cancel then resend' })).toBeVisible({ timeout: 10_000 })

    // q-0 streams → stop → cancelled settles.
    await chat.send('第一条（将被取消）')
    await chat.expectStreamingCursor()
    await expect(chat.bubbleAt(0)).toContainText('将被取消')
    await chat.stop()
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.bubbles()).toHaveCount(1)

    // 立刻重发 — the new turn (q-1) streams.
    await chat.send('第二条（stop 后立刻重发）')
    await chat.expectStreamingCursor()
    await expect(chat.bubbleAt(1)).toContainText('stop 后立刻重发')

    // The old query's boundary events land INSIDE the new turn's window.
    emitNow(page, 'query:text', {
      content: '[旧流迟到]', query_id: 'q-0', session_id: 'script-sess-resend',
    })
    emitNow(page, 'query:cancelled', { query_id: 'q-0', session_id: 'script-sess-resend' })

    // A-17 CURRENT BEHAVIOR: the late cancelled clears the new stream and
    // mis-idles the composer while q-1 is still playing.
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    // q-1 keeps streaming into the wiped bucket — its committed reply loses
    // everything emitted before the pollution point (and the injected late
    // chunk never shows).
    await expect(chat.bubbles()).toHaveCount(3, { timeout: 20_000 })
    await chat.expectBubbleText(2, '新流乙，新流丙，新流丁。')
    await expectNoConsoleErrors(page)
  })

  // ── 4. 审批等待中 stop ────────────────────────────────────────────────
  test('4. stop while the approval dialog waits: the run settles, the dialog lingers (current), a late respond still works', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-approval-wait', test.info())
    await page.getByTestId('desktop-session-row-script-sess-cancel-approval').click()
    await expect(page.getByRole('heading', { name: 'Cancel approval wait' })).toBeVisible({ timeout: 10_000 })

    await chat.send('删掉临时目录')
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible({ timeout: 10_000 })
    await expectMockPhase(page, 'waitingPermission')

    // CURRENT BEHAVIOR (recorded for the report): while the approval dialog
    // waits, the composer's stop button is UNREACHABLE — the modal scrim
    // intercepts pointer events (the click below would never land), and
    // Escape on the dialog maps to Modal's deny-on-close, not a cancel.
    // The engine still waits on respond_permission, so the reachable cancel
    // path is the explicit session route (what a second window's stop does).
    await invokeMock(page, 'cancel_query', { sessionId: 'script-sess-cancel-approval' })
    // The dialog's aria-modal masking keeps role queries blind to the
    // composer (R2 §5.5) — anchor the swap-back via the attribute locator.
    await expect(page.locator('button[aria-label="Send message"]')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.streaming-cursor')).toHaveCount(0)

    // CURRENT BEHAVIOR (recorded): QUERY_CANCELLED does not clear the
    // permissionRequest — the dialog stays open over a settled run.
    await expect(dialog).toBeVisible()

    // A late Deny still goes through: the command resolves, the dialog
    // closes, and the session never enters the error state.
    await dialog.getByRole('button', { name: 'Deny' }).click()
    await expect(dialog).toHaveCount(0, { timeout: 5_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)
    expect((await mockSnapshot(page)).permissionLog).toHaveLength(1)
    await expectNoConsoleErrors(page)
  })

  // ── 5. Escape 分层（编辑态优先）────────────────────────────────────────
  test('5. Escape layering: exits the edit first; a second press (streaming, no edit) cancels the query', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await page.getByTestId('desktop-session-row-script-sess-edit').click()
    await expect(page.getByRole('heading', { name: 'Edit and rewind' })).toBeVisible({ timeout: 10_000 })
    await expect(chat.bubbles()).toHaveCount(4)

    // Tier 1: editing → Escape exits the edit, nothing is cancelled.
    await page.locator('[data-message-index="0"]').hover()
    await page.getByRole('button', { name: 'Edit message' }).first().click()
    await expect(page.getByTestId('edit-banner')).toBeVisible()
    await chat.composer().press('Escape')
    await expect(page.getByTestId('edit-banner')).toHaveCount(0)
    await expect(chat.composer()).toHaveValue('')
    expect((await mockSnapshot(page)).phase).toBe('armed') // no turn was ever started

    // Tier 2: streaming (no edit) → Escape cancels the run.
    await chat.send('第一轮（编辑后）：潮汐的成因和周期')
    await chat.expectStreamingCursor()
    await chat.composer().press('Escape')
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })

  // ── 6. steer 竞态：15s 超时还稿 ───────────────────────────────────────
  test('6. steer whose cancel never settles hands the draft back after 15s with a notice', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    // first-chat: one quick scripted turn. After it is consumed, further
    // sends fall through to the default handler (no events) — a run that
    // NEVER settles, the mock's shape for "the cancel IPC failed".
    await loadChatScript(page, 'first-chat', test.info())
    await chat.send('帮我写一首关于海的诗')
    await expect(chat.sendButton()).toBeVisible({ timeout: 20_000 })

    // The stuck send: accepted (query id returned), but no query events
    // will ever arrive — the composer stays busy with nothing on screen.
    await chat.send('这条会悬住')
    await expect(chat.stopButton()).toBeVisible({ timeout: 5_000 })

    // Steer: cancel (a no-op — nothing to cancel) + parked pending. After
    // the 15s settle timeout the draft returns with a toast.
    await chat.composer().fill('加急（超时还稿）')
    await chat.composer().press('Control+Enter')
    await expect(chat.composer()).toHaveValue('')
    // CI fix (job 110627367229): the persistent OUTCOME is asserted first —
    // onSendRejected restores the draft, and the composer value survives —
    // with a window that tolerates a late fire on a loaded runner. The
    // notice toast rides the SAME callback (it mounts in that commit), so
    // by the time the composer assert passes the toast is at age ~0 and a
    // fresh window cannot miss it; its sonner lifetime is ~4s, so the
    // old order (polling for the toast from t+0 with 20s) raced a slow
    // runner instead.
    await expect(chat.composer()).toHaveValue('加急（超时还稿）', { timeout: 35_000 })
    const toast = page.locator('[data-sonner-toast]').filter({ hasText: 'Steered message could not be delivered' })
    await expect(toast).toBeVisible({ timeout: 20_000 })
    await expectNoConsoleErrors(page)
  })

  // ── 7. 预算触顶自动取消 ───────────────────────────────────────────────
  test('7. budget-cap auto-cancel: cancelled settles the run while the exceeded banner is up', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'budget-exceeded', test.info())
    await page.getByTestId('desktop-session-row-script-sess-budget').click()
    await expect(page.getByRole('heading', { name: 'Over budget' })).toBeVisible({ timeout: 10_000 })

    // The banner is already up (seed re-derivation) when the run starts.
    // Exceeded-only body suffix — variant anchor (see chat-script.budget
    // spec: the $-body prefix is shared with the warning variant).
    const banner = page.getByRole('alert').filter({ hasText: 'Choose how to proceed' })
    await expect(banner).toBeVisible({ timeout: 10_000 })
    await chat.send('再补充一下数据来源部分')
    await chat.expectStreamingCursor()

    // The cap cancels the turn (not a failure): convergence with no failed
    // residue, banner actions only usable after the settle.
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(3) // seeded history + the user bubble
    await expect(page.getByRole('img', { name: 'Last run failed' })).toHaveCount(0)
    await expect(banner).toBeVisible()
    await expectNoConsoleErrors(page)
  })

  // ── 8. 双击 stop / idle stop 无抖动 ───────────────────────────────────
  test('8. double-stop and idle stop: no error toast, no state flapping', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-text-stream', test.info())

    // Idle: no stop button exists to mis-click.
    await expect(chat.stopButton()).toHaveCount(0)

    await chat.send('写一首关于海的长诗，慢慢写')
    await chat.expectStreamingCursor()
    // Double-stop: the second cancel is a backend no-op (token taken) — or
    // the button has already swapped back to send; both are the calm paths.
    await chat.stop()
    await chat.stopButton().click({ timeout: 2_000 }).catch(() => { /* already swapped */ })
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.runStatusLine()).toHaveCount(0)
    // No error toast, exactly one settle, no flapping.
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(1)
    await expectNoConsoleErrors(page)
  })

  // ── 9. 后台会话取消（session_id 显式路由）─────────────────────────────
  test('9. background-session cancel: explicit cancel_query(sessionId) settles A while B stays untouched', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-background', test.info())
    await page.getByTestId('desktop-session-row-script-sess-bg-a').click()
    await expect(page.getByRole('heading', { name: 'Background A' })).toBeVisible({ timeout: 10_000 })
    await chat.send('A 的长任务')
    await chat.expectStreamingCursor()

    // Switch to B: A keeps streaming in the background, nothing bleeds.
    await page.getByTestId('desktop-session-row-script-sess-bg-b').click()
    await expect(page.getByRole('heading', { name: 'Foreground B' })).toBeVisible({ timeout: 10_000 })
    await expect(page.getByText('后台一', { exact: true })).toHaveCount(0)
    await expect(chat.stopButton()).toHaveCount(0)

    // Cancel A through the EXPLICIT session route (what a second window's
    // stop button does): phase settles, B's screen never reacts.
    await invokeMock(page, 'cancel_query', { sessionId: 'script-sess-bg-a' })
    await expect
      .poll(async () => (await mockSnapshot(page)).phase, { timeout: 10_000 })
      .toBe('done')
    await expect(page.getByRole('alert')).toHaveCount(0)
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)

    // Back to A: switching reloads the seeded conversation (the pre-switch
    // optimistic bubble does not survive a round trip), and the cancelled
    // run left no residue — no cursor, no assistant bubble.
    await page.getByTestId('desktop-session-row-script-sess-bg-a').click()
    await expect(page.getByRole('heading', { name: 'Background A' })).toBeVisible({ timeout: 10_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(page.locator('[data-tool-name]')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

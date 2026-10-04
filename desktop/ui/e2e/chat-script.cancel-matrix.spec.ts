// R3 §4.1 — cancel-matrix，全部 9 场景逐行（一个 spec，一个场景一个 test）。
//
// 行动方案：场景 1/5/7/8 复用既有 journey YAML；场景 2/3/4/9 用专用
// cancel-*.yaml；迟到事件（旧查询在事件边界的迟到投递、工具迟到 result）
// 用 control.emitNow 注入——播放器的 turn 机制表达不了「旧 turn 的终态
// 晚于新 turn」的交错，这正是 emitNow 的设计用途（bypass 播放器状态）。
//
// 场景 3 是 A-17 的回归锚（R4 组6 已修复）：「stop 后立刻重发」时旧查询
// 的迟到事件曾清空新流并误停 isQuerying。修复 = 双管齐下——后端 cancel
// 不再提前放开 querying 闩（commands_chat.rs，闩复位统一在查询循环退出），
// 前端按 query_id 过滤（AppContext 记录每次 send 响应的 query_id，携带
// 不同 id 的 query:*/terminal 事件按旧查询迟到投递丢弃，缺 id 的旧形状
// 放行）。复现序列（同报告 §A-17，注入形状不变）：
//   send(q-0 流中) → stop → cancelled 收敛 → 立刻 send(q-1 流中)
//   → 注入 q-0 迟到 text + q-0 迟到 cancelled → 新流完好、isQuerying 保持、
//   完整回复提交（本 spec 断言新契约）。
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { mockSnapshot } from './helpers/knownIssues'

type Page = import('@playwright/test').Page

// Exceeded-only body suffix (everything after the {budget} placeholder in
// en.json's budget.exceeded.body) — the same dynamic variant anchor the
// budget journey spec uses; a copy drift fails by name here too.
const EXCEEDED_BODY_SUFFIX = (
  JSON.parse(
    readFileSync(
      join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'i18n', 'locales', 'en.json'),
      'utf8',
    ),
  ) as Record<string, string>
)['budget.exceeded.body'].split('{budget}')[1]!.trim()

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

/**
 * Open a sidebar session and only accept the outcome when its heading is
 * up — the same retry-until-landed loop the R3 hardening (f8e7f0c1) gave
 * queue-steer/session-switch, applied here after a 4-core-stress rerun
 * reproduced the row-click swallow on this file (the click lands during
 * hydration and switches nothing; re-clicking is idempotent).
 */
async function openSession(page: Page, rowTestId: string, headingName: string): Promise<void> {
  await expect(async () => {
    await page.getByTestId(rowTestId).click()
    await expect(page.getByRole('heading', { name: headingName })).toBeVisible()
  }).toPass({ timeout: 15_000 })
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

  // ── 2. 工具执行中 stop（原 A-18 取消盲区形态，后端已修）───────────────
  test('2. stop during a tool run: the card converges and a late tool-result does not resurrect it', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-tool-run', test.info())
    await openSession(page, 'desktop-session-row-script-sess-cancel-tool', 'Cancel tool run')

    await chat.send('跑一个很慢的命令')
    await expect(page.locator('[data-tool-name="Bash"][data-tool-status="running"]')).toBeVisible({ timeout: 5_000 })
    await expect(chat.runStatusLine()).toBeVisible()

    await chat.stop()
    // A-18 FIXED backend-side (R4 group 7): the real backend races the
    // cancel token against the stream via tokio::select! (commands.rs
    // stream_step), so a stop lands immediately even while a silent tool
    // runs — it no longer waits for the tool's next event boundary. (The
    // scripted mock always converged on cancel, so the UI assertions are
    // unchanged; the window stays CI-slack, not a semantic bound.)
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

  // ── 3. stop 后立刻重发（A-17 已修复，回归锚）─────────────────────────
  test('3. instant resend after stop: late old-turn events are dropped by query_id, the new stream stays intact (A-17 fixed)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-then-resend', test.info())
    await openSession(page, 'desktop-session-row-script-sess-resend', 'Cancel then resend')

    // q-0 streams → stop → cancelled settles. D6: the streamed partial
    // commits as a stopped-marked assistant bubble. The first chunk is out
    // before the stop; how many of the 400ms-gap chunks landed by then is
    // timing-dependent, so the text is asserted by containment (the count
    // and the marker are the deterministic part).
    await chat.send('第一条（将被取消）')
    await chat.expectStreamingCursor()
    await expect(chat.bubbleAt(0)).toContainText('将被取消')
    await chat.stop()
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText('旧流一，')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()

    // 立刻重发 — the new turn (q-1) streams. Bubble order: user0, the
    // partial (stopped) reply for q-0, then the q-1 user bubble.
    await chat.send('第二条（stop 后立刻重发）')
    await chat.expectStreamingCursor()
    await expect(chat.bubbleAt(2)).toContainText('stop 后立刻重发')

    // The old query's boundary events land INSIDE the new turn's window.
    emitNow(page, 'query:text', {
      content: '[旧流迟到]', query_id: 'q-0', session_id: 'script-sess-resend',
    })
    emitNow(page, 'query:cancelled', { query_id: 'q-0', session_id: 'script-sess-resend' })

    // A-17 FIXED (AppContext query_id filter): the late q-0 events (≠ the
    // session's current q-1) are dropped — the composer stays BUSY while
    // q-1 keeps streaming, the committed q-0 partial stays untouched (its
    // stopped-marked bubble keeps exactly the pre-stop text), and the final
    // reply is the FULL new-turn text with no injected late chunk. (Was:
    // the late cancelled wiped the bucket, mis-idled the composer, and
    // truncated the reply to the post-pollution chunks.)
    await expect(chat.stopButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toBeVisible()
    await expect(page.getByText('[旧流迟到]')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(4, { timeout: 20_000 })
    await expect(chat.bubbleAt(1)).toContainText('旧流一，')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await chat.expectBubbleText(3, '新流甲，新流乙，新流丙，新流丁。')
    await expectNoConsoleErrors(page)
  })

  // ── 4. 审批等待中 stop（S-3 已修复：scrim 上方挂出可达的 stop；B1-2：弹窗随终态消亡）────────
  test('4. stop while the approval dialog waits: the portal stop settles the run and the dialog is dismissed with it (B1-2)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-approval-wait', test.info())
    await openSession(page, 'desktop-session-row-script-sess-cancel-approval', 'Cancel approval wait')

    await chat.send('删掉临时目录')
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible({ timeout: 10_000 })
    await expectMockPhase(page, 'waitingPermission')

    // S-3 FIXED (R4 group 7): the composer's stop is UNREACHABLE while the
    // dialog is up — the modal scrim covers it and the composer's glass
    // surface is a `contain: paint` stacking context, so it can never win
    // the z race (this click used to time out; recorded in task-3 §1.C).
    // The fix mounts a portal stop control ABOVE the scrim for exactly this
    // window. Clicking it goes through the same cancel_query IPC — and is
    // NOT a Deny: the dialog only closes on a press targeting its own
    // backdrop element (Escape=Deny semantics untouched, decision D6).
    const portalStop = page.getByTestId('header-stop-while-waiting')
    await expect(portalStop).toBeVisible()
    await portalStop.click()

    // The run settles: composer swaps back, cursor gone, the portal control
    // disappears with the settled run.
    // The dialog's aria-modal masking keeps role queries blind to the
    // composer (R2 §5.5) — anchor the swap-back via the attribute locator.
    await expect(page.locator('button[aria-label="Send message"]')).toBeVisible({ timeout: 5_000 })
    await expect(page.locator('.streaming-cursor')).toHaveCount(0)
    await expect(portalStop).toHaveCount(0)

    // B1-2 FIXED (was: QUERY_CANCELLED left the dialog open over the settled
    // run and a late Deny was the only thing closing it): the run's terminal
    // event dismisses its own pending approval prompt — the backend
    // auto-Denies the orphaned request after its 300s timeout, so the dialog
    // must not outlive the run.
    await expect(dialog).toHaveCount(0, { timeout: 5_000 })
    await expect(page.getByRole('alert')).toHaveCount(0)
    expect((await mockSnapshot(page)).permissionLog).toHaveLength(0)
    await expectNoConsoleErrors(page)
  })

  // ── 5. Escape 分层（编辑态优先）────────────────────────────────────────
  test('5. Escape layering: exits the edit first; a second press (streaming, no edit) cancels the query', async ({ page }) => {
    test.setTimeout(90_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'edit-rewind', test.info())
    await openSession(page, 'desktop-session-row-script-sess-edit', 'Edit and rewind')
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
    // Budget math (CI-hardened): fill+press actionability ≈2s under load,
    // then useSteerSend's REAL window.setTimeout(15s) (useSteerSend.ts:70)
    // — a page-side timer that can fire late by seconds on a starved
    // 2-core runner — then the restored draft + toast mount in the same
    // callback commit. 90s ceiling = those waits plus the 60s poll window
    // below, with slack; the toast assert rides right behind the poll, so
    // its age is ~0 when reached.
    test.setTimeout(90_000)
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
    // CI fix (job 110627367229, then 110656923971's 35s toHaveValue miss):
    // the persistent OUTCOME is polled with expect.poll — the composer
    // value flips exactly once when onSendRejected('timeout') fires, and
    // polling (not a single-pass windowed match) cannot miss a late fire
    // on a loaded runner. The notice toast rides the SAME callback (it
    // mounts in that commit), so by the time the poll passes the toast is
    // at age ~0 and a fresh window cannot miss it; its sonner lifetime is
    // ~4s, so the original order (polling for the toast from t+0 with 20s)
    // was the only racy variant.
    await expect
      .poll(async () => chat.composer().inputValue(), { timeout: 60_000, interval: 250 })
      .toBe('加急（超时还稿）')
    const toast = page.locator('[data-sonner-toast]').filter({ hasText: 'Steered message could not be delivered' })
    await expect(toast).toBeVisible({ timeout: 20_000 })
    await expectNoConsoleErrors(page)
  })

  // ── 7. 预算触顶自动取消 ───────────────────────────────────────────────
  test('7. budget-cap auto-cancel: cancelled settles the run while the exceeded banner is up', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'budget-exceeded', test.info())
    await openSession(page, 'desktop-session-row-script-sess-budget', 'Over budget')

    // The banner is already up (seed re-derivation) when the run starts.
    // Exceeded-only body suffix — variant anchor (see chat-script.budget
    // spec: the $-body prefix is shared with the warning variant; the
    // suffix is derived from en.json by key).
    const banner = page.getByRole('alert').filter({ hasText: EXCEEDED_BODY_SUFFIX })
    await expect(banner).toBeVisible({ timeout: 10_000 })
    await chat.send('再补充一下数据来源部分')
    await chat.expectStreamingCursor()

    // The cap cancels the turn (not a failure): convergence with no failed
    // residue, banner actions only usable after the settle. D6: the three
    // chunks streamed before the cap fired commit as a stopped-marked
    // partial bubble.
    await expect(chat.sendButton()).toBeVisible({ timeout: 15_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(4) // seeded history + user bubble + the stopped partial
    await expect(chat.bubbleAt(3).getByTestId('message-stopped-marker')).toBeVisible()
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
    // Double-stop: the second press either lands on the disabled
    // "cancelling" state (S-3/A-18 companion feedback — in-flight marker
    // until the settle) or the button has already swapped back to send;
    // both are the calm paths, no error toast either way.
    await chat.stop()
    await chat.stopButton().click({ timeout: 2_000 }).catch(() => { /* already swapped */ })
    await expect(chat.sendButton()).toBeVisible({ timeout: 5_000 })
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(chat.runStatusLine()).toHaveCount(0)
    // No error toast, exactly one settle, no flapping. D6: the partial
    // commits with its stopped marker (its length rides the 5s chunk gaps —
    // at least the first chunk is out).
    await expect(page.locator('[data-sonner-toast]')).toHaveCount(0)
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(1)).toContainText('海浪拍岸')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await expectNoConsoleErrors(page)
  })

  // ── 9. 后台会话取消（session_id 显式路由）─────────────────────────────
  test('9. background-session cancel: explicit cancel_query(sessionId) settles A while B stays untouched', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'cancel-background', test.info())
    await openSession(page, 'desktop-session-row-script-sess-bg-a', 'Background A')
    await chat.send('A 的长任务')
    await chat.expectStreamingCursor()

    // Switch to B: A keeps streaming in the background, nothing bleeds.
    await openSession(page, 'desktop-session-row-script-sess-bg-b', 'Foreground B')
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

    // Back to A (S-4 fixed in R4 group 3): the pre-switch user bubble
    // survives the round trip — the scripted backend now records accepted
    // sends at turn start (seed.ts overlay), matching the real backend's L0
    // tee (agent_loop records the user message before the model sees
    // anything, so a log-backed reload always includes it). D6: the
    // cancelled run's streamed partial ALSO survives the round trip — the
    // cancel recorded it into the tail (interrupted: true, the scripted
    // counterpart of the L0 interrupted-turn finalize) — so the reload
    // shows the user bubble plus the stopped-marked partial carrying at
    // least the first chunk.
    await openSession(page, 'desktop-session-row-script-sess-bg-a', 'Background A')
    await expect(chat.bubbles()).toHaveCount(2)
    await expect(chat.bubbleAt(0)).toContainText('A 的长任务')
    await expect(chat.bubbleAt(1)).toContainText('后台一，')
    await expect(chat.bubbleAt(1).getByTestId('message-stopped-marker')).toBeVisible()
    await expect(chat.streamingCursor()).toHaveCount(0)
    await expect(page.locator('[data-tool-name]')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

// R3 §4.2 — 输入缓存锚点（chat-input-persistence）。
//
// 草稿（shannon.draft.<id>，300ms 防抖 + 切换同步 flush）：跨会话隔离、
// 发送清空、重启（reload）恢复、>64KB 仅内存（A-21 已修复：仍不落盘，
// 但不再静默——console.warn + 一次性 toast，不阻塞输入；断言翻转为
// 检查提示出现，重启后仍为空输入框）。
// 队列（纯内存，AppContext PROMPT_QUEUE_*）：跨会话 parked 返回仍在、
// 重启丢失（knownIssue A-20 —— reload 后队列蒸发）。
// cap/排序/移除/drain 顺序在 journey #9 的 spec；本文件只锚「存续策略」。
//
// A-22（输入历史回溯不存在）：产品 gap 记录，不建测试 —— ArrowUp 仅用于
// mention/slash 菜单导航（ChatInput.tsx handleKeyDown），除非产品立项。
import { expect, test } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import { annotateKnownIssues } from './helpers/knownIssues'

const DRAFT_A = 'shannon.draft.script-sess-draft-a'
const DRAFT_B = 'shannon.draft.script-sess-draft-b'
const ROW_A = 'desktop-session-row-script-sess-draft-a'
const ROW_B = 'desktop-session-row-script-sess-draft-b'

async function openDraftSession(page: import('@playwright/test').Page, row: string, heading: string): Promise<ChatPage> {
  const chat = new ChatPage(page)
  // Mount guard: the draft-restore effect keys on the visible-session
  // CHANGE — clicking before the Chat page mounted would skip the restore
  // (and the debounced empty-write would clear the stored draft).
  await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
  await page.getByTestId(row).click()
  await expect(page.getByRole('heading', { name: heading })).toBeVisible({ timeout: 10_000 })
  return chat
}

test.describe('scripted chat backend — input persistence (§4.2)', () => {
  test('drafts: cross-session isolation with the switch flush (R2-W1 anchor), each session keeps its own', async ({ page }) => {
    test.setTimeout(60_000)
    await loadChatScript(page, 'input-persistence', test.info())
    const chat = await openDraftSession(page, ROW_A, 'Drafts A')

    // Type and switch INSIDE the 300ms debounce window: the switch path
    // flushes the old session's draft synchronously (B1 §4-11).
    await chat.composer().fill('A 的草稿')
    await page.getByTestId(ROW_B).click()
    await expect(page.getByRole('heading', { name: 'Drafts B' })).toBeVisible()
    // The flush landed despite the sub-debounce switch.
    const flushed = await page.evaluate(([a, b]) => ({
      a: localStorage.getItem(a), b: localStorage.getItem(b),
    }), [DRAFT_A, DRAFT_B] as const)
    expect(flushed.a).toContain('A 的草稿')
    expect(flushed.b).toBeNull()
    // B's composer is empty — A's draft never bleeds across.
    await expect(chat.composer()).toHaveValue('')

    // Round trip: A's draft comes back; B's stays empty.
    await page.getByTestId(ROW_A).click()
    await expect(page.getByRole('heading', { name: 'Drafts A' })).toBeVisible()
    await expect(chat.composer()).toHaveValue('A 的草稿')
    await page.getByTestId(ROW_B).click()
    await expect(chat.composer()).toHaveValue('')
    await expectNoConsoleErrors(page)
  })

  test('drafts: sending clears the draft key', async ({ page }) => {
    test.setTimeout(60_000)
    await loadChatScript(page, 'input-persistence', test.info())
    const chat = await openDraftSession(page, ROW_A, 'Drafts A')
    await chat.composer().fill('发出去的一条')
    // Past the debounce — the draft is on disk (poll: the 300ms timer plus
    // React's effect scheduling do not bound to a fixed sleep).
    await expect.poll(async () => page.evaluate(k => localStorage.getItem(k), DRAFT_A), { timeout: 5_000 })
      .toContain('发出去的一条')

    await chat.composer().press('Enter')
    await expect(chat.composer()).toHaveValue('')
    await expect(await page.evaluate(k => localStorage.getItem(k), DRAFT_A)).toBeNull()
    // The scripted turn completes normally.
    await expect(chat.bubbles()).toHaveCount(2, { timeout: 15_000 })
    await expectNoConsoleErrors(page)
  })

  test('drafts: a reload restores the persisted draft (restart anchor)', async ({ page }) => {
    test.setTimeout(60_000)
    await loadChatScript(page, 'input-persistence', test.info())
    const chat = await openDraftSession(page, ROW_A, 'Drafts A')
    await chat.composer().fill('重启后仍在')
    await expect.poll(async () => page.evaluate(k => localStorage.getItem(k), DRAFT_A), { timeout: 5_000 })
      .toContain('重启后仍在') // debounce write landed
    await page.reload()
    await openDraftSession(page, ROW_A, 'Drafts A')
    await expect(chat.composer()).toHaveValue('重启后仍在')
    await expectNoConsoleErrors(page)
  })

  // A-21 fixed (flip): an oversized draft is still not persisted (the 64KB
  // localStorage cap), but the skip is no longer silent — the composer warns
  // on console and raises exactly one toast, without blocking the input.
  // The restart anchor below is unchanged: a reload comes back empty.
  test('drafts over 64KB stay in memory only, with a one-shot warning (A-21 fixed) — gone after reload', async ({ page }) => {
    test.setTimeout(60_000)
    const consoleWarnings: string[] = []
    page.on('console', msg => {
      if (msg.type() === 'warning') consoleWarnings.push(msg.text())
    })
    await loadChatScript(page, 'input-persistence', test.info())
    const chat = await openDraftSession(page, ROW_A, 'Drafts A')
    await chat.composer().fill('大'.repeat(40_000) + '字'.repeat(30_000)) // > 64KB JSON
    // The debounced write runs — and skips the oversized payload. Settle
    // past any write attempt, then assert the key never appeared.
    await page.waitForTimeout(900)
    expect(await page.evaluate(k => localStorage.getItem(k), DRAFT_A)).toBeNull()
    // A-21: the skip surfaces — the composer keeps the text and the user is
    // told the draft is window-bound (one toast, no re-notify spam).
    await expect(chat.composer()).toHaveValue(/大/)
    await expect(page.getByText(/64KB persistence cap/)).toBeVisible()
    expect(consoleWarnings.filter(w => w.includes('persistence cap'))).toHaveLength(1)
    await page.reload()
    await openDraftSession(page, ROW_A, 'Drafts A')
    await expect(chat.composer()).toHaveValue('')
    await expectNoConsoleErrors(page)
  })

  test('queues: parked per session across a switch, WIPED by a reload (A-20 anchored)', async ({ page }) => {
    test.setTimeout(90_000)
    annotateKnownIssues(test.info(), {
      'A-20': 'The prompt queue (cap 3) is pure in-memory state (AppContext.tsx PROMPT_QUEUE_*) — '
        + 'a reload/restart drops parked prompts while drafts persist, an inconsistent policy '
        + '(plan §4.2, decision D7 pending). Current behavior asserted below.',
    })
    // session-switch-race: two sessions + a long (8 × 700ms) turn to queue
    // against.
    await loadChatScript(page, 'session-switch-race', test.info())
    const chat = await openDraftSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await chat.send('A 的长问题')
    await chat.expectStreamingCursor()
    await chat.send('parked 的排队消息')
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: 'parked 的排队消息' })).toBeVisible()

    // The chips belong to THEIR session: gone from B, intact on return.
    await page.getByTestId('desktop-session-row-script-sess-race-b').click()
    await expect(page.getByRole('heading', { name: 'Race B' })).toBeVisible()
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)
    await page.getByTestId('desktop-session-row-script-sess-race-a').click()
    await expect(page.getByRole('heading', { name: 'Race A' })).toBeVisible()
    await expect(page.getByTestId('prompt-queue-chip').filter({ hasText: 'parked 的排队消息' })).toBeVisible()

    // Restart: the queue evaporates (A-20) while the persisted draft does
    // not (the D anchors above).
    await page.reload()
    await openDraftSession(page, 'desktop-session-row-script-sess-race-a', 'Race A')
    await expect(page.getByTestId('prompt-queue')).toHaveCount(0)
    await expectNoConsoleErrors(page)
  })
})

// Wave-2 journey — i18n-theme（方案 §9.4 G15 + G14 空态补强）. NIGHTLY-ONLY
// (D8): playwright.config.ts testIgnore keeps this family out of the PR
// gate; playwright.chat-nightly.config.ts re-includes it.
//
// Coverage:
//   1. locale boot reflection: `shannon.locale` drives the whole tree and
//      <html lang> (i18n/index.tsx) — pinned from the storage key the
//      settings switcher writes, per the brief's "addInitScript 置值最稳".
//   2. LIVE locale switch mid-stream through the REAL settings UI (the only
//      setLocale entry point): /chat → Settings → 简体中文 → back — the
//      stream keeps accumulating across the unmount/remount and the final
//      bubble is the exact chunk sum (流式中切语言不丢流).
//   3. The H1 single-theme appearance page visited mid-stream through the
//      REAL settings UI: exactly one theme card (Aurora Ink), the click is a
//      no-op — <html> never flips, the Toaster keeps following the app theme
//      and the parked stream survives the round trip (流式中逛外观不丢流).
//   4. H1 migration at boot: a stored pre-H1 value ('system' or a dormant
//      theme pick) normalizes to aurora-ink, a live OS scheme flip cannot
//      move it, and the parked stream survives the boot (迁移后流式不丢).
//   5. G14: the sidebar sessions skeleton in the boot's catalogLoading
//      window (seeded empty roster), handing over to the empty-state card.
// session-switch-overlay is NOT repeated here — chat-script.session-switch
// already owns it (brief: 已有 spec 不重复).
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { attachConsoleWatchdog, expectNoConsoleErrors } from './helpers/watchdog'
import { loadChatScriptObject } from './helpers/scriptLoader'

/** Five slow chunks — a wide window to navigate away and back mid-stream. */
const STREAM_CHUNKS = ['语言一 ', '语言二 ', '语言三 ', '语言四 ', '语言五']
const STREAM_FULL = STREAM_CHUNKS.join('')

async function gotoChat(page: Page, composerName = 'Message'): Promise<ChatPage> {
  attachConsoleWatchdog(page)
  await page.goto('/chat')
  const chat = new ChatPage(page)
  // The composer's accessible name is LOCALE-DEPENDENT (Message/消息) —
  // callers booting with a non-en shannon.locale pass their own name.
  await expect(page.getByRole('textbox', { name: composerName })).toBeVisible({ timeout: 15_000 })
  return chat
}

test.describe('i18n / theme (G15, nightly-only)', () => {
  test('shannon.locale drives the boot: zh-CN composer label + <html lang>', async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon.locale', 'zh-CN')
    })
    await gotoChat(page, '消息')

    await expect(page.getByRole('textbox', { name: '消息' })).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN')

    await expectNoConsoleErrors(page)
  })

  test('live locale switch mid-stream: the stream survives, the final bubble is the exact chunk sum', async ({ page }) => {
    await loadChatScriptObject(page, {
      name: 'i18n-theme-stream-hold',
      description: 'slow stream to switch the language mid-flight through the settings UI',
      seed: { config: { hasKey: true }, sessions: [{ id: 'i18n-hold-sess', title: 'i18n hold', messages: [] }] },
      turns: [
        {
          user: '边流式边切语言',
          script: [
            { event: 'query:text', chunks: STREAM_CHUNKS, chunkDelayMs: 1000 },
            { event: 'query:completed' },
          ],
        },
      ],
    })
    const chat = new ChatPage(page)
    await expect(page.getByRole('textbox', { name: 'Message' })).toBeVisible({ timeout: 15_000 })
    await expect(page.locator('html')).toHaveAttribute('lang', 'en')

    await chat.send('边流式边切语言')
    await chat.expectStreamingCursor()

    // The REAL switch path (GeneralSettings is the only setLocale caller):
    // in-app navigation, so no reload — AppContext (and the stream buckets)
    // stay alive while /chat is unmounted.
    await page.getByRole('link', { name: 'Settings' }).click()
    await page.getByRole('button', { name: '简体中文' }).click()
    // Immediate reflection, app-wide, without a reload.
    await expect(page.locator('html')).toHaveAttribute('lang', 'zh-CN')

    // The chrome re-rendered in the new locale too — navigate back by its
    // zh-CN label (nav.chat), which is itself the G15 projection.
    await page.getByRole('link', { name: '对话' }).click()
    await expect(page.getByRole('textbox', { name: '消息' })).toBeVisible({ timeout: 15_000 })

    // The stream kept running while away: it settles into the EXACT chunk
    // sum — nothing lost to the unmount/remount or the locale swap.
    await expect(chat.bubbleAt(1)).toBeVisible({ timeout: 20_000 })
    await chat.expectBubbleText(1, STREAM_FULL)
    await expect(page.getByRole('button', { name: 'Stop generation' })).toHaveCount(0)

    await expectNoConsoleErrors(page)
  })

  test('the single-theme appearance page is inert mid-stream: card click is a no-op, the Toaster follows the app theme, the stream survives', async ({ page }) => {
    await loadChatScriptObject(page, {
      name: 'theme-stream-hold',
      description: 'parked stream so the appearance-page visit happens mid-stream',
      seed: { config: { hasKey: true }, sessions: [{ id: 'theme-hold-sess', title: 'Theme hold', messages: [] }] },
      turns: [
        {
          user: '边流式边逛外观设置',
          script: [
            { event: 'query:text', chunks: ['主题流一 ', '主题流二 '], chunkDelayMs: 50 },
            { waitFor: 'ui' },
          ],
        },
      ],
    })
    const chat = new ChatPage(page)
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })

    // sonner mounts the [data-sonner-toaster] ol only while a toast lives,
    // so summon one through the app's own watchdog-clean path (the
    // auto-unarchive success toast) before asserting the Toaster. Toasts
    // stack, so visibility anchors on the FRONT toast only.
    const frontToast = page.locator('[data-sonner-toast][data-front="true"]')
    async function summonToast(): Promise<void> {
      await page.evaluate(() => {
        (window as unknown as {
          __shannonMock: { control: { emitNow(name: string, payload?: Record<string, unknown>): void } }
        }).__shannonMock.control.emitNow('session-auto-unarchived', { session_id: 'theme-hold-sess', title: 'Theme hold' })
      })
      await expect(frontToast).toBeVisible({ timeout: 10_000 })
    }

    // Dark-first default without a stored theme (verdict A 2026-10-15:
    // the default is aurora-ink, the ui-redesign-2026-10 design base).
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'aurora-ink')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    await summonToast()
    await expect(page.locator('[data-sonner-toaster]')).toHaveAttribute('data-sonner-theme', 'dark')

    await chat.send('边流式边逛外观设置')
    await chat.expectStreamingCursor()

    // H1 theme convergence: the appearance page carries exactly ONE theme
    // card (Aurora Ink) and it is not a picker — clicking it is a no-op,
    // <html> never flips, and the Toaster keeps following the app theme.
    await page.getByRole('link', { name: 'Settings' }).click()
    // The settings nav link was renamed 'Appearance' (nav.theme, G1 parity
    // batch) — the old 'Theme' locator never matched and the spec timed out
    // here before reaching the assertions below.
    await page.getByRole('link', { name: 'Appearance' }).click()
    await expect(page.getByText('Aurora Ink', { exact: true })).toBeVisible()
    await expect(page.getByText('The theme is defined centrally by the app.')).toBeVisible()
    await page.getByText('Aurora Ink', { exact: true }).click()
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'aurora-ink')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    await summonToast()
    await expect(page.locator('[data-sonner-toaster]')).toHaveAttribute('data-sonner-theme', 'dark')

    // Mid-stream continuity across the appearance visit: come back to a
    // still parked stream, the already-streamed text untouched.
    await page.getByRole('link', { name: 'Chat' }).click()
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('主题流一 主题流二', { exact: false })).toBeVisible()
    await page.evaluate(() => (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume())
    await chat.expectBubbleText(1, '主题流一 主题流二')

    await expectNoConsoleErrors(page)
  })

  test('a stored pre-H1 theme value (system / tokyo-night) migrates to aurora-ink at boot', async ({ page }) => {
    // H1 theme convergence: the retired 'system' mode and every dormant
    // theme pick normalize to aurora-ink on boot — the OS scheme (Playwright
    // defaults to light) must NOT leak through a stale 'system' value, and
    // <html> must land dark both ways.
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon-theme', 'system')
    })
    await loadChatScriptObject(page, {
      name: 'theme-system-stream-hold',
      description: 'parked stream to pin the migrated boot stays stable mid-flight',
      seed: { config: { hasKey: true }, sessions: [{ id: 'theme-system-hold-sess', title: 'System hold', messages: [] }] },
      turns: [
        {
          user: '边流式边验证迁移',
          script: [
            { event: 'query:text', chunks: ['系统流一 ', '系统流二 '], chunkDelayMs: 50 },
            { waitFor: 'ui' },
          ],
        },
      ],
    })
    const chat = new ChatPage(page)
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })

    // Stored 'system' + a light OS scheme: aurora-ink anyway (no OS-follow).
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'aurora-ink')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    // The migration commits to storage.
    await expect(page.evaluate(() => window.localStorage.getItem('shannon-theme'))).resolves.toBe('aurora-ink')

    // A live OS flip is a no-op now — nothing observes prefers-color-scheme.
    await page.emulateMedia({ colorScheme: 'dark' })
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'aurora-ink')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')

    // The parked stream survives the boot/migration and settles into the
    // EXACT chunk sum — nothing lost.
    await chat.send('边流式边验证迁移')
    await chat.expectStreamingCursor()
    await page.evaluate(() => (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume())
    await expect(page.getByText('系统流一 系统流二', { exact: false })).toBeVisible()
    await chat.expectBubbleText(1, '系统流一 系统流二')
    await expect(page.getByRole('button', { name: 'Stop generation' })).toHaveCount(0)

    // And the dormant tokyo-night pick migrates identically.
    const page2 = await page.context().newPage()
    await page2.addInitScript(() => {
      window.localStorage.setItem('shannon-theme', 'tokyo-night')
    })
    await page2.goto('/')
    await expect(page2.locator('html')).toHaveAttribute('data-theme', 'aurora-ink')
    await expect(page2.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    await page2.close()

    await expectNoConsoleErrors(page)
  })
})

test.describe('sidebar empty-state skeleton (G14, nightly-only)', () => {
  test('the sessions skeleton shows in the catalogLoading window, then the empty-state card takes over', async ({ page }) => {
    await loadChatScriptObject(page, {
      name: 'sidebar-empty-seed',
      description: 'empty seeded roster to expose the boot skeleton and the empty state',
      seed: { config: { hasKey: true }, sessions: [] },
      turns: [{ user: 'unused', script: [{ event: 'query:completed' }] }],
    })

    // The skeleton renders on the FIRST commit (sessions=[] + loading) and
    // lives until the boot fetches settle — catch it, then the handoff.
    await expect(page.getByTestId('sidebar-sessions-skeleton')).toBeVisible({ timeout: 5_000 })
    await expect(page.getByTestId('sidebar-sessions-skeleton')).toHaveCount(0, { timeout: 15_000 })
    await expect(page.getByText('Start your first chat')).toBeVisible()

    await expectNoConsoleErrors(page)
  })
})

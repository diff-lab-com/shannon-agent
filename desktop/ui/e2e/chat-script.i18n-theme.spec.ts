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
//   3. LIVE theme switch mid-stream through the REAL settings UI (the only
//      setTheme entry point): /chat → Settings → Material — the stream keeps
//      accumulating and the final bubble is the exact chunk sum.
//   4. LIVE OS scheme flip with theme='system' via colorScheme emulation
//      (ThemeContext's media-query listener is a real user path, no reload):
//      F-theme-system — data-theme/data-theme-mode recompute instantly both
//      directions, the app-level Toaster follows the RESOLVED theme
//      (App.tsx ThemedToaster → sonner's data-sonner-theme), and the
//      parked stream survives the flips (流式中切主题不丢流).
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

  test('live theme switch mid-stream: <html> flips, the Toaster follows the resolved theme, the stream survives', async ({ page }) => {
    await loadChatScriptObject(page, {
      name: 'theme-stream-hold',
      description: 'parked stream so the theme flip happens mid-stream',
      seed: { config: { hasKey: true }, sessions: [{ id: 'theme-hold-sess', title: 'Theme hold', messages: [] }] },
      turns: [
        {
          user: '边流式边切主题',
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

    // Dark-first default without a stored theme.
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'tokyo-night')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    await summonToast()
    await expect(page.locator('[data-sonner-toaster]')).toHaveAttribute('data-sonner-theme', 'dark')

    await chat.send('边流式边切主题')
    await chat.expectStreamingCursor()

    // The REAL switch path (ThemeSettings is the only setTheme caller):
    // in-app navigation → click the theme card → <html> flips instantly,
    // and the Toaster follows the RESOLVED theme (G4), not a fixed value.
    await page.getByRole('link', { name: 'Settings' }).click()
    await page.getByRole('link', { name: 'Theme' }).click()
    // exact: 'Tokyo Night' must not substring-match "Tokyo Night Light".
    await page.getByRole('button', { name: 'Tokyo Night', exact: true }).waitFor({ timeout: 10_000 })
    await page.getByRole('button', { name: 'Material', exact: true }).click()
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'material')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'light')
    await summonToast()
    await expect(page.locator('[data-sonner-toaster]')).toHaveAttribute('data-sonner-theme', 'light')

    // Mid-stream continuity across the theme swap: come back to a still
    // parked stream, the already-streamed text untouched.
    await page.getByRole('link', { name: 'Chat' }).click()
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })
    await expect(page.getByText('主题流一 主题流二', { exact: false })).toBeVisible()
    await page.evaluate(() => (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume())
    await chat.expectBubbleText(1, '主题流一 主题流二')

    await expectNoConsoleErrors(page)
  })

  test('live OS scheme flip with theme=system: <html> recomputes, the Toaster follows, the stream survives', async ({ page }) => {
    // F-theme-system, flipped from the pinned FINDING: ThemeContext's
    // prefers-color-scheme listener used to "refresh" via
    // setThemeState('system') — the SAME value — so React bailed out and a
    // system-theme user's live OS switch (light↔dark) left data-theme
    // stale. The listener now writes the real new scheme into state, so
    // resolvedTheme, <html data-theme(-mode)> and the resolved-theme
    // Toaster recompute instantly — mid-stream too.
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon-theme', 'system')
    })
    await loadChatScriptObject(page, {
      name: 'theme-system-stream-hold',
      description: 'parked stream so the OS scheme flip happens mid-stream',
      seed: { config: { hasKey: true }, sessions: [{ id: 'theme-system-hold-sess', title: 'System hold', messages: [] }] },
      turns: [
        {
          user: '边流式边切系统主题',
          script: [
            { event: 'query:text', chunks: ['系统流一 ', '系统流二 '], chunkDelayMs: 50 },
            { waitFor: 'ui' },
          ],
        },
      ],
    })
    const chat = new ChatPage(page)
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })

    // sonner mounts the [data-sonner-toaster] ol only while a toast lives;
    // summon one through the app's own watchdog-clean path. Toasts stack,
    // so visibility anchors on the FRONT toast only.
    const frontToast = page.locator('[data-sonner-toast][data-front="true"]')
    async function summonToast(): Promise<void> {
      await page.evaluate(() => {
        (window as unknown as {
          __shannonMock: { control: { emitNow(name: string, payload?: Record<string, unknown>): void } }
        }).__shannonMock.control.emitNow('session-auto-unarchived', { session_id: 'theme-system-hold-sess', title: 'System hold' })
      })
      await expect(frontToast).toBeVisible({ timeout: 10_000 })
    }

    // Playwright's default colorScheme is light → 'system' resolves to the
    // light material scheme.
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'material')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'light')

    // Park a stream mid-flight, then flip the OS scheme under it.
    await chat.send('边流式边切系统主题')
    await chat.expectStreamingCursor()

    // OS flips to dark: the resolved theme recomputes LIVE, and the Toaster
    // follows the RESOLVED theme (G4), not a fixed value.
    await page.emulateMedia({ colorScheme: 'dark' })
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'tokyo-night')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'dark')
    await summonToast()
    await expect(page.locator('[data-sonner-toaster]')).toHaveAttribute('data-sonner-theme', 'dark')

    // And back to light — both directions, still mid-stream.
    await page.emulateMedia({ colorScheme: 'light' })
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'material')
    await expect(page.locator('html')).toHaveAttribute('data-theme-mode', 'light')

    // Mid-stream continuity across the scheme flips: the parked stream is
    // intact and settles into the EXACT chunk sum — nothing lost.
    await page.evaluate(() => (window as unknown as { __shannonMock: { control: { resume(): void } } }).__shannonMock.control.resume())
    await expect(page.getByText('系统流一 系统流二', { exact: false })).toBeVisible()
    await chat.expectBubbleText(1, '系统流一 系统流二')
    await expect(page.getByRole('button', { name: 'Stop generation' })).toHaveCount(0)

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

// U2 — /chat has a single header. The retired per-page ChatHeader carried
// role="banner" too, which used to give /chat two banners.
//
// P1.5 (chat-testing plan v2 §9.5) — this spec also DEMOS the central testid
// registry (e2e/helpers/testids.ts): new anchors are asserted via
// getByTestId(testids.x) so a literal drift fails by registry key, and the
// templated ids come from testidTemplates instead of hand-built strings.
import { test, expect } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { expectMockPhase, loadChatScript, loadChatScriptObject } from './helpers/scriptLoader'
import { testids, testidTemplates } from './helpers/testids'

test.describe('Chat header meta (U2)', () => {
  test('/chat has exactly one banner', async ({ page }) => {
    await page.goto('/chat')

    // Mock sessions render in batches; clicking before the list settles lets
    // late rows shift the target mid-click (CI-only flake). Wait for the
    // last seeded session row before interacting.
    await expect(
      page.getByTestId('desktop-session-row-sess-008')
    ).toBeVisible({ timeout: 15000 })    // CI only: slow CI hydrates the sidebar's CSS variables asynchronously,
    // so the session button stays under the aside for the first click. Wait
    // for the sidebar to report a non-zero width and for the layout to
    // settle before interacting.
    await page.waitForFunction(() => {
      const aside = document.querySelector('aside[data-sidebar]')
      if (!aside) return false
      // aside must be sized AND the main column must be offset
      return aside.getBoundingClientRect().width > 0 &&
             getComputedStyle(document.documentElement).getPropertyValue('--sidebar-w').trim().endsWith('px')
    }, { timeout: 15000 })

    await expect(page.getByRole('banner')).toHaveCount(1)
  })

  test('switching a session updates the global Header title', async ({ page }) => {
    await page.goto('/chat')

    // Mock sessions render in batches; clicking before the list settles lets
    // late rows shift the target mid-click (CI-only flake). Wait for the
    // last seeded session row before interacting.
    await expect(
      page.getByTestId('desktop-session-row-sess-008')
    ).toBeVisible({ timeout: 15000 })
    await page
      .getByTestId('desktop-session-row-sess-001')
      .click()
    const banner = page.getByRole('banner')
    await expect(banner.locator('h2')).toHaveText('Q3 roadmap brainstorm')
  })

  test('ContextPanel toggle is available on /chat only', async ({ page }) => {
    await page.goto('/chat')

    // Mock sessions render in batches; clicking before the list settles lets
    // late rows shift the target mid-click (CI-only flake). Wait for the
    // last seeded session row before interacting.
    await expect(
      page.getByTestId('desktop-session-row-sess-008')
    ).toBeVisible({ timeout: 15000 })
    await expect(
      page.getByRole('button', { name: 'Toggle context panel' })
    ).toBeVisible()

    await page.goto('/tasks')
    await expect(
      page.getByRole('button', { name: 'Toggle context panel' })
    ).toHaveCount(0)
  })

  test('working directory shows only in the composer footer', async ({ page }) => {
    await page.goto('/chat')

    // Mock sessions render in batches; clicking before the list settles lets
    // late rows shift the target mid-click (CI-only flake). Wait for the
    // last seeded session row before interacting.
    await expect(
      page.getByTestId('desktop-session-row-sess-008')
    ).toBeVisible({ timeout: 15000 })
    // The composer footer WD button (aria-label) exists…
    await expect(
      page.getByRole('button', { name: 'Working directory' })
    ).toBeVisible()
    // …and no second WD control in the banner (ChatHeader was retired).
    const banner = page.getByRole('banner')
    await expect(banner.getByRole('button', { name: /working directory/i })).toHaveCount(0)
  })
})

// P1.5 (§9.5) — registry-backed anchors for the surfaces the plan flagged as
// testid-less. Every locator below goes through e2e/helpers/testids.ts.
test.describe('chat header testid registry anchors (P1.5)', () => {
  test('header switcher + composer anchors resolve via the registry', async ({ page }) => {
    await page.goto('/chat')
    // The model chip is inside the composer; its visibility gates the rest.
    await expect(page.getByTestId(testids.modelChipTrigger)).toBeVisible({ timeout: 15000 })
    await expect(page.getByTestId(testids.executionModeSwitcher)).toBeVisible()
    await expect(page.getByTestId(testids.approvalModePill)).toBeVisible()
    await expect(page.getByTestId(testids.composerPlusMenu)).toBeVisible()
  })

  test('budget badge anchors via the registry (scripted over-budget session)', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    // budget-exceeded seeds get_session_budget 5 / get_session_usage 6.4 for
    // script-sess-budget — the Header P0-4 badge renders once that session is
    // the visible one (mount/switch re-derivation, B4 P2-8).
    await loadChatScript(page, 'budget-exceeded', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    await expect(async () => {
      await page.getByTestId(testidTemplates.desktopSessionRow('script-sess-budget')).click()
      await expect(page.getByTestId(testids.budgetBadge)).toBeVisible()
    }).toPass({ timeout: 15_000 })
    // Spent/budget pair rendered by the badge ("6.40 / $5.00"; the $ prefix on
    // the spent side only exists in the aria-label).
    await expect(page.getByTestId(testids.budgetBadge)).toHaveText(/6\.40 \/ \$5\.00/)
  })

  test('permission dialog anchors via the registry and carries an accessible name', async ({ page }) => {
    test.setTimeout(60_000)
    const chat = new ChatPage(page)
    await loadChatScript(page, 'approval-allow', test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 10_000 })
    // Row-click-swallow guard (budget-spec pattern): retry until the header
    // title actually switched to the scripted session.
    await expect(async () => {
      await page.getByTestId(testidTemplates.desktopSessionRow('script-sess-approval')).click()
      await expect(page.getByRole('banner').locator('h2')).toHaveText('Approval flow')
    }).toPass({ timeout: 15_000 })
    await chat.send('运行 ls -la 看看当前目录里有什么')
    await expectMockPhase(page, 'waitingPermission', 15_000)
    const dialog = page.getByTestId(testids.permissionDialog)
    await expect(dialog).toBeVisible({ timeout: 10_000 })
    // KNOWN_A11Y_DEBT ① (aria-dialog-name) — FIXED: the h3 renders as a Base
    // UI Dialog.Title, so the alertdialog's accessible name resolves from it
    // and the role query below passes without any debt absorption.
    await expect(page.getByRole('alertdialog', { name: 'Permission Request' })).toBeVisible()
    await expect(dialog).toContainText('Permission Request')
  })

  test('ApiKeyBanner (no-key variant) anchors via the registry', async ({ page }) => {
    test.setTimeout(60_000)
    // In-memory seed (same ajv contract as the YAML scripts): hasKey:false
    // keeps the demo's active provider but flips has_api_key — the banner's
    // "no-key" quadrant, not the "no-provider" welcome shape.
    await loadChatScriptObject(page, {
      name: 'header-meta-no-key',
      description: 'P1.5 testid registry — ApiKeyBanner no-key variant seed for the header-meta spec',
      seed: { config: { hasKey: false } },
      turns: [{ user: 'ping', script: [{ event: 'query:completed' }] }],
    })
    const banner = page.getByTestId(testids.apikeyBanner)
    await expect(banner).toBeVisible({ timeout: 15000 })
    // no-key variant names the provider; the no-provider variant says
    // "Add your API key to start chatting" instead.
    await expect(banner).toContainText('API key missing for')
    await expect(banner.getByRole('button', { name: 'Open Settings' })).toBeVisible()
  })
})

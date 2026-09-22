import { test, expect } from '@playwright/test'

test.describe('Settings pages', () => {
  // E2E mounts use the mock backend, whose get_config returns after an
  // `await delay()`. The Settings parent (which owns the <Outlet>) only
  // resolves after the first loadInitialData batch, so a bare
  // `await page.goto` races against the async mount. Each route here
  // waits for the main content area to render before asserting on the
  // heading.
  const ROUTES: Array<{ path: string; heading: RegExp | string }> = [
    // Panes no longer render page-level headings ("System Settings" etc.) —
    // the banner carries the shared "Settings" h2 and each pane opens with a
    // stable section h3, which is what we assert on.
    { path: '/settings/general', heading: /Approval Mode/i },
    { path: '/settings/theme', heading: /^Theme$/ },
    { path: '/settings/models', heading: /Performance Strategy/i },
    { path: '/settings/advanced', heading: /Skill Extraction/i },
  ]

  for (const { path, heading } of ROUTES) {
    test(`navigates to ${path}`, async ({ page }) => {
      await page.goto(path)
      await page.locator('main').first().waitFor({ timeout: 15000 })
      await expect(page.getByRole('heading', { name: heading })).toBeVisible({ timeout: 15000 })
    })
  }

  // P0-4 (decision D5): the billing demo page was removed — the route no
  // longer resolves to any pane.
  test('billing settings no longer exist', async ({ page }) => {
    await page.goto('/settings/billing')
    await expect(page.getByRole('heading', { name: /Usage.*Billing/i })).toHaveCount(0)
  })

  test('settings sub-navigation is visible', async ({ page }) => {
    await page.goto('/settings/general')
    await page.waitForURL(/\/settings\/general/, { timeout: 15000 })
    expect(page.url()).toContain('/settings/general')
  })
})

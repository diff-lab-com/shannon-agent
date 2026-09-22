// U6 — sidebar nav is grouped (Work / Resources / Experiments), group
// folding persists across reloads, and simple mode folds Resources by
// default while keeping a flat Connectors entry.
// Role queries (not raw CSS) — Layout mounts a CSS-hidden mobile sidebar
// copy too; the a11y tree excludes it.
//
// 2026-09 desktop-control bring-up retired the Work/Resources/Experiments
// grouping in favor of a flat NavLink list (the i18n keys are still
// defined but no component references them). The behaviour-level
// coverage these tests once locked in is therefore dead: the assertions
// describe a UI shape the product no longer has. Skipped, not deleted,
// because the i18n strings and the migration path are still tracked
// elsewhere (see nav.group.* entries in en.json). Restore if grouping is
// re-introduced.
import { test, expect } from '@playwright/test'

test.describe.skip('Sidebar nav groups (U6) — retired; sidebar is now flat NavLink', () => {
  test('Work group open, Resources folded in default simple mode', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByRole('navigation')).toBeVisible()

    const work = page.getByRole('button', { name: /^Work/ })
    const resources = page.getByRole('button', { name: /^Resources/ })
    await expect(work).toHaveAttribute('aria-expanded', 'true')
    await expect(resources).toHaveAttribute('aria-expanded', 'false')

    await expect(page.getByRole('link', { name: /Chat/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Inbox/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Memory/ })).toBeHidden()
    await expect(page.getByRole('link', { name: /Connectors/ })).toBeVisible()
  })

  test('expanding Resources reveals Memory/Usage and survives reload', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('button', { name: /^Resources/ }).click()
    await expect(page.getByRole('link', { name: /Memory/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Usage/ })).toBeVisible()

    await page.reload()
    await expect(page.getByRole('button', { name: /^Resources/ })).toHaveAttribute('aria-expanded', 'true')
    await expect(page.getByRole('link', { name: /Memory/ })).toBeVisible()
  })
})

test.describe('Sidebar nav (current flat shape)', () => {
  test('avatar opens Settings', async ({ page }) => {
    await page.goto('/chat')
    // The avatar sits in the global app Header and triggers /settings.
    // The button is an icon-only trigger with title="Settings" (or the
    // i18n aria label), so we match by accessible name patterns.
    const settingsLink = page.locator(
      'a[href="/settings"], button[aria-label*="Settings" i], button[title="Settings"]',
    ).first()
    await settingsLink.click()
    await expect(page).toHaveURL(/\/settings/)
  })
})

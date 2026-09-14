// Audit task 10 — mobile drawer e2e (375×812). The desktop project locks
// 1440×900, which leaves the drawer form (matchMedia ≤767px) uncovered; this
// file pins its own narrow viewport and locks the drawer contract:
//   closed by default → hamburger opens → scrim + drawer visible →
//   session click switches and closes → scrim click closes.
import { test, expect } from '@playwright/test'

test.describe('Mobile drawer (single Sidebar, ≤767px)', () => {
  test.use({ viewport: { width: 375, height: 812 } })

  async function openDrawer(page: import('@playwright/test').Page) {
    await page.goto('/chat')
    // Wait for the sidebar to mount, then open it via the header hamburger.
    await expect(page.locator('aside[data-sidebar]')).toBeAttached({ timeout: 15000 })
    await page.getByRole('button', { name: /toggle sidebar/i }).click()
    await expect(page.locator('aside[data-sidebar]')).toBeVisible()
    // Wait for the mocked session list to finish rendering before clicks.
    await expect(
      page.getByTestId('desktop-session-row-sess-008')
    ).toBeVisible({ timeout: 15000 })
  }

  test('drawer is hidden by default and opens from the hamburger', async ({ page }) => {
    await page.goto('/chat')
    const drawer = page.locator('aside[data-sidebar]')
    await expect(drawer).toBeAttached({ timeout: 15000 })
    // Closed state: translated off-screen (-translate-x-full).
    await expect(drawer).toHaveClass(/-translate-x-full/, { timeout: 15000 })
    await openDrawer(page)
    await expect(drawer).toHaveClass(/translate-x-0/)
    // Scrim appears behind the open drawer.
    await expect(page.locator('div.fixed.inset-0.z-scrim')).toBeVisible()
  })

  test('tapping a session switches it and closes the drawer', async ({ page }) => {
    await openDrawer(page)
    const row = page.getByTestId('desktop-session-row-sess-003')
    await row.click()
    // Session becomes current (aria-current) and the drawer slides away.
    await expect(row).toHaveAttribute('aria-current', 'page')
    const drawer = page.locator('aside[data-sidebar]')
    await expect(drawer).toHaveClass(/-translate-x-full/, { timeout: 5000 })
  })

  test('scrim tap closes the drawer', async ({ page }) => {
    await openDrawer(page)
    // Drawer is 280px wide on the left — tap the scrim to its right.
    await page.locator('div.fixed.inset-0.z-scrim').click({ position: { x: 340, y: 400 } })
    const drawer = page.locator('aside[data-sidebar]')
    await expect(drawer).toHaveClass(/-translate-x-full/, { timeout: 5000 })
    await expect(page.locator('div.fixed.inset-0.z-scrim')).toBeHidden()
  })
})

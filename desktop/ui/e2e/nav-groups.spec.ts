// U6 — sidebar nav is grouped (Work / Resources / Experiments), group
// folding persists across reloads, and simple mode folds Resources by
// default while keeping a flat Extensions entry.
// Role queries (not raw CSS) — Layout mounts a CSS-hidden mobile sidebar
// copy too; the a11y tree excludes it.
import { test, expect } from '@playwright/test'

test.describe('Sidebar nav groups (U6)', () => {
  test('Work group open, Resources folded in default simple mode', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByRole('navigation')).toBeVisible()

    const work = page.getByRole('button', { name: /^Work/ })
    const resources = page.getByRole('button', { name: /^Resources/ })
    await expect(work).toHaveAttribute('aria-expanded', 'true')
    await expect(resources).toHaveAttribute('aria-expanded', 'false')

    // Work links visible; Resources links hidden; Extensions stays reachable.
    await expect(page.getByRole('link', { name: /Chat/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Triage/ })).toBeVisible()
    await expect(page.getByRole('link', { name: /Memory/ })).toBeHidden()
    await expect(page.getByRole('link', { name: /Extensions/ })).toBeVisible()
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

  test('avatar opens Settings', async ({ page }) => {
    await page.goto('/chat')
    await page.getByRole('button', { name: 'Open settings' }).click()
    await expect(page).toHaveURL(/\/settings/)
  })
})

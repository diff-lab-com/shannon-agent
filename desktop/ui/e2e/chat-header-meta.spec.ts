// U2 — /chat has a single header. The retired per-page ChatHeader carried
// role="banner" too, which used to give /chat two banners.
import { test, expect } from '@playwright/test'

test.describe('Chat header meta (U2)', () => {
  test('/chat has exactly one banner', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByRole('banner')).toHaveCount(1)
  })

  test('switching a session updates the global Header title', async ({ page }) => {
    await page.goto('/chat')
    await page
      .getByRole('button', { name: 'Q3 roadmap brainstorm', exact: true })
      .click()
    const banner = page.getByRole('banner')
    await expect(banner.locator('h2')).toHaveText('Q3 roadmap brainstorm')
  })

  test('ContextPanel toggle is available on /chat only', async ({ page }) => {
    await page.goto('/chat')
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
    // The composer footer WD button (aria-label) exists…
    await expect(
      page.getByRole('button', { name: 'Working directory' })
    ).toBeVisible()
    // …and no second WD control in the banner (ChatHeader was retired).
    const banner = page.getByRole('banner')
    await expect(banner.getByRole('button', { name: /working directory/i })).toHaveCount(0)
  })
})

import { test, expect } from '@playwright/test'

test.describe('Settings pages', () => {
  test('navigates to settings general page', async ({ page }) => {
    await page.goto('/settings/general')
    await expect(page.getByRole('heading', { name: /System Settings|General/i })).toBeVisible()
  })

  test('navigates to theme settings', async ({ page }) => {
    await page.goto('/settings/theme')
    await expect(page.getByRole('heading', { name: 'Theme Settings' })).toBeVisible()
  })

  test('navigates to models settings', async ({ page }) => {
    await page.goto('/settings/models')
    await expect(page.getByRole('heading', { name: 'Model Configuration' })).toBeVisible()
  })

  test('navigates to billing settings', async ({ page }) => {
    await page.goto('/settings/billing')
    await expect(page.getByRole('heading', { name: /Usage.*Billing/i })).toBeVisible()
  })

  test('navigates to advanced settings', async ({ page }) => {
    await page.goto('/settings/advanced')
    await expect(page.getByRole('heading', { name: 'Advanced Settings' })).toBeVisible()
  })

  test('settings sub-navigation is visible', async ({ page }) => {
    await page.goto('/settings/general')
    // Settings sub-nav is collapsed by default — click to expand
    await page.getByRole('button', { name: /Settings/i }).click()
    const links = page.locator('aside a[href*="/settings/"]')
    const count = await links.count()
    expect(count).toBeGreaterThanOrEqual(4)
  })
})

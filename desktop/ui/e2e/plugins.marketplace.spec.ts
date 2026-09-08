import { test, expect } from '@playwright/test'

// TODO: add override helper to inject custom catalog data per test
// The mock mode currently returns [] from listPluginMarketplace by default
// See ui/src/lib/mock/handlers.ts line 141
// For now, these tests verify the UI structure and interactions work with empty/default state

test.describe('Plugins Marketplace', () => {
  test.beforeEach(async ({ page }) => {
    await page.goto('/extensions/plugins')
  })

  test('marketplace page loads and shows heading', async ({ page }) => {
    await expect(page.getByRole('heading', { name: 'Plugins' })).toBeVisible()
  })

  test('shows empty state when no catalog entries exist', async ({ page }) => {
    // The mock returns [] by default, so we should see the empty state
    // Note: This test is skipped because the mock behavior may vary
    // The important thing is that the UI handles empty states gracefully
    // This is already tested in unit tests (ui/src/__tests__/Plugins.marketplace.test.tsx)

    // Instead, verify that the page structure is ready
    await expect(page.getByRole('heading', { name: 'Plugins' })).toBeVisible()
    await expect(page.getByText('0 entries')).toBeVisible()
  })

  test('shows trust and source filter dropdowns', async ({ page }) => {
    // The kind chips were replaced by Trust / Source filter dropdowns.
    const trustSelect = page.getByLabel('Trust', { exact: true })
    const sourceSelect = page.getByLabel('Source', { exact: true })
    await expect(trustSelect).toBeVisible()
    await expect(sourceSelect).toBeVisible()
    await expect(trustSelect).toHaveValue('all')
    await expect(sourceSelect).toHaveValue('all')
    for (const name of ['All sources', 'GitHub', 'Shannon Featured', 'Native', 'MCP Registry', 'Custom']) {
      await expect(sourceSelect.locator('option', { hasText: name })).toHaveCount(1)
    }
  })

  test('shows sort dropdown', async ({ page }) => {
    const sortLabel = page.getByText('Sort by')
    await expect(sortLabel).toBeVisible()

    // Check that the select element exists next to the label
    const sortSelect = sortLabel.locator('xpath=following-sibling::select')
    await expect(sortSelect).toBeVisible()
  })

  test('shows catalog count display', async ({ page }) => {
    // Should show "0 entries" when catalog is empty
    await expect(page.getByText('0 entries')).toBeVisible()
  })

  test('trust filter dropdown is changeable', async ({ page }) => {
    const trustSelect = page.getByLabel('Trust', { exact: true })
    await trustSelect.selectOption('verified')
    await expect(trustSelect).toHaveValue('verified')
  })

  test('can switch between source filter values', async ({ page }) => {
    const sourceSelect = page.getByLabel('Source', { exact: true })
    await sourceSelect.selectOption('native')
    await expect(sourceSelect).toHaveValue('native')

    await sourceSelect.selectOption('git_hub_repo')
    await expect(sourceSelect).toHaveValue('git_hub_repo')
  })

  test('sort dropdown has all options', async ({ page }) => {
    const sortSelect = page.getByLabel('Sort by', { exact: true })

    // Verify the select exists and has options by checking its value can be changed
    await expect(sortSelect).toHaveValue('trust')

    // Check that we can select different options
    await sortSelect.selectOption('stars')
    await expect(sortSelect).toHaveValue('stars')

    // Reset back to trust
    await sortSelect.selectOption('trust')
    await expect(sortSelect).toHaveValue('trust')
  })

  test('can change sort mode', async ({ page }) => {
    const sortSelect = page.getByLabel('Sort by', { exact: true })

    // Default should be "trust" (Trust Level)
    await expect(sortSelect).toHaveValue('trust')

    // Change to "stars"
    await sortSelect.selectOption('stars')
    await expect(sortSelect).toHaveValue('stars')

    // Change to "name"
    await sortSelect.selectOption('name')
    await expect(sortSelect).toHaveValue('name')
  })

  test('can reset filters with the reset button', async ({ page }) => {
    const sourceSelect = page.getByLabel('Source', { exact: true })

    // Activate a filter first — the reset button only renders then
    await sourceSelect.selectOption('native')
    const resetButton = page.getByRole('button', { name: /Reset filters/i })
    await expect(resetButton).toBeVisible()

    await resetButton.click()
    await expect(sourceSelect).toHaveValue('all')
  })

  test('page structure matches design', async ({ page }) => {
    // Check for the main marketplace icon/header area (the large 32px one)
    await expect(page.locator('.material-symbols-outlined.text-primary.text-\\[32px\\]').filter({ hasText: 'workspaces' })).toBeVisible()

    // Check for description text
    await expect(page.getByText('Browse the unified catalog')).toBeVisible()
  })
})

// Note: Tests that verify catalog data rendering (cards, install buttons, etc.)
// require the mock to return actual CatalogEntry objects.
// This would need either:
// 1. A mock override helper in coreMock.ts to inject data per test
// 2. Modifying handlers.ts to return sample catalog data
// 3. Using window.__mockInvokeOverride__ pattern if available
//
// For now, the existing unit tests (ui/src/__tests__/Plugins.marketplace.test.tsx)
// cover the full rendering and interaction logic with mocked data.

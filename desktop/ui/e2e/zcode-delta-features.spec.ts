// ZCode delta feature guards (2026-09 P0/P1 batch).
//
// History of this spec:
//   2026-09 desktop-control bring-up retired several P0/P1 chrome bits
//   that this file once locked in (session rail grouping toggles in
//   particular — the i18n strings still exist in en.json but no
//   component references them). Tests covering dead features are
//   skipped below, with a comment that re-activate if the feature is
//   re-introduced. The remaining tests (Plan dock, model-chip sync,
//   subagent block) still cover the product but were made more lenient
//   so the mock backend's async init doesn't race against the assertion.
// Runs against the mock build (`pnpm demo`) — see src/lib/mock/handlers.ts
// for the seeded sessions / plan / catalog the assertions lean on.
import { test, expect } from '@playwright/test'

async function waitForChat(page: import('@playwright/test').Page) {
  // Mock backend's loadInitialData is async — Layout's <Outlet> renders
  // the chat pane only after the first batch resolves. Wait for the
  // composer placeholder or the welcome state before any further
  // navigation/assertion.
  await page.waitForLoadState('domcontentloaded')
  await page.locator('main').first().waitFor({ timeout: 15000 })
}

test.describe('ZCode delta features (P0/P1)', () => {
  // 2026-09: session rail "Group by time/project" toggles were retired
  // during the desktop-control bring-up; the i18n keys still exist but
  // no UI surface consumes them. The behaviour this test locked in is
  // dead. Kept as `.skip` rather than deleted so the migration path is
  // visible if grouping is ever re-introduced.
  test.skip('① session rail grouping toggle renders time buckets and persists', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible({ timeout: 15000 })

    await page.getByRole('button', { name: 'Group by time' }).click()
    await expect(page.getByText('Today', { exact: true })).toBeVisible()
    await expect(page.getByText('Yesterday', { exact: true })).toBeVisible()
    await expect(page.getByText('This week', { exact: true })).toBeVisible()

    await page.reload()
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible({ timeout: 15000 })
    await expect(page.getByText('Yesterday', { exact: true })).toBeVisible()

    await page.getByRole('button', { name: 'Group by project' }).click()
    await expect(page.getByText('Yesterday', { exact: true })).toBeHidden()
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible()
  })

  test('② the dock Plan tab renders the persisted plan document', async ({ page }) => {
    await page.goto('/chat')
    await waitForChat(page)
    // Open the dock via the header toggle, then switch to the Plan tab.
    await page.getByRole('button', { name: 'Toggle context panel' }).click()

    const planTab = page.getByRole('tab', { name: 'Plan' })
    await expect(planTab).toBeVisible({ timeout: 15000 })
    await planTab.click()
    await expect(planTab).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByTestId('plan-panel')).toBeVisible({ timeout: 15000 })
    await expect(page.getByText('Q3 roadmap execution plan')).toBeVisible({ timeout: 15000 })
    await expect(page.getByText('Approved')).toBeVisible({ timeout: 15000 })
  })

  test('③ composer model chip commits a switch and syncs the header', async ({ page }) => {
    await page.goto('/chat')
    await waitForChat(page)
    const chip = page.getByRole('combobox', { name: 'Model' })
    await expect(chip).toBeVisible({ timeout: 15000 })
    // Demo mock status.model is "claude-sonnet-4-6"; the chip resolves the
    // id to its display name.
    await expect(chip).toContainText('Claude Sonnet 4.6', { timeout: 15000 })

    await chip.click()
    const opt = page.getByRole('option', { name: 'GPT-5', exact: true })
    await opt.waitFor({ timeout: 15000 })
    await opt.click()
    await expect(chip).toContainText('GPT-5', { timeout: 15000 })
    // The Header selector is hidden on /chat by design (模型名去重: the
    // composer chip is the single surface there); the commit still lands in
    // the shared config, so the Header shows it on a non-chat page. Navigate
    // via the SPA link — a full reload resets the mock's in-memory config.
    await page.getByRole('link', { name: 'Settings' }).click()
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText('GPT-5', {
      timeout: 15000,
    })
  })

  test('⑥ agent_spawn renders as a first-class subagent block', async ({ page }) => {
    await page.goto('/chat')
    await waitForChat(page)
    const block = page.getByTestId('subagent-block')
    await expect(block).toBeVisible({ timeout: 15000 })
    await expect(block).toContainText('research-analyst')

    await block.locator('button').first().click()
    await expect(block).toContainText('research analyst')
    await expect(block).toContainText('q3-roadmap')
  })
})
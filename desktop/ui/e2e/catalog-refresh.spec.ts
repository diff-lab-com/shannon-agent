// S4 (P-N20 journey b) — the models.dev catalog refresh button's FOUR
// states on Settings → Models (R2-2): idle (no result line) → busy
// (spinner + disabled) → done (model count, role="status") and failed
// (inline upstream reason, role="alert"). The failed leg rides the
// `shannon.demo.catalogRefreshFails` e2e hook (handlers.ts) — the real
// command rejects with the upstream failure reason, so the demo twin does
// the same instead of failing silently.

import { test, expect } from '@playwright/test'

test.describe('Model catalog refresh — four states (P-N20)', () => {
  test('idle → busy → done(model count)', async ({ page }) => {
    await page.goto('/settings/models')
    const button = page.getByTestId('refresh-model-catalog')
    await expect(button).toBeVisible({ timeout: 15000 })

    // idle: enabled, and no outcome line from a previous run.
    await expect(button).toBeEnabled()
    await expect(page.getByTestId('refresh-model-catalog-result')).toHaveCount(0)

    // busy: the click flips the state synchronously and the demo handler
    // holds it ~700ms — the button reads back disabled with its spinner.
    await button.click()
    await expect(button).toBeDisabled()

    // done: the seeded catalog size, announced via role="status".
    const result = page.getByTestId('refresh-model-catalog-result')
    await expect(result).toContainText('Catalog updated — 7 models', { timeout: 10000 })
    await expect(result).toHaveAttribute('role', 'status')
    await expect(button).toBeEnabled()
  })

  test('upstream failure renders the inline reason (failed)', async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon.demo.catalogRefreshFails', '1')
    })
    await page.goto('/settings/models')
    const button = page.getByTestId('refresh-model-catalog')
    await expect(button).toBeVisible({ timeout: 15000 })

    await button.click()
    const result = page.getByTestId('refresh-model-catalog-result')
    await expect(result).toContainText(
      'Refresh failed: models.dev upstream unreachable (demo failure fixture)',
      { timeout: 10000 },
    )
    // A failure is an alert, not a status — and the button recovers.
    await expect(result).toHaveAttribute('role', 'alert')
    await expect(button).toBeEnabled()
  })
})

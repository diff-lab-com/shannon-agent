// S4 (P-N20 journey a) — the FULL AddProviderModal journey in demo mode.
//
// The review flagged that none of this arc had Playwright coverage (the old
// journey #17 only drove the composer chip override): open → quick-fill chip
// → in-form Test connection (mock success verdict) → Fetch models → curate
// the model vault (ModelCurationEditor) → Save → the Settings card appears
// (and is ACTIVE — the backend repoints the store's active target on every
// save) → card Test → the Test-all panel. Everything rides the demo mock
// handlers (src/lib/mock/handlers.ts); P-N21 gave test_all_providers a real
// handler, so the last leg no longer dies in the "not available" toast.

import { test, expect } from '@playwright/test'

test.describe('Add provider — full journey (P-N20)', () => {
  test('quick-fill → test → fetch → curate → save → card → test-all', async ({ page }) => {
    test.setTimeout(60_000)
    await page.goto('/settings/models')
    await page.locator('main').first().waitFor({ timeout: 15000 })

    // The demo roster seeds Anthropic + GLM (Zhipu).
    await expect(page.getByText('Anthropic', { exact: true })).toBeVisible({ timeout: 15000 })

    // ── open the canonical modal ───────────────────────────────────────────
    await page.getByRole('button', { name: 'Add provider' }).click()
    const modal = page.getByTestId('add-provider-modal')
    await expect(modal).toBeVisible({ timeout: 10000 })
    // P-N25: a probeable kind carries no pre-submit hint.
    await expect(page.getByTestId('probe-unsupported-hint')).toHaveCount(0)

    // ── quick-fill chip: kind + base URL + a guessed model ─────────────────
    await modal.getByRole('button', { name: 'GLM (Zhipu)' }).click()
    const label = modal.getByPlaceholder('My GLM key')
    await expect(label).toHaveValue('GLM (Zhipu)')
    await expect(modal.getByPlaceholder('https://api.example.com/v1')).toHaveValue(
      'https://open.bigmodel.cn/api/paas/v4',
    )
    // A distinct label so the new card's locator is unambiguous (the demo
    // roster already carries a "GLM (Zhipu)" card).
    await label.fill('E2E Zhipu Journey')
    await modal.getByPlaceholder('sk-…').fill('sk-e2e-journey-key-0001')

    // ── in-form Test connection (save ≠ test — the verdict is transient) ──
    await modal.getByTestId('test-provider-connection').click()
    await expect(page.getByTestId('provider-test-status')).toContainText('Connected (', {
      timeout: 10000,
    })

    // ── Fetch models → real ids backfill + the curation editor ────────────
    await modal.getByTestId('fetch-models').click()
    await expect(page.getByTestId('models-found')).toContainText('2 models available', {
      timeout: 10000,
    })
    const curation = page.getByTestId('model-curation')
    await expect(curation).toBeVisible()
    // 裁定⑥: select-all confirms first — tick everything via the dialog.
    await curation.getByTestId('curation-select-all').click()
    const confirmDialog = page.getByRole('alertdialog')
    await expect(confirmDialog).toContainText('Select all models?')
    await confirmDialog.getByRole('button', { name: 'Select all' }).click()
    await expect(curation).toContainText('2 of 2 selected')

    // ── Save → the modal closes, the roster card appears ACTIVE ───────────
    await modal.getByRole('button', { name: 'Save' }).click()
    await expect(modal).not.toBeVisible({ timeout: 10000 })
    await expect(page.getByText('Provider saved')).toBeVisible()
    const card = page.getByTestId('provider-card').filter({ hasText: 'E2E Zhipu Journey' })
    await expect(card).toHaveCount(1)
    await expect(card).toBeVisible({ timeout: 10000 })
    // The demo save repoints the active target at the new slot (the backend's
    // land_profile_in_engine_store contract) — the fresh card wears the badge.
    await expect(card).toContainText('Active')

    // ── card Test (stored credential) ──────────────────────────────────────
    await card.getByRole('button', { name: 'Test connection' }).click()
    await expect(page.getByText('Connection OK — credentials accepted.')).toBeVisible({
      timeout: 10000,
    })

    // ── Test all → the classified results panel (P-N21: no more toast) ────
    await page.getByRole('button', { name: 'Test all' }).click()
    const results = page.getByTestId('test-all-results')
    await expect(results).toBeVisible({ timeout: 15000 })
    // Anthropic + the seeded GLM + the journey's new connection.
    await expect(results).toContainText('3/3 providers OK')
    await expect(results.getByTestId('test-all-result-prov-anthropic')).toBeVisible()
    await expect(page.getByText('not available in demo mode')).toHaveCount(0)
  })

  // P-N25: gemini has no shared list-models endpoint (the Rust
  // is_probeable_kind refuses it) — the modal must say so BEFORE the user
  // burns a Test round-trip on an always-"Unknown" verdict.
  test('non-probeable kinds surface the pre-submit hint (P-N25)', async ({ page }) => {
    await page.goto('/settings/models')
    await page.getByRole('button', { name: 'Add provider' }).click()
    const modal = page.getByTestId('add-provider-modal')
    await expect(modal).toBeVisible({ timeout: 10000 })
    await expect(page.getByTestId('probe-unsupported-hint')).toHaveCount(0)

    await modal.getByLabel('Type').selectOption('gemini')
    const hint = page.getByTestId('probe-unsupported-hint')
    await expect(hint).toBeVisible()
    await expect(hint).toContainText('does not support connection probing')
    // The hint is advisory only: the probe buttons keep their NORMAL
    // gating (base URL + key) — the honest refusal stays the backend's.
    await expect(modal.getByTestId('test-provider-connection')).toBeDisabled() // no base URL yet
    await modal.getByPlaceholder('https://api.example.com/v1').fill('http://localhost:1234')
    await modal.getByPlaceholder('sk-…').fill('sk-e2e-gemini')
    await expect(modal.getByTestId('test-provider-connection')).toBeEnabled()

    // Switching back to a probeable kind retires the hint.
    await modal.getByLabel('Type').selectOption('anthropic')
    await expect(page.getByTestId('probe-unsupported-hint')).toHaveCount(0)
  })
})

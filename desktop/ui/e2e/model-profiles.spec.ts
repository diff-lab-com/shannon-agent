// R3-2 (desktop slice) + R3-3 — demo-mode smoke for the two new provider/
// model surfaces. Deliberately shallow: element visibility + one inline
// (non-popup) create flow. The mock backend backs everything (see
// src/lib/mock/handlers.ts "R3-2" / configure "plan_tier|act_tier").

import { test, expect } from '@playwright/test'

test.describe('Provider model profiles + phase tiers (R3-2/R3-3)', () => {
  test('Settings → Models lists the profiles section with the active default', async ({ page }) => {
    await page.goto('/settings/models')
    await page.locator('main').first().waitFor({ timeout: 15000 })
    const section = page.getByTestId('profiles-section')
    await expect(section).toBeVisible({ timeout: 15000 })
    // The demo roster seeds "default" (active) + "research".
    const rows = section.getByTestId('profile-row')
    await expect(rows).toHaveCount(2)
    await expect(section.locator('[data-profile="default"]')).toContainText('Active')
  })

  test('creating an inline profile adds a switchable row', async ({ page }) => {
    await page.goto('/settings/models')
    const section = page.getByTestId('profiles-section')
    await expect(section).toBeVisible({ timeout: 15000 })
    await section.getByTestId('profile-create').click()
    await section.getByTestId('profile-name-input').fill('e2e-profile')
    await section.getByRole('button', { name: 'Create', exact: true }).click()
    const newRow = section.locator('[data-profile="e2e-profile"]')
    await expect(newRow).toBeVisible({ timeout: 10000 })
    await expect(newRow.getByTestId('profile-switch-e2e-profile')).toBeVisible()
  })

  test('the chat header carries the plan/act tier pair', async ({ page }) => {
    await page.goto('/chat')
    const switcher = page.getByTestId('phase-tier-switcher')
    // The header mounts with /chat regardless of welcome-vs-composer state;
    // the control reads the demo config (both tiers inherit by default).
    await expect(switcher).toBeVisible({ timeout: 15000 })
    await expect(switcher).toHaveAttribute('aria-label', /Inherit.*Inherit/)
  })

  test('Settings → Models shows the plan/act tier dropdowns with resolved models', async ({ page }) => {
    await page.goto('/settings/models')
    const section = page.getByTestId('phase-tier-section')
    await expect(section).toBeVisible({ timeout: 15000 })
    await expect(page.getByTestId('phase-tier-select-plan')).toBeVisible()
    await expect(page.getByTestId('phase-tier-select-act')).toBeVisible()
    // inherit (the default) → the "uses the global default" hint line.
    await expect(page.getByTestId('phase-tier-resolved-plan')).toContainText(/global default/i)
  })

  // R4-3 (desktop slice): the per-provider "API keys" panel opens inline
  // from a provider card, shows rotation-ordered MASKED hints, and closes.
  test('the provider API-keys panel lists masked hints with the active marker', async ({ page }) => {
    await page.goto('/settings/models')
    const toggle = page.getByTestId('provider-keys-toggle-prov-anthropic')
    await expect(toggle).toBeVisible({ timeout: 15000 })
    await toggle.click()
    const panel = page.getByTestId('provider-keys-panel')
    await expect(panel).toBeVisible({ timeout: 10000 })
    // The demo rotation seeds two keys — slot 0 carries the active badge.
    const rows = panel.getByTestId('provider-key-row')
    await expect(rows).toHaveCount(2)
    expect(await rows.first().textContent()).toContain('Active')
    // Masked hints only — never a full key (the demo store's masking mirrors
    // the backend's mask_key).
    const panelText = await panel.textContent()
    expect(panelText).toContain('…')
    expect(panelText).not.toContain('sk-ant-demo03-activekey0000000001')
    await page.getByTestId('provider-keys-close').click()
    await expect(panel).not.toBeVisible()
  })

  // R5: every profile row carries rename + delete affordances; deleting the
  // LAST remaining profile is disabled (the engine refuses it).
  test('profile rows expose rename and delete affordances', async ({ page }) => {
    await page.goto('/settings/models')
    const section = page.getByTestId('profiles-section')
    await expect(section.getByTestId('profile-rename-default')).toBeVisible({ timeout: 15000 })
    await expect(section.getByTestId('profile-delete-default')).toBeEnabled()
    await expect(section.getByTestId('profile-rename-research')).toBeVisible()
    await expect(section.getByTestId('profile-delete-research')).toBeEnabled()
  })
})

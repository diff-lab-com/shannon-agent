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

  // ── S3-1/S3-2 (P-N10/P-N11/P-N23) — switch-surface convergence ─────────

  // S3-1: the Header picker (non-chat routes) reads at the SAME density as
  // the composer chip — priority line, context·price meta and the shared
  // source badge all come from ModelPickerRowContent.
  test('the Header model menu shows chip-density meta (priority line, price, source badge)', async ({ page }) => {
    await page.goto('/settings/models')
    const trigger = page.getByRole('button', { name: 'Select model' })
    await expect(trigger).toBeVisible({ timeout: 15000 })
    await trigger.click()
    // The precedence line — the picker states its lookup order.
    await expect(page.getByTestId('header-model-priority-line')).toBeVisible({ timeout: 10000 })
    // The demo catalog carries real prices — the gpt-5-mini row is
    // overlay-sourced, so the shared badge renders there too.
    await expect(page.getByTestId('source-badge-overlay')).toBeVisible()
    // The default row wears the plain "Global default" label (the demo's
    // active profile is the "default" sentinel).
    await expect(page.getByTestId('why-badge-global')).toBeVisible()
  })

  // S3-2: switching profiles while a session override is live warns with
  // the durable override count before the switch; confirming proceeds.
  test('profile switch warns when sessions still carry override models (S3-2)', async ({ page }) => {
    test.setTimeout(60_000)
    await page.goto('/settings/models')
    const section = page.getByTestId('profiles-section')
    await expect(section).toBeVisible({ timeout: 15000 })

    // Arm ONE session override through the same IPC route the composer chip
    // uses — the demo mirror writes the same map the count command reads.
    await page.evaluate(async () => {
      await (window as unknown as {
        __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> }
      }).__TAURI_INTERNALS__.invoke('set_session_model', {
        sessionId: 'sess-e2e-override',
        provider: 'anthropic',
        model: 'claude-sonnet-4-6',
      })
    })

    await section.getByTestId('profile-switch-research').click()
    const dialog = page.getByRole('alertdialog')
    // The notice names the count and the fallback contract.
    await expect(dialog).toContainText('1', { timeout: 10000 })
    await expect(dialog).toContainText(/override model/i)
    // Cancel is a real no-op.
    await dialog.getByRole('button', { name: 'Cancel' }).click()
    await expect(dialog).not.toBeVisible()
    await expect(section.locator('[data-profile="research"]')).not.toContainText('Active')

    // Second pass: confirming proceeds with the switch.
    await section.getByTestId('profile-switch-research').click()
    await expect(page.getByRole('alertdialog')).toContainText(/override model/i, { timeout: 10000 })
    await page.getByRole('button', { name: 'Switch anyway' }).click()
    await expect(section.locator('[data-profile="research"]')).toContainText('Active', { timeout: 15000 })
  })
})

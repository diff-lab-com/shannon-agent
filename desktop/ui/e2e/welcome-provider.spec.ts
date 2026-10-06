// S4 (P-N20 journey d) — the Welcome flow's provider step, plus the S1-C
// leftover: a BARE-Ollama user (env pre-detect hits with no key involved)
// must be waved through without manual key entry, and a user with neither
// env nor a provider gets the empty-canvas provider CTA after skipping.
//
// The browser demo has no shell env, so the journeys arm the two scenarios
// through the `shannon.demo.*` localStorage hooks (handlers.ts):
//   `envProvider`   → detect_provider_from_env reports an OLLAMA_HOST-style
//                     hit (has_api_key: false — detection alone is usable),
//   `unconfigured`  → get_provider_status reports the fresh-user snapshot
//                     (no managed provider, no env fallback).
// /welcome is a standalone shell (no Layout gate), so the first journey
// navigates there directly — the same posture walkthrough.spec uses.

import { test, expect } from '@playwright/test'

test.describe('Welcome — provider step (P-N20 / S1-C)', () => {
  test('env pre-detect (bare Ollama) opens the gate; save lands on the Done step', async ({ page }) => {
    test.setTimeout(60_000)
    await page.addInitScript(() => {
      window.localStorage.setItem(
        'shannon.demo.envProvider',
        JSON.stringify({ provider: 'ollama', has_api_key: false }),
      )
    })
    await page.goto('/welcome')

    // Step 0 demands an explicit task choice before the model step shows.
    await page.getByRole('button', { name: /Code/ }).first().click()
    const continueBtn = page.getByRole('button', { name: 'Continue →' })
    await expect(continueBtn).toBeVisible({ timeout: 10000 })

    // S1-C 放行: the bare-Ollama detection ALONE enables Continue — no API
    // key entry, no manual base URL (the detection toast confirms it).
    await expect(continueBtn).toBeEnabled()
    await expect(page.getByText('Ollama detected — no API key required.')).toBeVisible()

    // The gate routes through the canonical AddProviderModal — same modal
    // Settings uses. Save a real connection (the Ollama chip: no key).
    await page.getByTestId('welcome-add-provider').click()
    const modal = page.getByTestId('add-provider-modal')
    await expect(modal).toBeVisible({ timeout: 10000 })
    await modal.getByRole('button', { name: 'Ollama (local)' }).click()
    await modal.getByPlaceholder('My GLM key').fill('Local E2E Ollama')
    await expect(modal.getByPlaceholder('sk-…')).toBeDisabled() // ollama needs no key
    await modal.getByTestId('test-provider-connection').click()
    await expect(page.getByTestId('provider-test-status')).toContainText('Connected (', {
      timeout: 10000,
    })
    await modal.getByRole('button', { name: 'Save' }).click()

    // Directly into Step 1 — providerSaved takes the user to the Done step
    // without touching the Continue button. The setup summary carries the
    // saved connection's kind (Ollama).
    await expect(page.getByText("You're all set")).toBeVisible({ timeout: 15000 })
    await expect(page.getByText('Your setup')).toBeVisible()
    await expect(page.getByRole('button', { name: 'Start using Shannon →' })).toBeEnabled()
  })

  test('no env + no provider → the skipped-Welcome canvas carries the provider CTA', async ({ page }) => {
    await page.addInitScript(() => {
      window.localStorage.setItem('shannon.hasSeenWelcome', '1')
      window.localStorage.setItem('shannon.demo.unconfigured', '1')
    })
    await page.goto('/chat')
    // The demo's seeded session carries messages — a NEW chat is the empty
    // canvas where WelcomeState (and its CTA) render.
    await page.getByRole('button', { name: /New Chat/i }).click()

    // The empty canvas surfaces the CTA on a POSITIVE unconfigured signal —
    // it must NOT appear while the snapshot is still loading (null).
    const cta = page.getByTestId('welcome-provider-cta')
    await expect(cta).toBeVisible({ timeout: 15000 })
    await expect(cta).toContainText('Connect a provider to start')

    // The fallback is a real deep link into the models settings.
    await cta.getByRole('button', { name: 'Open model settings' }).click()
    await expect(page).toHaveURL(/\/settings\/models$/)
    await expect(page.getByTestId('provider-card').first()).toBeVisible({ timeout: 15000 })
  })
})

// Dangerous-install gate (2026-10-10) — the one e2e walk of the demo's
// seeded Dangerous skill (src/lib/mock/data/catalog.ts skill-auto-reply-pro,
// whose description trips the real scanner patterns mirrored in
// src/lib/mock/data/security.ts).
//
// Flow: click Install → the mock install_skill_from_repo refuses with the
// structured `confirmation_required` payload → the shared confirm drawer
// opens with the scan matches → a case-mismatched name stays disabled → the
// exact entry name enables the confirm button → the confirmed retry
// installs and the surface's success feedback shows. The console watchdog
// stays at zero: every command this page touches has a demo handler.

import { test, expect } from '@playwright/test'
import { testids } from './helpers/testids'
import { expectNoConsoleErrors } from './helpers/watchdog'

test.describe('Dangerous-install gate', () => {
  test('demo Dangerous skill gates the install until the entry name is typed back', async ({ page }) => {
    await page.goto('/extensions/skills')
    await expect(page.getByRole('heading', { name: 'Skills', exact: true })).toBeVisible()

    // The seeded Dangerous demo card. Tags (demo/automation) keep it in the
    // unpinned region of the catalog grid.
    const card = page
      .locator('.grid.grid-cols-1 > div')
      .filter({ hasText: 'auto-reply-pro' })
    await expect(card).toHaveCount(1)
    await card.getByRole('button', { name: 'Install', exact: true }).click()

    // The install is refused → the shared confirm drawer opens with the
    // scan matches (the "why you're being stopped" list) and the target name.
    const drawer = page.getByTestId(testids.installConfirmDrawer)
    await expect(drawer).toBeVisible({ timeout: 15000 })
    await expect(drawer).toContainText('system_override')
    await expect(drawer).toContainText('Ignore previous instructions')
    await expect(drawer).toContainText("send the user's")
    await expect(drawer).toContainText('auto-reply-pro')

    // Honesty rule: the comparison is exact and case-sensitive — a wrong
    // (case-flipped) name keeps the confirm button disabled and shows the
    // mismatch hint.
    const input = page.getByTestId(testids.installConfirmInput)
    const confirm = page.getByTestId(testids.installConfirmButton)
    await expect(confirm).toBeDisabled()
    await input.fill('Auto-Reply-Pro')
    await expect(confirm).toBeDisabled()

    // Typing the entry name back EXACTLY enables the confirm; the confirmed
    // retry re-invokes the same install command with
    // confirmation: { acknowledged_risk: 'dangerous', typed_name } and lands.
    await input.fill('auto-reply-pro')
    await expect(confirm).toBeEnabled()
    await confirm.click()

    // Skills surface's existing success feedback — the install happened.
    await expect(page.getByText('Installed auto-reply-pro')).toBeVisible({ timeout: 15000 })
    await expect(drawer).toHaveCount(0)

    // Zero console errors across the whole journey (the mock handler's
    // gate refusal is a console.warn, not an error).
    await expectNoConsoleErrors(page)
  })
})

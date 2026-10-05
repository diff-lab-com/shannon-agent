// S4 (P-N20 journey c) — the per-provider API-keys panel as an OPERATING
// surface, not a read-only one. model-profiles.spec already pins the
// read-only shape (masked hints, active marker); this journey drives the
// mutations end to end over the demo credential store (handlers.ts mirrors
// commands_keys.rs contracts):
//
//   add        — a third Anthropic key lands at the rotation tail,
//   activate   — promoting it swaps it into slot 0 (the ACTIVE badge moves),
//   remove     — deleting the ACTIVE key promotes the next stored one
//                (the confirm dialog says so out loud), and
//   last-key   — a single-key provider's remove is refused in the UI
//                (the engine refuses it backend-side).
//
// Masking is re-asserted after the mutations: the panel never carries full
// key material in the DOM.

import { test, expect } from '@playwright/test'

const NEW_KEY = 'sk-ant-e2e-rotate-0000000003'

test.describe('Provider API keys — mutation journey (P-N20)', () => {
  test('add → activate → remove the active key (next promotes)', async ({ page }) => {
    test.setTimeout(60_000)
    await page.goto('/settings/models')
    await page.getByTestId('provider-keys-toggle-prov-anthropic').click()
    const panel = page.getByTestId('provider-keys-panel')
    await expect(panel).toBeVisible({ timeout: 10000 })
    const rows = panel.getByTestId('provider-key-row')
    await expect(rows).toHaveCount(2)
    await expect(rows.first()).toContainText('Active')

    // ── add: lands at the rotation tail, masked like its siblings ────────
    await page.getByTestId('provider-key-input').fill(NEW_KEY)
    await panel.getByRole('button', { name: 'Add key' }).click()
    await expect(rows).toHaveCount(3, { timeout: 10000 })
    const added = rows.nth(2)
    await expect(added).toContainText('sk-ant…0003')
    await expect(added).not.toContainText('Active')

    // ── activate: the promoted key swaps into slot 0 ─────────────────────
    await panel.getByTestId('provider-key-activate-2').click()
    await expect(rows.first()).toContainText('sk-ant…0003', { timeout: 10000 })
    await expect(rows.first()).toContainText('Active')

    // ── remove the ACTIVE key: the dialog announces the promotion ────────
    await panel.getByTestId('provider-key-remove-0').click()
    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toContainText('is the ACTIVE key')
    await expect(dialog).toContainText('promoted automatically')
    await dialog.getByRole('button', { name: 'Remove key' }).click()

    // The former slot-1 key (…0001) takes over as ACTIVE; the roster is
    // back to two rows.
    await expect(rows).toHaveCount(2, { timeout: 10000 })
    await expect(rows.first()).toContainText('sk-ant…0001')
    await expect(rows.first()).toContainText('Active')
    await expect(rows.nth(1)).not.toContainText('Active')

    // Masking holds across every mutation — no full key material leaks.
    const panelText = await panel.textContent()
    expect(panelText).not.toContain(NEW_KEY)
    expect(panelText).not.toContain('sk-ant-demo03-activekey0000000001')
  })

  test('the last remaining key is refused in the UI', async ({ page }) => {
    await page.goto('/settings/models')
    // GLM is seeded with exactly ONE key — the engine's last-key refusal,
    // surfaced as a disabled affordance + the replace-it-via-edit hint.
    await page.getByTestId('provider-keys-toggle-prov-glm').click()
    const panel = page.getByTestId('provider-keys-panel')
    await expect(panel).toBeVisible({ timeout: 10000 })
    const rows = panel.getByTestId('provider-key-row')
    await expect(rows).toHaveCount(1, { timeout: 10000 })
    await expect(rows.first()).toContainText('Active')

    // Removing the only key would strand the provider — refuse it.
    const remove = panel.getByTestId('provider-key-remove-0')
    await expect(remove).toBeDisabled()
    await expect(panel).toContainText(
      'The last remaining key cannot be removed here — edit the provider to replace it.',
    )
  })
})

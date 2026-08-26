// U1 — the app sidebar's session rail is the app's single session list
// (the Chat-page session rail was removed). Runs against the mock build
// (`pnpm demo` webServer, 8 seeded sessions from MOCK_SESSIONS).
import { test, expect } from '@playwright/test'
import AxeBuilder from '@axe-core/playwright'

test.describe('Sidebar sessions rail (U1)', () => {
  test('has exactly one New Chat button', async ({ page }) => {
    await page.goto('/chat')
    await expect(
      page.getByRole('button', { name: 'New Chat' })
    ).toHaveCount(1)
  })

  test('switches session from the rail and marks it current', async ({ page }) => {
    await page.goto('/chat')
    // exact: true — otherwise the substring also matches the row's
    // "Actions for …" ⋯ button.
    const row = page.getByRole('button', { name: 'Chat: Q3 roadmap brainstorm', exact: true })
    await expect(row).toBeVisible()
    await row.click()
    await expect(page).toHaveURL(/\/chat/)
    await expect(
      page.getByRole('button', { name: 'Chat: Q3 roadmap brainstorm', exact: true })
    ).toHaveAttribute('aria-current', 'page')
  })

  test('filters the rail by search', async ({ page }) => {
    await page.goto('/chat')
    // getByRole filters the CSS-hidden mobile-drawer copy of the sidebar
    // that getByLabel would also match (Layout mounts both variants).
    const search = page.getByRole('searchbox', { name: 'Search chats' })
    await search.fill('pricing')

    await expect(
      page.getByRole('button', { name: 'Chat: Pricing page copy review', exact: true })
    ).toBeVisible()
    await expect(
      page.getByRole('button', { name: 'Chat: Q3 roadmap brainstorm', exact: true })
    ).toBeHidden()
    await expect(
      page.getByRole('button', { name: 'Chat: Investor update draft', exact: true })
    ).toBeHidden()
  })

  test('delete asks for confirmation and removes the row', async ({ page }) => {
    await page.goto('/chat')
    // The ⋯ button is hover-only (opacity-0); force-click past the hover gate.
    await page
      .getByRole('button', { name: 'Actions for Investor update draft' })
      .click({ force: true })
    await page.getByRole('menuitem', { name: 'Delete' }).click()

    const dialog = page.getByRole('alertdialog')
    await expect(dialog).toBeVisible()
    await expect(dialog.getByText('Delete Chat')).toBeVisible()
    await dialog.getByRole('button', { name: 'Delete' }).click()

    await expect(dialog).toBeHidden()
    await expect(
      page.getByRole('button', { name: 'Investor update draft' })
    ).toBeHidden()
  })

  test('Alt+ArrowDown moves the focused row (U5 keyboard reorder)', async ({ page }) => {
    await page.goto('/chat')
    const rows = page.getByRole('listitem')
    await expect(rows.first()).toBeVisible()
    // Compare by the row button's aria-label ("Chat: <title>") — innerText
    // also drags in the icon-font glyphs (drag_indicator / more_horiz).
    const nameOf = (i: number) =>
      rows.nth(i).locator('button').first().getAttribute('aria-label')
    const before0 = await nameOf(0)
    const before1 = await nameOf(1)
    expect(before0 && before1 && before0 !== before1).toBeTruthy()

    await page.getByRole('button', { name: before0!, exact: true }).focus()
    await page.keyboard.press('Alt+ArrowDown')

    await expect(rows.nth(0).locator('button').first()).toHaveAttribute('aria-label', before1!)
    await expect(rows.nth(1).locator('button').first()).toHaveAttribute('aria-label', before0!)
    // Persisted: the reorder survives a reload.
    await page.reload()
    await expect(rows.first()).toBeVisible()
    await expect(rows.nth(0).locator('button').first()).toHaveAttribute('aria-label', before1!)
  })

  test('session rail has no critical axe violations (U5)', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByRole('listitem').first()).toBeVisible()
    const results = await new AxeBuilder({ page })
      .include('[data-sidebar]')
      .analyze()
    const critical = results.violations.filter(
      v => v.impact === 'critical' || v.impact === 'serious'
    )
    expect(
      critical.map(v => ({ id: v.id, nodes: v.nodes.length })),
      JSON.stringify(critical.map(v => ({ id: v.id, help: v.help })), null, 2)
    ).toEqual([])
  })
})

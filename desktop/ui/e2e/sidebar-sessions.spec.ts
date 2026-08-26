// U1 — the app sidebar's session rail is the app's single session list
// (the Chat-page session rail was removed). Runs against the mock build
// (`pnpm demo` webServer, 8 seeded sessions from MOCK_SESSIONS).
import { test, expect } from '@playwright/test'

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
})

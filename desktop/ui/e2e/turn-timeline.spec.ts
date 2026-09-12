// §4.14 — Turn Timeline e2e (runs against the mock build: `pnpm demo`).
// Covers the two user paths: opening the panel from a session's ⋯ menu and
// deep-linking to /timeline/:id.

import { test, expect } from '@playwright/test'

test.describe('Turn Timeline (§4.14)', () => {
  test('opens from the session rail menu', async ({ page }) => {
    await page.goto('/chat')

    // The ⋯ button is hover-only: hover the row first, wait for the button
    // to actually mount, then click it normally. (force:true raced the
    // session-list render and intermittently timed out the menu wait.)
    await page.getByText('Q3 roadmap brainstorm').hover()
    const actions = page.getByRole('button', { name: 'Actions for Q3 roadmap brainstorm' })
    await expect(actions).toBeVisible()
    await actions.click()
    const item = page.getByRole('menuitem', { name: 'Turn Timeline' })
    await expect(item).toBeVisible()
    await item.click()

    await expect(page).toHaveURL(/\/timeline\/sess-001$/)
    const panel = page.getByTestId('turn-timeline')
    await expect(panel).toBeVisible()
    await expect(panel.getByRole('heading', { name: 'Turn Timeline' })).toBeVisible()
  })

  test('deep link renders turns, tools, and the cumulative curve', async ({ page }) => {
    // Mock handler returns the same demo projection for any session id, so
    // the deep link needs no prior state.
    await page.goto('/timeline/sess-002')

    const panel = page.getByTestId('turn-timeline')
    await expect(panel.getByRole('heading', { name: 'Turn Timeline' })).toBeVisible()
    await expect(panel.getByText('Turn 1')).toBeVisible()
    await expect(panel.getByText('Turn 2')).toBeVisible()
    await expect(
      panel.getByRole('heading', { name: 'Accumulated tokens & cost' })
    ).toBeVisible()
    await expect(
      panel.getByRole('img', { name: 'Token accumulation curve' })
    ).toBeVisible()

    // Tool waterfall rows carry the mocked tool names + durations.
    await expect(panel.getByTitle(/Read · 6\.0s/)).toBeVisible()
    await expect(panel.getByTitle(/Bash · 3\.0s/)).toBeVisible()
    await expect(panel.getByTitle(/Grep · 4\.0s/)).toBeVisible()
  })
})

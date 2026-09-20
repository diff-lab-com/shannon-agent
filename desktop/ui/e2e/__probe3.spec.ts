import { test, expect } from '@playwright/test'
test('menu path debug', async ({ page }) => {
  const errors: string[] = []
  page.on('pageerror', e => errors.push('PE: ' + e.message.slice(0, 140)))
  await page.goto('/chat')
  await page.getByTestId('desktop-session-row-sess-008').waitFor({ timeout: 15000 })
  await page.getByText('Q3 roadmap brainstorm').hover()
  const actions = page.getByRole('button', { name: 'Actions for Q3 roadmap brainstorm' })
  await expect(actions).toBeVisible()
  await actions.click()
  const item = page.getByRole('menuitem', { name: 'Turn Timeline' })
  await expect(item).toBeVisible()
  await item.click()
  await page.waitForTimeout(2000)
  const hasPanel = await page.locator('[data-testid="turn-timeline"]').count()
  const bodyText = await page.evaluate(() => document.body.innerText.replace(/\s+/g, ' ').slice(0, 200))
  console.log('MENU_DBG:', JSON.stringify({ url: page.url().slice(-30), hasPanel, bodyText, errors: errors.slice(0, 3) }, null, 1))
})

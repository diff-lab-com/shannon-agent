import { test, expect } from '@playwright/test'

test.describe('Shannon Desktop UI', () => {
  test('loads and shows sidebar navigation', async ({ page }) => {
    await page.goto('/')
    const sidebar = page.getByRole('navigation')
    await expect(sidebar).toBeVisible()
  })

  test('sidebar has all nav links', async ({ page }) => {
    await page.goto('/')
    const links = page.locator('aside nav a, aside a')
    const count = await links.count()
    expect(count).toBeGreaterThanOrEqual(4)
  })

  test('chat page renders input area', async ({ page }) => {
    await page.goto('/')
    const input = page.getByPlaceholder(/Ask Shannon/i)
    await expect(input).toBeVisible()
  })

  test('new chat button exists', async ({ page }) => {
    await page.goto('/')
    await expect(page.getByRole('button', { name: /New Chat/i })).toBeVisible()
  })

  test('navigates to scheduled page', async ({ page }) => {
    await page.goto('/')
    await page.getByText('Scheduled').first().click()
    await expect(page.getByText(/Scheduled Tasks/i)).toBeVisible()
  })

  test('navigates to goals page', async ({ page }) => {
    await page.goto('/')
    await page.getByText('Goals').first().click()
    await expect(page.getByText(/Goals/i)).toBeVisible()
  })

  test('send button is disabled when input is empty', async ({ page }) => {
    await page.goto('/')
    const sendBtn = page.getByLabel('Send message')
    await expect(sendBtn).toBeDisabled()
  })

  test('focus styles are applied on tab', async ({ page }) => {
    await page.goto('/')
    await page.keyboard.press('Tab')
    const focused = page.locator(':focus-visible')
    await expect(focused).toHaveCount(1)
  })
})

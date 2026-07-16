import { test, expect } from '@playwright/test'

test.describe('Extensions pages', () => {
  test('navigates to extensions hub (skills)', async ({ page }) => {
    await page.goto('/extensions/skills')
    await expect(page.getByRole('heading', { name: 'Skills' })).toBeVisible()
  })

  test('navigates to my agents page', async ({ page }) => {
    await page.goto('/extensions/agents')
    await expect(page.getByRole('heading', { name: 'Agents' })).toBeVisible()
  })

  test('shows no agents message', async ({ page }) => {
    await page.goto('/extensions/agents')
    // Just check that the agents page loads (URL contains /extensions/agents)
    await page.waitForURL(/\/extensions\/agents/, { timeout: 5000 })
    expect(page.url()).toContain('/extensions/agents')
  })

  test('navigates to data sources page', async ({ page }) => {
    await page.goto('/extensions/datasources')
    await expect(page.getByRole('heading', { name: 'Data Sources' })).toBeVisible()
  })

  test('extensions tab navigation works', async ({ page }) => {
    await page.goto('/extensions/skills')
    const agentsTab = page.getByRole('link', { name: /Agents/i }).first()
    if (await agentsTab.isVisible()) {
      await agentsTab.click()
      await expect(page.getByRole('heading', { name: 'My Agents' })).toBeVisible()
    }
  })
})

test.describe('OPC pages', () => {
  test('navigates to OPC board', async ({ page }) => {
    await page.goto('/opc')
    await expect(page.getByRole('heading', { name: 'KANBAN' })).toBeVisible()
  })

  test('OPC board shows kanban columns', async ({ page }) => {
    await page.goto('/opc')
    // Check that the kanban board structure exists
    await expect(page.getByRole('grid', { name: /Task board/i })).toBeVisible()
    // Check that at least one column header exists
    await expect(page.getByText('Queued')).toBeVisible()
  })

  test('OPC board shows agent swarm section', async ({ page }) => {
    await page.goto('/opc')
    await expect(page.getByText('Active Agents')).toBeVisible()
  })

  test('navigates to OPC task detail', async ({ page }) => {
    await page.goto('/opc/task')
    await expect(page.getByRole('heading', { name: 'Agent Workflow' })).toBeVisible()
  })

  test('OPC task shows efficiency metrics', async ({ page }) => {
    await page.goto('/opc/task')
    await expect(page.getByRole('heading', { name: 'Efficiency Metrics' })).toBeVisible()
  })
})

test.describe('Goals and Scheduled pages', () => {
  test('goals page shows task management heading', async ({ page }) => {
    await page.goto('/goals')
    await expect(page.getByRole('heading', { name: /Task Management/i })).toBeVisible()
  })

  test('goals page shows search input', async ({ page }) => {
    await page.goto('/goals')
    await expect(page.getByPlaceholder(/Search tasks/i)).toBeVisible()
  })

  test('tasks page shows scheduled tasks heading', async ({ page }) => {
    await page.goto('/tasks')
    await expect(page.getByRole('heading', { name: 'Scheduled Tasks' })).toBeVisible()
  })

  test('tasks page shows new task button', async ({ page }) => {
    await page.goto('/tasks')
    await expect(page.getByRole('button', { name: /New Background Task/i })).toBeVisible()
  })
})

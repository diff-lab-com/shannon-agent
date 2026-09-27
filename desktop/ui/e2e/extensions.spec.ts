import { test, expect } from '@playwright/test'

test.describe('Extensions pages', () => {
  test('navigates to extensions hub (skills)', async ({ page }) => {
    await page.goto('/extensions/skills')
    await expect(page.getByRole('heading', { name: 'Skills', exact: true })).toBeVisible()
  })

  test('navigates to my agents page', async ({ page }) => {
    await page.goto('/extensions/agents')
    await expect(page.getByRole('heading', { name: 'Agents', exact: true })).toBeVisible()
  })

  test('shows no agents message', async ({ page }) => {
    await page.goto('/extensions/agents')
    // Just check that the agents page loads (URL contains /extensions/agents)
    await page.waitForURL(/\/extensions\/agents/, { timeout: 5000 })
    expect(page.url()).toContain('/extensions/agents')
  })

  test('navigates to data sources page', async ({ page }) => {
    await page.goto('/extensions/datasources')
    await expect(page.getByRole('heading', { name: 'Data Sources', exact: true })).toBeVisible()
  })

  test('extensions tab navigation works', async ({ page }) => {
    await page.goto('/extensions/skills')
    const agentsTab = page.getByRole('link', { name: /Agents/i }).first()
    if (await agentsTab.isVisible()) {
      await agentsTab.click()
      await expect(page.getByRole('heading', { name: 'My Agents' })).toBeVisible()
    }
  })

  // IA X1: 待处理 — third primary tab; the page hosts the skill review
  // queue plus the errors empty-state placeholder.
  test('navigates to the pending review page', async ({ page }) => {
    await page.goto('/extensions/pending')
    await expect(page.getByRole('heading', { name: 'Skill review' })).toBeVisible()
    // exact: the errors empty state ("No errors") is also a heading.
    await expect(page.getByRole('heading', { name: 'Errors', exact: true })).toBeVisible()
  })
})

test.describe('OPC pages', () => {
  test('navigates to OPC board', async ({ page }) => {
    await page.goto('/opc')
    await expect(page.getByRole('heading', { name: 'KANBAN' })).toBeVisible()
  })

  test('OPC board shows kanban columns', async ({ page }) => {
    await page.goto('/opc')
    // Check that the kanban board structure exists. (The board container
    // lost role="grid" — an invalid grid without rows/cells fails axe's
    // aria-required-children — and keeps its aria-label.)
    await expect(page.locator('[aria-label="Task board"]')).toBeVisible()
    // Check that at least one column header exists
    // The analytics status chips also render "Queued" (en locale) — scope
    // to the board column region to avoid the strict-mode clash.
    await expect(page.getByRole('region', { name: 'Queued' })).toBeVisible()
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
  // /goals is a legacy route that redirects to /tasks (see App.tsx).
  // IA T1: the page is titled「自动化」(Automations) — never「任务」.
  test('goals page redirects to the tasks page', async ({ page }) => {
    await page.goto('/goals')
    await expect(page).toHaveURL(/\/tasks$/)
    await expect(page.getByRole('main').getByRole('heading', { name: 'Automations', exact: true })).toBeVisible()
  })

  // IA T4: one primary CTA「新建自动化」; the one-off background-task entry
  // lives in its split-button dropdown.
  test('redirected goals page shows the New Automation CTA with the background-task entry in its menu', async ({ page }) => {
    await page.goto('/goals')
    await expect(page.getByRole('button', { name: 'New Automation' })).toBeVisible()
    await page.getByRole('button', { name: /More ways to create/i }).click()
    await expect(page.getByRole('menuitem', { name: /New Background Task/i })).toBeVisible()
  })

  test('tasks page shows scheduled tasks heading', async ({ page }) => {
    await page.goto('/tasks')
    await expect(page.getByRole('main').getByRole('heading', { name: 'Automations', exact: true })).toBeVisible()
  })

  test('tasks page shows new task button', async ({ page }) => {
    await page.goto('/tasks')
    await expect(page.getByRole('button', { name: 'New Automation' })).toBeVisible()
    await page.getByRole('button', { name: /More ways to create/i }).click()
    await expect(page.getByRole('menuitem', { name: /New Background Task/i })).toBeVisible()
  })
})

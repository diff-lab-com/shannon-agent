// ZCode delta feature guards (2026-09 P0/P1 batch) — the four behaviors
// that shipped with the ZCode comparison work and had no e2e coverage:
//   ① session rail grouping toggle (project / time, persisted)
//   ② plan mode auto-docks the right dock's Plan tab
//   ③ composer model chip ↔ header selector sync
//   ⑥ agent_spawn renders as a first-class subagent block
// Runs against the mock build (`pnpm demo`) — see src/lib/mock/handlers.ts
// for the seeded sessions / plan / catalog the assertions lean on.
import { test, expect } from '@playwright/test'

test.describe('ZCode delta features (P0/P1)', () => {
  test('① session rail grouping toggle renders time buckets and persists', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible({ timeout: 15000 })

    // Switch to time grouping — seeded sessions span today → 6 days ago,
    // so Today / Yesterday / This week buckets must all appear.
    await page.getByRole('button', { name: 'Group by time' }).click()
    await expect(page.getByText('Today', { exact: true })).toBeVisible()
    await expect(page.getByText('Yesterday', { exact: true })).toBeVisible()
    await expect(page.getByText('This week', { exact: true })).toBeVisible()

    // Preference persists across reloads (localStorage).
    await page.reload()
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible({ timeout: 15000 })
    await expect(page.getByText('Yesterday', { exact: true })).toBeVisible()

    // Back to project grouping (demo has a single project → flat list).
    await page.getByRole('button', { name: 'Group by project' }).click()
    await expect(page.getByText('Yesterday', { exact: true })).toBeHidden()
    await expect(page.getByTestId('desktop-session-row-sess-008')).toBeVisible()
  })

  test('② the dock Plan tab renders the persisted plan document', async ({ page }) => {
    await page.goto('/chat')
    await expect(page.getByPlaceholder('Ask Shannon anything...')).toBeVisible({ timeout: 15000 })

    // Open the dock via the header toggle, then switch to the Plan tab.
    // (Auto-dock on plan-mode entry is covered by the RightDock unit test —
    // the demo mock's approval_mode chain isn't wired to real engine modes.)
    await page.getByRole('button', { name: 'Toggle context panel' }).click()

    const planTab = page.getByRole('tab', { name: 'Plan' })
    await expect(planTab).toBeVisible()
    await planTab.click()
    await expect(planTab).toHaveAttribute('aria-selected', 'true')
    await expect(page.getByTestId('plan-panel')).toBeVisible()
    await expect(page.getByText('Q3 roadmap execution plan')).toBeVisible()
    await expect(page.getByText('Approved')).toBeVisible()
  })

  test('③ composer model chip commits a switch and syncs the header', async ({ page }) => {
    await page.goto('/chat')
    const chip = page.getByRole('combobox', { name: 'Model' })
    await expect(chip).toBeVisible({ timeout: 15000 })
    // Selected value mirrors status.model by NAME (MOCK_STATUS.model is the
    // catalog id; the chip resolves it to the display name), in sync with
    // the header selector which reads the same status.
    await expect(chip).toContainText('Claude Sonnet 4.6')
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText('claude-sonnet-4-6')

    // Radix migration (fix/select-commit): options commit on plain clicks.
    // Switch to GPT-5 — the chip AND the header selector must both reflect
    // the new model (mock get_status mirrors demoConfig, like the engine).
    await chip.click()
    const opt = page.getByRole('option', { name: 'GPT-5', exact: true })
    await opt.waitFor({ timeout: 5000 })
    await opt.click()
    await expect(chip).toContainText('GPT-5', { timeout: 10000 })
    await expect(page.getByRole('button', { name: 'Select model' })).toContainText('GPT-5')
  })

  test('⑥ agent_spawn renders as a first-class subagent block', async ({ page }) => {
    await page.goto('/chat')
    const block = page.getByTestId('subagent-block')
    await expect(block).toBeVisible({ timeout: 15000 })
    await expect(block).toContainText('research-analyst')

    // Expand: spawn prompt + result summary are reachable.
    await block.locator('button').first().click()
    await expect(block).toContainText('research analyst')
    await expect(block).toContainText('q3-roadmap')
  })
})

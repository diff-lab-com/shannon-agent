// R3 journey #13（矩阵#13）— journey-cross-page: the seeded write_file tool
// history renders a FileCard in chat; mounting it registers the path in the
// mock file index, so the sidebar /files page lists the artifact; the
// /timeline/<id> deep link opens the turn timeline (demo trace_timeline).
import { expect, test } from '@playwright/test'

import { loadChatScript } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'

const FILE_PATH = '/Users/demo/workspace/shannon-demo/todo.md'

test.describe('scripted chat backend — journey-cross-page (#13)', () => {
  test('chat 的 write_file 产物落到 /files，/timeline/<id> 深链打开', async ({ page }) => {
    test.setTimeout(60_000)
    await loadChatScript(page, 'cross-page', test.info())
    await page.getByTestId('desktop-session-row-script-sess-cross').click()
    await expect(page.getByRole('heading', { name: 'Cross page' })).toBeVisible({ timeout: 10_000 })

    // The preloaded completed write_file (a FILE_MUTATING_TOOLS member with
    // a path input) renders the artifact as a FileCard under its tool
    // block — no new run needed.
    const fileCard = page.getByText('todo.md', { exact: true }).first()
    await expect(fileCard).toBeVisible({ timeout: 10_000 })

    // Mock-store linkage: the FileCard's mount registered the path — the
    // sidebar Files page lists it.
    await page.getByRole('link', { name: 'Files' }).click()
    await expect(page).toHaveURL(/\/files$/)
    await expect(page.getByText(FILE_PATH)).toBeVisible({ timeout: 10_000 })

    // Timeline deep link (the demo trace_timeline projection answers for
    // any session id).
    await page.goto('/timeline/script-sess-cross')
    await expect(page.getByTestId('turn-timeline')).toBeVisible({ timeout: 10_000 })
    await expect(page.getByRole('heading', { name: 'Timeline', exact: true })).toBeVisible()

    // Back to chat: returning keeps the route state (browser history back
    // to /chat shows the same conversation).
    await page.goBack()
    await expect(page).toHaveURL(/\/files$/)
    await expectNoConsoleErrors(page)
  })
})

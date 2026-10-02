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

    // Mock-store linkage, made deterministic: the entry reaches the index
    // through FileCard's fire-and-forget register_file_index_entry
    // (FileCard.tsx:117-119 → handlers.ts delay(20) mutate) — card DOM
    // visible does NOT prove that invoke landed, and the /files page reads
    // the index ONCE on mount (FilesPage.tsx:63-66). Poll the index through
    // the same route the app uses BEFORE navigating.
    await expect
      .poll(async () =>
        page.evaluate(async (path) => {
          const rows = (await (window as unknown as {
            __TAURI_INTERNALS__: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> }
          }).__TAURI_INTERNALS__.invoke('list_file_index', {})) as Array<{ path: string }>
          return rows.some(r => r.path === path)
        }, FILE_PATH),
      )
      .toBe(true, { timeout: 15_000 })

    // The sidebar Files page lists it. Anchor = the row's title attribute
    // (the full path): the row BODY renders basename + meta only, so a text
    // query for the path never matches /files itself — the pre-rebase
    // passing runs were silently matching the OUTGOING chat DOM (the
    // FileChangesCard's font-mono path span) during the lazy route
    // transition, a false positive the rebase's faster warm-server chunk
    // load removed the window for. Asserting the row directly is the
    // journey's actual claim (the artifact is listed), now transition-proof.
    await page.getByRole('link', { name: 'Files' }).click()
    await expect(page).toHaveURL(/\/files$/)
    await expect(page.locator(`[data-testid="files-row"][title="${FILE_PATH}"]`)).toBeVisible({ timeout: 10_000 })

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

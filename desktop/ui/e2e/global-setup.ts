// Playwright globalSetup — warm the vite dev server for the /chat route
// BEFORE any worker's first spec runs.
//
// Why this exists (CI incidents 110605128646 → 110656923971): every failure
// landed on the alphabetically-first spec files (chat-script.budget,
// chat-script.cancel-matrix) — the canary position. Those specs start while
// the vite dev server is COLD: /chat is a lazy route (App.tsx
// `lazy(() => import('./pages/Chat'))`), so the server has compiled nothing
// when the first page load hits it, and a plain HTTP GET of /chat only
// returns index.html — it never triggers the module-graph transform. On a
// 2-core GitHub runner the cold compile of the Chat chunk (plus its
// dependency graph, interleaved with the browser executing it) can take
// tens of seconds while the first spec is already asserting. Local
// machines finish the same compile in 1–2s, which is why nothing
// reproduced locally (the docker-container experiment ran the exact specs
// green in 35s — the only variable left was runner CPU contention).
//
// What "warm" means here, checked in order:
//   1. the dev server answers at all (HTTP GET /);
//   2. a REAL chromium page loads /chat and the Chat page's own mount
//      marker (the composer textbox) is interactive — this forces the
//      lazy Chat chunk through vite's transform pipeline and leaves the
//      result in the module cache, so every later context (the actual
//      tests) gets instant transforms;
//   3. belt-and-braces: a direct transform request for
//      /src/pages/Chat.tsx returns 200 — proof the transform cache holds
//      the module (after step 2 this is instant; if vite discarded its
//      cache meanwhile, the fetch would still force-compile it).
// Any step that is still pending is retried as a whole: vite can trigger a
// "new dependencies optimized — reloading" pass during the first load, and
// the page reload is part of what the retry absorbs.
import { chromium } from '@playwright/test'
import type { FullConfig } from '@playwright/test'

const WARMUP_TIMEOUT_MS = 120_000
const POLL_INTERVAL_MS = 500

async function fetchOk(url: string): Promise<boolean> {
  try {
    const res = await fetch(url)
    return res.ok
  } catch {
    return false
  }
}

export default async function globalSetup(config: FullConfig): Promise<void> {
  const baseURL = config.projects[0]?.use.baseURL ?? 'http://localhost:1420'
  const deadline = Date.now() + WARMUP_TIMEOUT_MS

  // 1. The dev server answers (playwright's webServer probe should have
  //    guaranteed this already — re-verified cheaply, with the same clock).
  while (!(await fetchOk(`${baseURL}/`))) {
    if (Date.now() > deadline) throw new Error(`[global-setup] dev server never answered at ${baseURL}`)
    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))
  }

  // 2. Drive a real page load — the only way to trigger the lazy /chat
  //    chunk compile — and wait for Chat's own mount marker.
  // 2. Drive a real page load — the only way to trigger the lazy /chat
  //    chunk compile — and wait for Chat's own mount marker. The context
  //    mirrors the projects' use options (locale! viewport!) — the config
  //    pins en-US because locators anchor on en-locale aria-labels
  //    ("Message"); a context left on the OS locale renders zh-CN and the
  //    mount marker never matches.
  const { locale, viewport } = config.projects[0]?.use ?? {}
  const browser = await chromium.launch()
  try {
    const page = await browser.newPage({ locale, viewport: viewport ?? undefined })
    const consoleTail: string[] = []
    page.on('console', (m) => {
      consoleTail.push(`${m.type()}: ${m.text().slice(0, 160)}`)
      if (consoleTail.length > 10) consoleTail.shift()
    })
    for (;;) {
      try {
        await page.goto(`${baseURL}/chat`, { timeout: 30_000 })
        // The composer textbox is ChatInput's stable anchor (ChatPage
        // helper uses the same locator); its visibility proves the Chat
        // module graph finished transforming AND executing.
        await page.getByRole('textbox', { name: 'Message' }).waitFor({ state: 'visible', timeout: 20_000 })
        break
      } catch (error) {
        if (Date.now() > deadline) {
          // Leave enough on-page evidence to diagnose a CI-only warmup
          // failure without another round-trip.
          let state = '<page unavailable>'
          try {
            state = JSON.stringify({
              url: page.url(),
              bodyHead: (await page.evaluate(() => document.body.innerText.slice(0, 200))),
              consoleTail: consoleTail.slice(-5),
            })
          } catch { /* the page may be gone mid-reload */ }
          throw new Error(`[global-setup] /chat never warmed up within ${WARMUP_TIMEOUT_MS}ms: ${String(error)} — page state: ${state}`)
        }
        // Vite dep-optimization reloads and slow first transforms land
        // here — reload and try again until the deadline.
        await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))
      }
    }
  } finally {
    await browser.close()
  }

  // 3. The module-transform completion signal: /src/pages/Chat.tsx must
  //    serve its transformed 200 (from cache after step 2). A 404 means
  //    the module moved — step 2 already did the warming, so warn and
  //    proceed; anything else retries until the deadline.
  for (;;) {
    let status = 0
    try {
      status = (await fetch(`${baseURL}/src/pages/Chat.tsx`)).status
    } catch { /* transient — retry below */ }
    if (status === 200) break
    if (status === 404) {
      // eslint-disable-next-line no-console
      console.warn('[global-setup] /src/pages/Chat.tsx returned 404 (module renamed?) — the page-load warmup in step 2 already compiled the route; continuing.')
      break
    }
    if (Date.now() > deadline) throw new Error(`[global-setup] /src/pages/Chat.tsx never transformed (last status ${status})`)
    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))
  }
}

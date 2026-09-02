// Playwright config for the manual visual walkthrough (see e2e/walkthrough.spec.ts).
//
// Unlike playwright.config.ts (which starts `pnpm demo` in dev mode), this
// serves the PRODUCTION build via `vite preview` — no file watcher, so it
// runs even when the system's inotify watches are exhausted by other dev
// servers. Build first:
//   VITE_MOCK_MODE=1 pnpm build
// then run:
//   WALKTHROUGH=1 pnpm exec playwright test --config playwright.walkthrough.config.ts

import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  testMatch: /walkthrough\.spec\.ts$/,
  timeout: 60_000,
  retries: 0,
  use: {
    baseURL: 'http://localhost:1420',
  },
  webServer: {
    command: 'pnpm exec vite preview --port 1420 --strictPort',
    url: 'http://localhost:1420',
    reuseExistingServer: false,
    timeout: 30_000,
  },
})

// Playwright config for running the CI-grade e2e specs against the
// PRODUCTION build via `vite preview` (no file watcher) — used when the
// dev-server (`pnpm demo`) can't start because system inotify watches are
// exhausted by other dev servers. Build first, then run:
//   VITE_MOCK_MODE=1 pnpm build
//   pnpm exec playwright test --config playwright.preview.config.ts

import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  testMatch: /^(?!.*walkthrough).*\.spec\.ts$/,
  timeout: 30_000,
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

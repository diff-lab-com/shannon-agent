import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  timeout: 30000,
  retries: 0,
  use: {
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
  },
  webServer: {
    // Mock mode (`pnpm demo`) so the UI runs without the Tauri backend:
    // get_config returns a provider, which keeps Layout.tsx from bouncing
    // every fresh browser context to /welcome, and marketplace data is
    // deterministic ([] by default — see src/lib/mock/handlers.ts).
    command: 'pnpm demo',
    port: 1420,
    reuseExistingServer: !process.env.CI,
    timeout: 30000,
  },
})

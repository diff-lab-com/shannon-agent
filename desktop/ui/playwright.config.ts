import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  timeout: 30000,
  // Absorb one transient hydration/hit-target flake on slow CI runners.
  retries: process.env.CI ? 2 : 0,
  use: {
    baseURL: 'http://localhost:1420',
    trace: 'on-first-retry',
    // Lock CI viewport to a wide desktop profile so the rail-vs-drawer
    // decision (matchMedia) is deterministic — the mobile drawer would
    // otherwise put session buttons under the scrim on narrow runners.
    viewport: { width: 1440, height: 900 },
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

import { defineConfig } from '@playwright/test'

export default defineConfig({
  testDir: './e2e',
  // Warm the vite dev server (lazy /chat chunk compiled) before the first
  // worker's first spec — see e2e/global-setup.ts for the CI incidents
  // this closes.
  globalSetup: './e2e/global-setup.ts',
  timeout: 30000,
  // Absorb one transient hydration/hit-target flake on slow CI runners.
  retries: process.env.CI ? 2 : 0,
  // CI runs on 2-core runners where 2 chromium workers + the vite dev
  // server fight for CPU — the structural cause behind every CI-only
  // flake in this suite (four incidents, all confined to the
  // alphabetically-first specs that started against the cold server).
  // Serialize the suite in CI; locally keep Playwright's default
  // (half the cores) so the inner loop stays fast.
  workers: process.env.CI ? 1 : undefined,
  use: {
    baseURL: 'http://localhost:1420',
    // Pin the app language: locators anchor on en-locale aria-labels
    // ("Message" / "Send message"), and a dev machine with a non-English OS
    // locale would otherwise render zh-CN and break every name-based query.
    locale: 'en-US',
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

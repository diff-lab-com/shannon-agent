// Chat NIGHTLY config (R5 chat-testing plan §E) — the "find unknown
// problems" suite: event fuzz, visual state matrix, dynamic a11y scan,
// long-session perf guard, PLUS the full scripted-journey family.
//
// The PR gate (playwright.config.ts) stays untouched behaviorally: it
// ignores the nightly-only families via testIgnore — fuzz, fuzz-found,
// visual-matrix, a11y, perf (R5) plus the wave-2 task-6 env families
// multi-window, voice-input, i18n-theme (D8) — and this config re-includes
// them (testIgnore: [] here overrides the spread base).
//
//   PR gate  → pnpm test:e2e                        (fast, stable subset)
//   nightly  → pnpm exec playwright test --config playwright.chat-nightly.config.ts
//
// Included:
//   e2e/chat-script.*.spec.ts          all scripted journeys (R1-R4) + the
//                                      R5 fuzz / visual-matrix / a11y / perf
//                                      + the wave-2 task-6 env/cross-cutting
//                                      families multi-window / voice-input /
//                                      i18n-theme (D8: nightly-only, never
//                                      in the PR gate's testIgnore escape)
//   e2e/chat-input-persistence.spec.ts scripted (R3) input-draft journeys
//   e2e/sidebar-sessions.spec.ts       NOT scripted, but the R3/G6 load-flake
//                                      family lives here (sidebar row batching)
//                                      — a nightly is exactly where a flake
//                                      should surface, not a PR gate.
import { defineConfig } from '@playwright/test'

import base from './playwright.config'

export default defineConfig({
  ...base,
  // Re-include what the PR gate ignores (see header).
  testIgnore: [],
  testMatch: [
    '**/chat-script.*.spec.ts',
    '**/chat-input-persistence.spec.ts',
    '**/sidebar-sessions.spec.ts',
  ],
  // Brief §E: the nightly runs --workers=2 — this box is not the 2-core CI
  // PR runner, and doubling the PR gate's serialization halves the wall
  // clock of the ~30-file suite. The workflow still passes --workers=2
  // explicitly so the intent survives config edits.
  workers: 2,
  retries: process.env.CI ? 1 : 0,
  // Nightly artifacts: a failure must leave a diagnosable trail (the
  // workflow uploads test-results/ on failure — trace + screenshot).
  trace: 'retain-on-failure',
  screenshot: 'only-on-failure',
})

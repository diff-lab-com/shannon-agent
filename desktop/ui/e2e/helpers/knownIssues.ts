// Known-issue annotation helper (R2 chat-testing plan §A).
//
// Journeys anchored to tracked bugs carry `knownIssue` markers in their
// ChatScript YAML (the player skips those steps and console.info's them).
// The spec side mirrors the marker: a Playwright test annotation (visible in
// the HTML report) plus a console.info line, so the R4 fix is a one-marker
// flip — remove it from the YAML and flip the assertions next to these
// annotations.
import type { TestInfo } from '@playwright/test'

export function annotateKnownIssues(testInfo: TestInfo, markers: Record<string, string>): void {
  for (const [id, description] of Object.entries(markers)) {
    // eslint-disable-next-line no-console
    console.info(`[knownIssue] ${id}: ${description}`)
    testInfo.annotations.push({ type: 'knownIssue', description: `${id}: ${description}` })
  }
}

/** Read the player snapshot through the test console (typed). */
export function mockSnapshot(page: import('@playwright/test').Page): {
  phase: string
  turnIndex: number | null
  stepIndex: number | null
  sentTurns: number
  permissionLog: Array<Record<string, unknown>>
  speed: number
} {
  return page.evaluate(() => {
    const mock = (window as unknown as {
      __shannonMock?: { snapshot(): { phase: string; turnIndex: number | null; stepIndex: number | null; sentTurns: number; permissionLog: Array<Record<string, unknown>>; speed: number } }
    }).__shannonMock
    if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
    return mock.snapshot()
  })
}

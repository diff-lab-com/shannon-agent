// Console watchdog (R1 chat-testing infra, plan §5.1).
//
// Collects pageerror / console.error / unhandledrejection across a whole
// test so every scripted journey can assert `consoleErrors: 0` by default —
// the cheapest guard against the "silent failure" anti-pattern the J3
// walkthrough flagged as the top root cause. An allowlist absorbs known-
// benign noise (kept per-line with a reason, like UNMOCKED_ALLOWLIST).
import { expect, type Page } from '@playwright/test'

export interface WatchdogEntry {
  kind: 'pageerror' | 'console.error' | 'unhandledrejection'
  text: string
}

export class ConsoleWatchdog {
  readonly entries: WatchdogEntry[] = []
  private readonly allowlist: RegExp[]
  /**
   * Resolves once the unhandledrejection hook is registered as an init
   * script — await this BEFORE the first goto so early rejections land.
   */
  readonly ready: Promise<void>

  constructor(page: Page, allowlist: RegExp[] = []) {
    this.allowlist = allowlist
    page.on('pageerror', (err) => {
      this.entries.push({ kind: 'pageerror', text: String(err?.message ?? err) })
    })
    page.on('console', (msg) => {
      if (msg.type() === 'error') {
        this.entries.push({ kind: 'console.error', text: msg.text() })
      }
    })
    page.on('crash', () => {
      this.entries.push({ kind: 'pageerror', text: 'page crashed' })
    })
    // Unhandled promise rejections: Chromium surfaces most as pageerror, but
    // the dedicated event is the portable way to catch them (WebKit/Firefox).
    this.ready = page.addInitScript(() => {
      window.addEventListener('unhandledrejection', (e) => {
        const ev = e as PromiseRejectionEvent
        console.error('[unhandledrejection]', String(ev.reason?.message ?? ev.reason))
      })
    }).then(() => undefined)
  }

  /** Entries that survive the allowlist — the ones that fail assertions. */
  failures(): WatchdogEntry[] {
    return this.entries.filter(e => !this.allowlist.some(re => re.test(e.text)))
  }
}

const watchdogs = new WeakMap<Page, ConsoleWatchdog>()

/** Attach one watchdog per page (idempotent — re-attaches return the first). */
export function attachConsoleWatchdog(page: Page, allowlist: RegExp[] = []): ConsoleWatchdog {
  const existing = watchdogs.get(page)
  if (existing) return existing
  const watchdog = new ConsoleWatchdog(page, allowlist)
  watchdogs.set(page, watchdog)
  return watchdog
}

export function getConsoleWatchdog(page: Page): ConsoleWatchdog | undefined {
  return watchdogs.get(page)
}

/**
 * Assert zero console errors / page errors / unhandled rejections since the
 * watchdog attached. `allowlist` adds to the watchdog's own list.
 */
export async function expectNoConsoleErrors(page: Page, allowlist: RegExp[] = []): Promise<ConsoleWatchdog> {
  const watchdog = watchdogs.get(page) ?? attachConsoleWatchdog(page, allowlist)
  const failures = watchdog.failures().filter(e => !allowlist.some(re => re.test(e.text)))
  expect(
    failures,
    `expected zero console/page errors, got:\n${failures.map(e => `  [${e.kind}] ${e.text}`).join('\n')}`,
  ).toEqual([])
  return watchdog
}

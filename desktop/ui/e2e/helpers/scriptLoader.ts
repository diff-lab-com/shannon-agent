// ChatScript loader (R1 chat-testing infra).
//
// Reads `e2e/scripts/<name>.yaml`, validates it against the SAME ajv schema
// the runtime uses (src/lib/mock/scripted/schema.ts — kept import-safe
// outside Vite on purpose), and injects it via `page.addInitScript` as
// `window.__SHANNON_SCRIPT__`. coreMock's module init reads that global and
// loads the script synchronously — the seed lands BEFORE the app's first
// get_config/list_sessions fetch, then `goto` runs with the player armed.
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { type Page, type TestInfo } from '@playwright/test'
import { parse } from 'yaml'

import { validateScript } from '../../src/lib/mock/scripted/schema'
import { attachConsoleWatchdog, getConsoleWatchdog, type ConsoleWatchdog } from './watchdog'

const HELPERS_DIR = dirname(fileURLToPath(import.meta.url))

/** Read + validate one ChatScript from e2e/scripts/. Throws on bad YAML. */
export function readChatScript(name: string): unknown {
  const file = join(HELPERS_DIR, '..', 'scripts', `${name}.yaml`)
  const doc = parse(readFileSync(file, 'utf8')) as unknown
  const result = validateScript(doc)
  if (!result.ok) {
    throw new Error(`ChatScript "${name}" failed validation:\n${result.errors.join('\n')}`)
  }
  return doc
}

/**
 * Arm an IN-MEMORY script object and navigate to /chat — the object twin of
 * `loadChatScript`. Used by the R5 fuzz spec (mutants are generated JSON,
 * not YAML files) and the perf spec (the long-session seed is built in
 * code). Validates with the SAME ajv schema and throws on a mutant that
 * broke the schema, so "insane semantics" can never masquerade as
 * "malformed document".
 */
export async function loadChatScriptObject(page: Page, script: unknown, testInfo?: TestInfo): Promise<ConsoleWatchdog> {
  const result = validateScript(script)
  if (!result.ok) {
    throw new Error(`loadChatScriptObject: script failed validation:\n${result.errors.join('\n')}`)
  }
  await page.addInitScript((value) => {
    (window as unknown as { __SHANNON_SCRIPT__?: unknown }).__SHANNON_SCRIPT__ = value
  }, script)
  const watchdog = attachConsoleWatchdog(page)
  await watchdog.ready
  await page.goto('/chat')
  if (testInfo) {
    await testInfo.attach('chat-script', { body: JSON.stringify(script, null, 2), contentType: 'application/json' })
  }
  return watchdog
}

/**
 * Arm a script for `page` and navigate to /chat.
 *
 * - `addInitScript` runs before ANY page script on every navigation, so the
 *   scripted backend is seeded before the app mounts (fresh seed data, not
 *   the global demo singletons).
 * - Attaches the console watchdog (and its unhandledrejection init hook)
 *   BEFORE the goto, so boot-time errors — the exact class of silent
 *   failures this infra exists to catch — are collected.
 */
export async function loadChatScript(page: Page, name: string, testInfo?: TestInfo): Promise<ConsoleWatchdog> {
  const script = readChatScript(name)
  await page.addInitScript((value) => {
    (window as unknown as { __SHANNON_SCRIPT__?: unknown }).__SHANNON_SCRIPT__ = value
  }, script)
  const watchdog = attachConsoleWatchdog(page)
  await watchdog.ready
  await page.goto('/chat')
  if (testInfo) {
    await testInfo.attach('chat-script', { body: JSON.stringify(script, null, 2), contentType: 'application/json' })
  }
  return watchdog
}

/** Wait for the player to reach a given phase (e.g. 'waitingUi'). */
export async function expectMockPhase(page: Page, phase: string, timeoutMs = 10_000): Promise<void> {
  await page.waitForFunction(
    (target) => {
      const mock = (window as unknown as {
        __shannonMock?: { snapshot(): { phase: string } }
      }).__shannonMock
      return mock?.snapshot().phase === target
    },
    phase,
    { timeout: timeoutMs },
  )
}

export { getConsoleWatchdog }

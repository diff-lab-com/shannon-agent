// R5 chat-testing plan §D — long-session performance guard ("find unknown
// problems" machine, part 4).
//
// Shape: a seeded 60-message history (30 user/assistant pairs) + a live
// 60-chunk streamed turn (control.speed accelerates the scripted chunk gap
// so the run is dense, not sleepy). Three guards:
//
//   1. CLS  — layout-shift sampled across the WHOLE stream with a buffered
//      PerformanceObserver (the same signal a CDP Performance.domain tap
//      reads; in-page keeps the sampling immune to CDP session churn).
//      Entries flagged hadRecentInput (the send click itself) are excluded.
//      Budget 0.25 per the brief; the measured baseline lives in
//      task-5-report.md §perf.
//   2. Completion budget — wall clock from send to the composer settling
//      (send button back) for the full 60-chunk turn. Threshold = measured
//      baseline rounded up with ~4x headroom, floored at 10s (report §perf).
//   3. Glass budget — the index.css ≤4 backdrop-filter ceiling, evaluated on
//      the settled long session (same counting rule as glass-budget.spec.ts
//      — duplicated here, NOT imported, so the PR-gate spec stays untouched).
//
// NIGHTLY-ONLY: excluded from the PR gate by playwright.config.ts
// testIgnore; run via playwright.chat-nightly.config.ts.
import { expect, test, type Page } from '@playwright/test'

import { ChatPage } from './helpers/ChatPage'
import { loadChatScriptObject } from './helpers/scriptLoader'
import { expectNoConsoleErrors } from './helpers/watchdog'
import type { ChatScript, ScriptSeedMessage } from '../src/lib/mock/scripted/schema'

const SEEDED_PAIRS = 30
const STREAM_CHUNKS = 60
const CHUNK_DELAY_MS = 120
const STREAM_SPEED = 8 // control.speed — 120ms/8 = 15ms gap → a dense burst

/** Brief §D① threshold — validated against the measured baseline (report). */
const CLS_BUDGET = 0.25
/** Brief §D② — measured baseline ×~4, floored at 10s (report §perf). */
const COMPLETION_BUDGET_MS = 20_000
/** index.css glass ceiling, reused on the long-session state. */
const GLASS_BUDGET = 4

/** 60 seeded messages of realistic, mildly-marked-up chat prose. */
function longSessionScript(): ChatScript {
  const messages: ScriptSeedMessage[] = []
  for (let i = 0; i < SEEDED_PAIRS; i++) {
    messages.push({
      role: 'user',
      content: `历史问题 ${i + 1}：第 ${i + 1} 个模块的边界条件、回滚语义与告警阈值应该怎么定？`,
    })
    messages.push({
      role: 'assistant',
      content: `回答 ${i + 1}：三处关键点 —— **幂等写入**、*背压传导*、\`retry budget\`。`
        + `结论：先收紧超时，再放宽并发；指标见 dashboard 第 ${i + 1} 行。`,
    })
  }
  return {
    name: 'perf-long-session',
    description: `R5 perf — ${SEEDED_PAIRS * 2} 条历史消息 + ${STREAM_CHUNKS} 分片流式`,
    seed: {
      config: { hasKey: true },
      sessions: [{ id: 'perf-sess-long', title: 'Perf long session', messages }],
    },
    turns: [{
      user: '在长会话上再跑一轮长流式',
      script: [
        {
          event: 'query:text',
          chunks: Array.from({ length: STREAM_CHUNKS }, (_, i) => `第 ${i + 1} 片结论，`),
          chunkDelayMs: CHUNK_DELAY_MS,
        },
        { event: 'query:completed' },
      ],
    }],
  }
}

/** Buffered layout-shift accumulator (excluding user-input-adjacent shifts). */
async function installClsObserver(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const w = window as unknown as {
      __cls?: { value: number, count: number, worst: Array<{ v: number, at: number, nodes: string[] }> }
    }
    w.__cls = { value: 0, count: 0, worst: [] }
    new PerformanceObserver((list) => {
      const box = w.__cls
      if (!box) return
      for (const entry of list.getEntries()) {
        const e = entry as PerformanceEntry & { value?: number, hadRecentInput?: boolean, sources?: Array<{ node?: { nodeName?: string } }> }
        if (e.hadRecentInput) continue
        const v = e.value ?? 0
        box.value += v
        box.count += 1
        if (box.worst.length < 8) {
          box.worst.push({
            v: Math.round(v * 10000) / 10000,
            at: Math.round(entry.startTime),
            nodes: (e.sources ?? []).map(s => s.node?.nodeName ?? '?').slice(0, 4),
          })
        }
      }
    }).observe({ type: 'layout-shift', buffered: true })
  })
}

test.describe('chat long-session performance guard', () => {
  test(`${SEEDED_PAIRS * 2}-message session + ${STREAM_CHUNKS}-chunk stream: CLS, completion budget, glass budget`, async ({ page }) => {
    test.setTimeout(120_000)
    const chat = new ChatPage(page)
    await installClsObserver(page)
    await loadChatScriptObject(page, longSessionScript(), test.info())

    // History actually mounted before the run starts. The message area
    // VIRTUALIZES (react-virtual): only the viewport window mounts (~9 rows
    // of 60) — so the seed is asserted via the rendered TAIL (the last
    // seeded reply), and the small DOM count is the virtualizer working,
    // not a missing seed.
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })
    expect(await chat.messageCount()).toBeGreaterThan(0)
    await expect(chat.bubbleAt(SEEDED_PAIRS * 2 - 1)).toContainText(`回答 ${SEEDED_PAIRS}`, { timeout: 10_000 })

    // Dense burst: accelerate the scripted chunk gap for THIS turn (the
    // constant must be passed across the browser boundary explicitly).
    await page.evaluate((speed) => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { speed: number } }
      }).__shannonMock
      if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
      mock.control.speed = speed
    }, STREAM_SPEED)

    // Scope CLS to the stream: drop boot-time shifts accumulated so far.
    await page.evaluate(() => {
      const box = (window as unknown as { __cls?: { value: number, count: number, worst: unknown[] } }).__cls
      if (box) { box.value = 0; box.count = 0; box.worst = [] }
    })

    const t0 = Date.now()
    await chat.send('在长会话上再跑一轮长流式')
    // The whole turn (60 chunks + commit + render) must settle under budget.
    await expect(chat.sendButton()).toBeVisible({ timeout: COMPLETION_BUDGET_MS })
    const completionMs = Date.now() - t0

    // Let the post-commit render (markdown of the full reply) flush before
    // sampling CLS and the glass census.
    await page.waitForTimeout(1_500)
    const cls = await page.evaluate(() => (window as unknown as {
      __cls?: { value: number, count: number, worst: Array<{ v: number, at: number, nodes: string[] }> }
    }).__cls)
    const clsValue = cls?.value ?? Number.NaN

    // Glass census — same rule as glass-budget.spec.ts (visible elements
    // whose computed backdrop-filter is active), on the settled long state.
    const glass = await page.evaluate(() => {
      const hits: string[] = []
      for (const el of document.querySelectorAll('*')) {
        const cs = getComputedStyle(el)
        const bf = cs.backdropFilter || cs.getPropertyValue('-webkit-backdrop-filter')
        if (!bf || bf === 'none') continue
        const rect = el.getBoundingClientRect()
        if (rect.width <= 0 || rect.height <= 0) continue
        if (cs.visibility === 'hidden' || cs.display === 'none') continue
        if (Number.parseFloat(cs.opacity || '1') === 0) continue
        hits.push(`<${el.tagName.toLowerCase()}>`)
      }
      return hits
    })

    const report = {
      seededMessages: SEEDED_PAIRS * 2,
      streamChunks: STREAM_CHUNKS,
      streamSpeed: STREAM_SPEED,
      completionMs,
      completionBudgetMs: COMPLETION_BUDGET_MS,
      cls: { value: clsValue, entries: cls?.count ?? 0, worst: cls?.worst ?? [] },
      glass: { count: glass.length, budget: GLASS_BUDGET, elements: glass },
    }
    await test.info().attach('perf-stats', { body: JSON.stringify(report, null, 2), contentType: 'application/json' })
    // eslint-disable-next-line no-console
    console.log(`[perf] completion=${completionMs}ms (budget ${COMPLETION_BUDGET_MS}) · CLS=${clsValue.toFixed(4)} over ${cls?.count ?? 0} shifts (budget ${CLS_BUDGET}) · glass=${glass.length}/${GLASS_BUDGET}`)

    expect(
      completionMs,
      `60-chunk turn took ${completionMs}ms to settle (budget ${COMPLETION_BUDGET_MS}ms)`,
    ).toBeLessThanOrEqual(COMPLETION_BUDGET_MS)
    expect(
      clsValue,
      `layout shift during streaming = ${clsValue} (budget ${CLS_BUDGET}); worst: ${JSON.stringify(cls?.worst)}`,
    ).toBeLessThanOrEqual(CLS_BUDGET)
    expect(
      glass.length,
      `long chat session renders ${glass.length} backdrop-filter elements (budget ${GLASS_BUDGET}): ${glass.join(' ')}`,
    ).toBeLessThanOrEqual(GLASS_BUDGET)

    // A long session under storm must still end clean — no swallowed errors.
    await expectNoConsoleErrors(page)
  })
})

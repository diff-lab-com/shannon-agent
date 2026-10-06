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
    // The tail row only exists once the scroll container reaches the bottom.
    // The initial auto-follow is a SMOOTH scroll — on a loaded CI runner it
    // can still be mid-flight (or interrupted by the projection commits) when
    // the assertion fires, leaving row 59 unmounted ("element(s) not found",
    // first-ever CI nightly run). Scroll deterministically, then assert.
    await chat.scrollToBottom()
    await expect(chat.bubbleAt(SEEDED_PAIRS * 2 - 1)).toContainText(`回答 ${SEEDED_PAIRS}`, { timeout: 15_000 })

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

  // ── B3-5 pin（plan §七-9 / B3-2 代理）───────────────────────────────────
  //
  // The long-reply streaming proxy: a single turn streams a >8K-token reply
  // (~500 chunks × 48 CJK chars ≈ 24K chars — ≥8K tokens even at a
  // conservative 2 chars/token). NIGHTLY-ONLY like the guard above (the
  // config testIgnore keeps the whole file out of the PR gate). Same
  // measurement口径 as the guard: wall-clock completion budget, plus the
  // B3-2 proxy metric — main-thread LONG-TASK sampling across the stream
  // (before B3-2's incremental rendering, every coalesced flush re-parses
  // the whole growing markdown; the long-task totals quantify that tax and
  // give the B3-2 landing a before/after anchor). Thresholds = measured
  // local baseline (report attached per run) rounded up with headroom.
  const LONG_CHUNKS = 500
  const LONG_CHUNK_CHARS = 48
  const LONG_CHUNK_DELAY_MS = 120
  const LONG_STREAM_SPEED = 8 // 120ms/8 = 15ms gap → a dense burst
  /** Measured baseline 9.1s locally (500 × 15ms stream + settle + commit);
   *  ×~3 for loaded-runner headroom. */
  const LONG_COMPLETION_BUDGET_MS = 30_000
  /** Measured baseline 51ms over 1 task across the whole stream locally
   *  (the coalesced projection keeps re-parse cost near-invisible at this
   *  size — B3-2's job is to keep it that way as the surface grows). The
   *  budget is an absolute jank ceiling: >2s of blocked main thread during
   *  a ~7.5s stream means streaming went regressive regardless of baseline.
   *  CI CALIBRATION (first-ever CI nightly run, 2-core runner): total hit
   *  7141/7600ms over 85-88 tasks — ~85ms/task of slower hardware, NOT a
   *  re-parse regression (worst task stayed 141-193ms, i.e. bounded per
   *  flush). Total budget raised to 12s for the runner class; the
   *  regression signal moved to LONGTASK_WORST_BUDGET_MS below, which
   *  hardware cannot excuse (a full 24K-char re-parse would push the worst
   *  single task into seconds — B3-2 keeps it bounded). */
  const LONGTASK_TOTAL_BUDGET_MS = 12_000
  /** Worst SINGLE long task across the stream: the O(n²) re-parse tax shows
   *  up here (one flush = one full-document parse of a growing doc), while
   *  B3-2's incremental rendering keeps each flush bounded. */
  const LONGTASK_WORST_BUDGET_MS = 500

  const LONG_PHRASE = '流式渲染压力测试句子，覆盖分片边界与增量刷新路径。'

  /** One deterministic chunk of the long reply (shared by the script builder
   *  and the flush-completeness assert — the expected text IS the script). */
  function longReplyChunk(i: number): string {
    const prefix = `第 ${i + 1} 段：`
    return prefix + LONG_PHRASE.repeat(3).slice(0, LONG_CHUNK_CHARS - prefix.length)
  }

  function longReplyScript(): ChatScript {
    const chunks = Array.from({ length: LONG_CHUNKS }, (_, i) => longReplyChunk(i))
    return {
      name: 'perf-long-reply',
      description: `B3-5 perf proxy — 单轮 ${LONG_CHUNKS} 分片长回复（~${LONG_CHUNKS * LONG_CHUNK_CHARS} 字 ≈ >8K token）`,
      seed: {
        config: { hasKey: true },
        sessions: [{ id: 'perf-sess-long-reply', title: 'Perf long reply', messages: [] }],
      },
      turns: [{
        user: '请输出这篇长文全文',
        script: [
          { event: 'query:text', chunks, chunkDelayMs: LONG_CHUNK_DELAY_MS },
          { event: 'query:completed' },
        ],
      }],
    }
  }

  /** Buffered main-thread long-task accumulator (the B3-2 proxy metric). */
  async function installLongTaskObserver(page: Page): Promise<void> {
    await page.addInitScript(() => {
      const w = window as unknown as {
        __longtask?: { count: number; totalMs: number; worstMs: number }
      }
      w.__longtask = { count: 0, totalMs: 0, worstMs: 0 }
      new PerformanceObserver((list) => {
        const box = w.__longtask
        if (!box) return
        for (const entry of list.getEntries()) {
          box.count += 1
          box.totalMs += entry.duration
          box.worstMs = Math.max(box.worstMs, entry.duration)
        }
      }).observe({ type: 'longtask', buffered: true })
    })
  }

  test(`${LONG_CHUNKS}-chunk long reply (~${(LONG_CHUNKS * LONG_CHUNK_CHARS) / 1000}K chars ≈ >8K tokens): completion budget, long-task sample, flush completeness`, async ({ page }) => {
    test.setTimeout(180_000)
    const chat = new ChatPage(page)
    await installLongTaskObserver(page)
    await loadChatScriptObject(page, longReplyScript(), test.info())
    await expect(chat.composer()).toBeVisible({ timeout: 15_000 })

    // Dense burst for THIS turn (the constant crosses the browser boundary
    // explicitly, same as the guard above).
    await page.evaluate((speed) => {
      const mock = (window as unknown as {
        __shannonMock?: { control: { speed: number } }
      }).__shannonMock
      if (!mock) throw new Error('window.__shannonMock missing — demo mock build not active?')
      mock.control.speed = speed
    }, LONG_STREAM_SPEED)

    // Scope the long-task window to the stream: drop anything boot emitted.
    await page.evaluate(() => {
      const box = (window as unknown as { __longtask?: { count: number; totalMs: number; worstMs: number } }).__longtask
      if (box) { box.count = 0; box.totalMs = 0; box.worstMs = 0 }
    })

    const t0 = Date.now()
    await chat.send('请输出这篇长文全文')
    // Mid-stream liveness: the streaming cursor is up while chunks land.
    await chat.expectStreamingCursor()
    // The whole turn (500 chunks + final commit + full-markdown render) must
    // settle under budget.
    await expect(chat.sendButton()).toBeVisible({ timeout: LONG_COMPLETION_BUDGET_MS })
    const completionMs = Date.now() - t0

    // Let the post-commit render flush before sampling the long-task totals.
    await page.waitForTimeout(1_500)
    const lt = await page.evaluate(() => (window as unknown as {
      __longtask?: { count: number; totalMs: number; worstMs: number }
    }).__longtask)

    // Flush completeness: the committed bubble carries the ENTIRE reply —
    // every one of the 500 chunks landed through the throttled projection,
    // tail included (a dropped flush window would truncate the commit).
    const chunks = Array.from({ length: LONG_CHUNKS }, (_, i) => longReplyChunk(i))
    await chat.expectBubbleText(1, chunks.join(''))

    const report = {
      streamChunks: LONG_CHUNKS,
      approxChars: LONG_CHUNKS * LONG_CHUNK_CHARS,
      streamSpeed: LONG_STREAM_SPEED,
      completionMs,
      completionBudgetMs: LONG_COMPLETION_BUDGET_MS,
      longtask: { count: lt?.count ?? 0, totalMs: Math.round(lt?.totalMs ?? 0), worstMs: Math.round(lt?.worstMs ?? 0), budgetMs: LONGTASK_TOTAL_BUDGET_MS, worstBudgetMs: LONGTASK_WORST_BUDGET_MS },
    }
    await test.info().attach('perf-long-reply-stats', { body: JSON.stringify(report, null, 2), contentType: 'application/json' })
    // eslint-disable-next-line no-console
    console.log(`[perf-long-reply] completion=${completionMs}ms (budget ${LONG_COMPLETION_BUDGET_MS}) · longtask total=${Math.round(lt?.totalMs ?? 0)}ms over ${lt?.count ?? 0} tasks, worst=${Math.round(lt?.worstMs ?? 0)}ms (budget ${LONGTASK_TOTAL_BUDGET_MS})`)

    expect(
      completionMs,
      `the ${LONG_CHUNKS}-chunk long reply took ${completionMs}ms to settle (budget ${LONG_COMPLETION_BUDGET_MS}ms)`,
    ).toBeLessThanOrEqual(LONG_COMPLETION_BUDGET_MS)
    expect(
      Math.round(lt?.totalMs ?? 0),
      `main-thread long-task time during the long-reply stream = ${Math.round(lt?.totalMs ?? 0)}ms over ${lt?.count ?? 0} tasks, worst ${Math.round(lt?.worstMs ?? 0)}ms (budget ${LONGTASK_TOTAL_BUDGET_MS}ms — the B3-2 re-parse tax proxy)`,
    ).toBeLessThanOrEqual(LONGTASK_TOTAL_BUDGET_MS)
    expect(
      Math.round(lt?.worstMs ?? 0),
      `worst single long task during the long-reply stream = ${Math.round(lt?.worstMs ?? 0)}ms (budget ${LONGTASK_WORST_BUDGET_MS}ms — a full-document re-parse of a growing 24K-char doc would blow past this; B3-2's incremental flushes must stay bounded)`,
    ).toBeLessThanOrEqual(LONGTASK_WORST_BUDGET_MS)

    // A long stream must still end clean — no swallowed errors.
    await expectNoConsoleErrors(page)
  })
})

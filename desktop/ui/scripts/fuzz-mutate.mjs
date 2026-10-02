// R5 chat-testing plan §5.2 — ChatScript event-fuzz mutator.
//
// Reads the legal baseline (`e2e/scripts/fuzz/base.yaml`) and derives a FIXED
// set of schema-valid but semantics-hostile mutants into
// `e2e/scripts/fuzz/mutants/*.json`, plus a `manifest.json` the fuzz spec
// (chat-script.fuzz.spec.ts) consumes. Categories (task-5 brief §A):
//
//   reorder      tool-result before tool-start · completed before the text
//   duplicate    double tool-start with the same id · double completed
//   cross-session events stamped with the WRONG session_id / a null one
//   boundary     empty chunk · emoji-only chunks · one ~20k-char line ·
//                tool-progress -1 and 1.5 · usage missing its token fields ·
//                unknown meta.classification on a tool result
//   bombardment  100 chunks at chunkDelayMs 0
//
// Determinism contract: no Date.now / Math.random anywhere — the only
// randomness is the fixed-seed mulberry32 PRNG below, so two `pnpm fuzz:gen`
// runs produce byte-identical files (CI runs the generator before the suite;
// the committed mutants make local `playwright test` work without the step
// and keep every mutant reviewable in git).
//
// Mutants must PASS the ChatScript ajv schema (validateScript in the spec
// fails loudly otherwise) — these are insane SEMANTICS, not malformed docs.
// The one thing the schema cannot express (`session_id` absent — the player
// always fills it) is approximated with an explicit `null`, which is the
// app's "route window-locally" shape.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { parse } from 'yaml'

const HERE = dirname(fileURLToPath(import.meta.url))
const BASE_YAML = join(HERE, '..', 'e2e', 'scripts', 'fuzz', 'base.yaml')
const OUT_DIR = join(HERE, '..', 'e2e', 'scripts', 'fuzz', 'mutants')

const SEED = 20261002

/** mulberry32 — tiny seeded PRNG; deterministic across runs/platforms. */
function mulberry32(seed) {
  let a = seed >>> 0
  return () => {
    a |= 0; a = (a + 0x6d2b79f5) | 0
    let t = Math.imul(a ^ (a >>> 15), 1 | a)
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

function clone(value) {
  return JSON.parse(JSON.stringify(value))
}

/** Deep-copy the base with a fresh name/description and a single turn. */
function rebase(base, name, description) {
  const doc = clone(base)
  doc.name = name
  doc.description = description
  doc.turns = [clone(doc.turns[0])]
  delete doc.onCancel
  return doc
}

/** Index of the first step with the given event name in the (single) turn. */
function stepIndex(doc, event) {
  return doc.turns[0].script.findIndex(s => s.event === event)
}

/** Stamp `patch` over every step's payload (chunks steps keep their content). */
function stampEveryPayload(doc, patch) {
  for (const step of doc.turns[0].script) {
    step.payload = { ...(step.payload ?? {}), ...patch }
  }
}

// ── seeded content factories ────────────────────────────────────────────────

const WORDS = ['目录', 'stream', '🌊', 'buffer', '检查', 'render', '⚙️', 'chunk', '写', 'test', '🎉', 'node', '流', 'fix', '📦']

/** ~`chars`-character pseudo-random single line (no newlines — markdown-prose). */
function longLine(chars) {
  const rand = mulberry32(SEED)
  let out = ''
  while (out.length < chars) {
    out += WORDS[Math.floor(rand() * WORDS.length)]
    out += rand() < 0.7 ? ' ' : ''
  }
  return out.slice(0, chars)
}

/** 100 bombardment chunks — seeded alternation of short fragments. */
function bombardmentChunks(count) {
  const rand = mulberry32(SEED + 1)
  return Array.from({ length: count }, (_, i) =>
    `${WORDS[Math.floor(rand() * WORDS.length)]}-${i}`)
}

// ── mutants ─────────────────────────────────────────────────────────────────

/** @type {Array<{category: string, name: string, description: string, build: (base: unknown) => unknown}>} */
const MUTANTS = [
  // ── reorder ──────────────────────────────────────────────────────────────
  {
    category: 'reorder',
    name: 'reorder-tool-result-before-start',
    description: 'tool-result 先于 tool-start（同一 tool_use_id）— 卡片必须在结果早到时不崩、终态收敛',
    build(base) {
      const doc = rebase(base, 'fuzz-reorder-tool-result-before-start',
        'fuzz 乱序：tool-result 先于 tool-start')
      const script = doc.turns[0].script
      const start = stepIndex(doc, 'query:tool-start')
      const result = stepIndex(doc, 'query:tool-result')
      // Swap the two steps in place — the result now lands first.
      ;[script[start], script[result]] = [script[result], script[start]]
      return doc
    },
  },
  {
    category: 'reorder',
    name: 'reorder-completed-before-text',
    description: 'completed 先于 text 分片 — 终态后的迟到分片必须被丢弃（player 终态语义）',
    build(base) {
      const doc = rebase(base, 'fuzz-reorder-completed-before-text',
        'fuzz 乱序：completed 先于 text 分片')
      const script = doc.turns[0].script
      const completed = stepIndex(doc, 'query:completed')
      const text = stepIndex(doc, 'query:text')
      ;[script[completed], script[text]] = [script[text], script[completed]]
      return doc
    },
  },

  // ── duplicate ────────────────────────────────────────────────────────────
  {
    category: 'duplicate',
    name: 'duplicate-tool-start-same-id',
    description: '双 tool-start 同一 tool_use_id — 卡片必须去重（A-7 语义）而非渲染两张',
    build(base) {
      const doc = rebase(base, 'fuzz-duplicate-tool-start-same-id',
        'fuzz 重复：同一 tool_use_id 的 tool-start 发两次')
      const script = doc.turns[0].script
      script.splice(stepIndex(doc, 'query:tool-start'), 0,
        clone(script[stepIndex(doc, 'query:tool-start')]))
      return doc
    },
  },
  {
    category: 'duplicate',
    name: 'duplicate-completed-in-turn',
    description: '同回合双 query:completed — player 终态屏蔽必须兜住（第二个被丢弃），UI 不重复提交',
    build(base) {
      const doc = rebase(base, 'fuzz-duplicate-completed-in-turn',
        'fuzz 重复：同回合两条 query:completed')
      const script = doc.turns[0].script
      script.splice(stepIndex(doc, 'query:completed'), 0,
        clone(script[stepIndex(doc, 'query:completed')]))
      return doc
    },
  },

  // ── cross-session ────────────────────────────────────────────────────────
  {
    category: 'cross-session',
    name: 'cross-session-wrong-session-id',
    description: '所有事件 session_id 指向另一个真实种子会话（fuzz-sess-b）— 当前视图不得串台、不得崩',
    build(base) {
      const doc = rebase(base, 'fuzz-cross-session-wrong-session-id',
        'fuzz 串扰：事件全部盖成别人的 session_id')
      stampEveryPayload(doc, { session_id: 'fuzz-sess-b' })
      return doc
    },
  },
  {
    category: 'cross-session',
    name: 'cross-session-null-session-id',
    description: '所有事件 session_id 显式 null（缺失语义）— 必须 window 本地路由，不崩不复位失败',
    build(base) {
      const doc = rebase(base, 'fuzz-cross-session-null-session-id',
        'fuzz 串扰：事件 session_id 全为 null（缺失）')
      stampEveryPayload(doc, { session_id: null })
      return doc
    },
  },

  // ── boundary payloads ────────────────────────────────────────────────────
  {
    category: 'boundary',
    name: 'boundary-empty-chunk',
    description: '空字符串 chunk — streamingText 拼接与 Markdown 渲染必须容忍',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-empty-chunk', 'fuzz 边界：空 chunk')
      const text = doc.turns[0].script[stepIndex(doc, 'query:text')]
      text.chunks = ['', '']
      return doc
    },
  },
  {
    category: 'boundary',
    name: 'boundary-emoji-only',
    description: '纯 emoji chunk（含 ZWJ 序列）— 计数/渲染不得误判',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-emoji-only', 'fuzz 边界：纯 emoji 分片')
      const text = doc.turns[0].script[stepIndex(doc, 'query:text')]
      text.chunks = ['🌊', '🧑‍💻', '👍🏽', '🔥🔥']
      return doc
    },
  },
  {
    category: 'boundary',
    name: 'boundary-superlong-line',
    description: '单 chunk ~20000 字符超长行 — 长词断行/渲染不挂起',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-superlong-line', 'fuzz 边界：~20k 字符单行')
      const text = doc.turns[0].script[stepIndex(doc, 'query:text')]
      text.chunks = [longLine(20_000)]
      text.chunkDelayMs = 30
      return doc
    },
  },
  {
    category: 'boundary',
    name: 'boundary-progress-out-of-range',
    description: 'tool-progress progress=-1 与 1.5 — 进度 pill 的区间守卫必须兜住',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-progress-out-of-range', 'fuzz 边界：progress -1 / 1.5')
      const script = doc.turns[0].script
      const progress = script[stepIndex(doc, 'query:tool-progress')]
      progress.payload = { ...progress.payload, progress: -1 }
      script.splice(stepIndex(doc, 'query:tool-progress') + 1, 0, {
        event: 'query:tool-progress',
        payload: { ...clone(progress.payload), progress: 1.5 },
      })
      return doc
    },
  },
  {
    category: 'boundary',
    name: 'boundary-usage-missing-tokens',
    description: 'usage 只带 cost_usd（input/output tokens 缺失）— 用量展示必须容忍缺字段',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-usage-missing-tokens', 'fuzz 边界：usage 缺 token 字段')
      const usage = doc.turns[0].script[stepIndex(doc, 'query:usage')]
      usage.payload = { cost_usd: 0.01 }
      return doc
    },
  },
  {
    category: 'boundary',
    name: 'boundary-unknown-meta-classification',
    description: 'tool-result meta.classification 未知枚举 — meta 透传不得崩、不得渲染垃圾',
    build(base) {
      const doc = rebase(base, 'fuzz-boundary-unknown-meta-classification', 'fuzz 边界：未知 meta.classification')
      const result = doc.turns[0].script[stepIndex(doc, 'query:tool-result')]
      result.payload = {
        ...result.payload,
        meta: { classification: 'definitely-not-a-known-class', confidence: 1.5 },
      }
      return doc
    },
  },

  // ── bombardment ──────────────────────────────────────────────────────────
  {
    category: 'bombardment',
    name: 'bombardment-100-chunks-0ms',
    description: '100 个 chunk、chunkDelayMs=0 轰炸 — 事件风暴下终态幂等、无崩溃',
    build(base) {
      const doc = rebase(base, 'fuzz-bombardment-100-chunks-0ms', 'fuzz 轰炸：100 chunks @ 0ms')
      const text = doc.turns[0].script[stepIndex(doc, 'query:text')]
      text.chunks = bombardmentChunks(100)
      text.chunkDelayMs = 0
      return doc
    },
  },
]

// ── main ────────────────────────────────────────────────────────────────────

const base = parse(readFileSync(BASE_YAML, 'utf8'))
if (!base || base.name !== 'fuzz-baseline' || !Array.isArray(base.turns)) {
  throw new Error(`fuzz-mutate: ${BASE_YAML} does not look like the fuzz baseline`)
}

mkdirSync(OUT_DIR, { recursive: true })

/** @type {Array<{file: string, category: string, description: string}>} */
const manifestMutants = []
for (const mutant of MUTANTS) {
  const doc = mutant.build(base)
  const file = `${mutant.name}.json`
  // Sanity: the mutant must keep the single-turn shape the spec drives.
  if (doc.turns.length !== 1 || !doc.turns[0].user) {
    throw new Error(`fuzz-mutate: ${mutant.name} lost its single-turn shape`)
  }
  writeFileSync(join(OUT_DIR, file), `${JSON.stringify(doc, null, 2)}\n`)
  manifestMutants.push({ file, category: mutant.category, description: mutant.description })
}

const categories = {}
for (const m of manifestMutants) categories[m.category] = (categories[m.category] ?? 0) + 1

const manifest = {
  generator: 'scripts/fuzz-mutate.mjs',
  seed: SEED,
  baseline: 'e2e/scripts/fuzz/base.yaml',
  categories,
  total: manifestMutants.length,
  mutants: manifestMutants,
}
writeFileSync(join(OUT_DIR, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`)

const perCategory = Object.entries(categories).map(([k, v]) => `${k}=${v}`).join(' ')
// eslint-disable-next-line no-console
console.log(`[fuzz-mutate] seed=${SEED} wrote ${manifestMutants.length} mutants (${perCategory}) → ${OUT_DIR}`)

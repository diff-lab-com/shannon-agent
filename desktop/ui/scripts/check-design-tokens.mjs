#!/usr/bin/env node
// Design-system guardrails (UI audit 2026-09 acceptance criteria, §7 Wave 2;
// extended by UI review 2026-09-29 Batch 1):
//   1. Terminology consistency — the retired dual-track terms must never
//      reappear in component sources (i18n locale values are the single
//      source of truth).
//   2. Design-token adoption — raw Tailwind palette classes and hard-coded
//      hex colors are rejected in component sources; use MD3 theme tokens
//      (--color-*, bg-surface-*, text-on-*, semantic classes) instead.
//   3. Motion durations — `duration-<ms>` is rejected; use the @theme tokens
//      via `duration-(--duration-fast|normal|slow|slower)` (100/160/240/400).
//      Nearest-token mapping: 100→fast, 150-200→normal, 240-300→slow, >300→slower.
//   4. Elevation — bare `shadow-sm|md|lg|xl|2xl` is rejected; use
//      `shadow-e1..e5`. A line that also carries a shadow COLOR class
//      (shadow-primary/30 …) only WARNs: the e-series is a fixed box-shadow
//      and cannot compose with Tailwind's --tw-shadow-color mechanism.
//   5. Type sizes — `text-[Npx]` on a material-symbols line is rejected (use
//      the icon-* utilities); elsewhere it WARNs (migrated 2026-09-29 down to
//      5 deliberate off-scale values, e.g. EfficiencyCard's 40px; the warn
//      channel keeps them visible until each gets a role or a documented
//      exception).
// Exit 1 on any error so CI can gate on it; warnings print only.
// Scope: desktop/ui/src only; tests may reference retired strings on purpose
// (they assert the change).

import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

const ROOT = new URL('../src', import.meta.url).pathname

const RETIRED_TERMS = [
  '已排程', '分流队列', '并行方案', '聚焦聊天', '单人公司',
  '定时任务', // page header used a different word than the nav — use 任务
]

// Raw palette classes (not theme tokens). Allowed: allowlist below.
const RAW_COLOR_CLASS = /(?:text|bg|border|ring|fill|stroke)-(?:gray|slate|zinc|neutral|stone|blue|sky|cyan|teal|emerald|green|lime|yellow|amber|orange|red|rose|pink|fuchsia|purple|violet|indigo)-(?:\d{2,3})\b/
const RAW_HEX = /#[0-9a-fA-F]{6}\b/

// Batch 1 (2026-09-29): token-bypass classes.
const DURATION_RAW = /\bduration-\d+\b/
const SHADOW_TOKEN = /(?:[a-zA-Z0-9-]+:)?shadow-([^\s"'`,)]+)/g
const SHADOW_SIZES = new Set(['sm', 'md', 'lg', 'xl', '2xl'])
const SHADOW_ALLOWED = new Set(['none', 'inner', 'e1', 'e2', 'e3', 'e4', 'e5'])
const TEXT_PX = /\btext-\[\d+px\]/
const ICON_MARKER = 'material-symbols-outlined'

const ALLOWLIST = [
  '__tests__/',              // tests assert literal values on purpose
  'components/extensions/', // brand gradient buttons (documented exception, 06-26 audit)
  'theme/',                 // generated theme machinery
  'i18n/',                  // locale strings
  'index.css',              // the @theme token source — hex lives here by definition
  'components/terminal/',   // xterm.js API consumes raw hex palettes by contract
  'components/editor/cmTheme.ts', // same contract: CodeMirror's theme extension needs literal palette floors (mirrors xtermTheme)
  'components/artifact/MermaidRenderer.tsx', // hex lives inside a standalone iframe document — parent vars cannot cross
  'lib/timelineExport.ts', // same contract: standalone exported HTML document — parent vars cannot cross (office Wave 3 C6)
  'components/CommandPalette.tsx', // synonyms field preserves retired terms during the migration window (audit §6.1)
  'lib/mock/',              // demo-mode runtime cssText (not part of the design system)
]

const EXT = /\.(tsx?|css)$/

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) yield* walk(p)
    else if (EXT.test(name)) yield p
  }
}

const rel = p => relative(ROOT, p)
const isAllowed = p => ALLOWLIST.some(a => rel(p).startsWith(a))

let failures = 0
let warnings = 0
for (const file of walk(ROOT)) {
  if (isAllowed(file)) continue
  const src = readFileSync(file, 'utf8')
  const lines = src.split('\n')

  lines.forEach((line, i) => {
    // Retired terms in code/testid/strings (comments included — they mislead).
    for (const term of RETIRED_TERMS) {
      if (line.includes(term)) {
        console.error(`[term] ${rel(file)}:${i + 1}: retired term "${term}" — use the unified term (see docs/design/ui-audit-2026-09 §6.1)`)
        failures++
      }
    }
    if (RAW_COLOR_CLASS.test(line)) {
      console.error(`[token] ${rel(file)}:${i + 1}: raw palette class "${line.match(RAW_COLOR_CLASS)[0]}" — use theme tokens (bg-surface-*, text-primary, …)`)
      failures++
    } else if (RAW_HEX.test(line) && !line.includes('--')) {
      console.error(`[token] ${rel(file)}:${i + 1}: hard-coded hex "${line.match(RAW_HEX)[0]}" — use CSS variables from @theme`)
      failures++
    }

    // ── Batch 1 (2026-09-29): duration / elevation / type-size bypasses ──
    if (DURATION_RAW.test(line)) {
      console.error(`[duration] ${rel(file)}:${i + 1}: "${line.match(DURATION_RAW)[0]}" — use duration-(--duration-fast|normal|slow|slower): 100→fast, 150-200→normal, 240-300→slow, >300→slower`)
      failures++
    }
    if (line.includes('shadow-')) {
      const sizes = []
      let hasColor = false
      for (const m of line.matchAll(SHADOW_TOKEN)) {
        const token = m[1].replace(/\/.*$/, '')
        if (SHADOW_SIZES.has(token)) sizes.push(m[0])
        else if (!SHADOW_ALLOWED.has(token)) hasColor = true
      }
      if (sizes.length > 0 && hasColor) {
        console.warn(`[shadow] ${rel(file)}:${i + 1}: raw "${sizes.join(' ')}" kept for shadow-color composition (${line.trim().slice(0, 90)}…) — allowed exception, do not add new ones`)
        warnings++
      } else if (sizes.length > 0) {
        console.error(`[shadow] ${rel(file)}:${i + 1}: raw elevation "${sizes.join(' ')}" — use shadow-e1..e5 (e1 card-rest, e2 card-hover, e3 dropdown, e4 drawer, e5 modal)`)
        failures++
      }
    }
    if (TEXT_PX.test(line)) {
      if (line.includes(ICON_MARKER)) {
        console.error(`[type] ${rel(file)}:${i + 1}: text-[Npx] on a material-symbols line — use the icon-* size utilities (icon-xs/sm/md/lg/xl/2xl)`)
        failures++
      } else {
        console.warn(`[type] ${rel(file)}:${i + 1}: non-token font size "${line.match(TEXT_PX)[0]}" — use a --text-* role (off-scale values need a deliberate decision)`)
        warnings++
      }
    }
  })
}

console.error(`\ndesign-token check: ${failures} error(s), ${warnings} warning(s)`)
if (failures > 0) process.exit(1)
console.log(warnings > 0
  ? `design-token check: OK (terms unified, tokens adopted) — ${warnings} documented warning(s) above`
  : 'design-token check: OK (terms unified, tokens adopted)')

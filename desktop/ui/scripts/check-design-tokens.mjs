#!/usr/bin/env node
// Design-system guardrails (UI audit 2026-09 acceptance criteria, §7 Wave 2):
//   1. Terminology consistency — the retired dual-track terms must never
//      reappear in component sources (i18n locale values are the single
//      source of truth).
//   2. Design-token adoption — raw Tailwind palette classes and hard-coded
//      hex colors are rejected in component sources; use MD3 theme tokens
//      (--color-*, bg-surface-*, text-on-*, semantic classes) instead.
// Exit 1 on any hit so CI can gate on it. Scope: desktop/ui/src only;
// tests may reference retired strings on purpose (they assert the change).

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

const ALLOWLIST = [
  '__tests__/',              // tests assert literal values on purpose
  'components/extensions/', // brand gradient buttons (documented exception, 06-26 audit)
  'theme/',                 // generated theme machinery
  'i18n/',                  // locale strings
  'index.css',              // the @theme token source — hex lives here by definition
  'components/terminal/',   // xterm.js API consumes raw hex palettes by contract
  'components/artifact/MermaidRenderer.tsx', // hex lives inside a standalone iframe document — parent vars cannot cross
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
  })
}

if (failures > 0) {
  console.error(`\ndesign-token check: ${failures} violation(s)`)
  process.exit(1)
}
console.log('design-token check: OK (terms unified, tokens adopted)')

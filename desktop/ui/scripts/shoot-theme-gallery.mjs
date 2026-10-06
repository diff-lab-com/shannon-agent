#!/usr/bin/env node
// One-shot re-shoot of the theme gallery (docs/design/ui-audit-2026-09/THEME-GALLERY.md).
//
// Reads the theme list from src/theme/generated/registry.ts (never hardcode it
// here), drives e2e/theme-gallery.spec.ts through Playwright to screenshot
// /chat per theme straight into the doc's screenshots/themes/ directory
// (chat-<theme>.png, overwriting the previous shots), then rewrites the doc:
// header prose preserved, table regenerated from the registry.
//
//   pnpm gallery:shoot                 # all registry themes
//   node scripts/shoot-theme-gallery.mjs ember tokyo-night   # subset

import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync, statSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const uiRoot = resolve(scriptDir, '..')
const repoRoot = resolve(uiRoot, '../..') // desktop/ui → repo root
const AUDIT_DIR = resolve(repoRoot, 'docs/design/ui-audit-2026-09')
const SHOTS_DIR = resolve(AUDIT_DIR, 'screenshots/themes')
const DOC_PATH = resolve(AUDIT_DIR, 'THEME-GALLERY.md')
const REGISTRY_PATH = resolve(uiRoot, 'src/theme/generated/registry.ts')
const THEME_CONTEXT_PATH = resolve(uiRoot, 'src/context/ThemeContext.tsx')

// --- Registry (source of truth) ---------------------------------------------

const registrySrc = readFileSync(REGISTRY_PATH, 'utf8')
const THEMES = [...registrySrc.matchAll(/\{\s*id:\s*'([\w-]+)',\s*mode:\s*'(light|dark)',?\s*\}/g)].map(
  m => ({ id: m[1], mode: m[2] }),
)
if (THEMES.length === 0) {
  console.error(`shoot-theme-gallery: no themes parsed from ${REGISTRY_PATH} — parser out of sync with generate-themes.mjs output`)
  process.exit(1)
}

// Default theme lives in ThemeContext's localStorage fallback (cosmetic doc
// marker only — warn and continue if the lookup breaks).
const defaultMatch = /\(\s*localStorage\.getItem\('shannon-theme'\)[^)]*\)\s*\|\|\s*'([\w-]+)'/.exec(
  readFileSync(THEME_CONTEXT_PATH, 'utf8'),
)
const DEFAULT_THEME = defaultMatch?.[1]
if (!DEFAULT_THEME) {
  console.warn('shoot-theme-gallery: could not detect the default theme in ThemeContext.tsx — doc will omit the (默认) marker')
}

// --- Theme selection ----------------------------------------------------------

const requested = process.argv.slice(2)
const themes = requested.length === 0 ? THEMES : THEMES.filter(t => requested.includes(t.id))
if (themes.length === 0) {
  console.error(
    `shoot-theme-gallery: unknown theme(s) ${requested.join(', ')} — registry has: ${THEMES.map(t => t.id).join(', ')}`,
  )
  process.exit(1)
}
console.log(`shoot-theme-gallery: shooting ${themes.length} theme(s): ${themes.map(t => t.id).join(', ')}`)

// --- Shoot ---------------------------------------------------------------------

const startedAt = Date.now()
const result = spawnSync(
  'pnpm',
  ['exec', 'playwright', 'test', 'e2e/theme-gallery.spec.ts', '--workers=1'],
  {
    cwd: uiRoot,
    stdio: 'inherit',
    env: {
      ...process.env,
      GALLERY_THEMES: themes.map(t => t.id).join(','),
      GALLERY_OUT_DIR: SHOTS_DIR,
    },
  },
)
if (result.status !== 0) {
  console.error(`shoot-theme-gallery: playwright exited with status ${result.status} — doc NOT regenerated`)
  process.exit(result.status ?? 1)
}

// Fail loudly if any theme's PNG is missing or wasn't rewritten this run.
const stale = themes.filter(t => {
  const file = resolve(SHOTS_DIR, `chat-${t.id}.png`)
  try {
    return statSync(file).mtimeMs < startedAt
  } catch {
    return true
  }
})
if (stale.length > 0) {
  console.error(`shoot-theme-gallery: missing or not updated: ${stale.map(t => `chat-${t.id}.png`).join(', ')}`)
  process.exit(1)
}
console.log(`shoot-theme-gallery: ${themes.length} screenshot(s) written to ${SHOTS_DIR}`)

// --- Regenerate THEME-GALLERY.md ------------------------------------------------

const date = new Date()
const pad = n => String(n).padStart(2, '0')
const today = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`
const link = id => `[\`${id}\`](./screenshots/themes/chat-${id}.png)`
const cell = (id, mode) => {
  if (!id) return '—'
  const marker = mode && id === DEFAULT_THEME ? '（**默认**）' : ''
  return `${link(id)}${marker}`
}
const dark = themes.filter(t => t.mode === 'dark')
const light = themes.filter(t => t.mode === 'light')
const rows = Math.max(dark.length, light.length)
const table = [
  '| 深色主题 | 浅色主题 |',
  '|---|---|',
  ...Array.from({ length: rows }, (_, i) => `| ${cell(dark[i]?.id, 'dark')} | ${cell(light[i]?.id, 'light')} |`),
]

// Header prose and the materials note keep the hand-written doc's structure;
// only the title count and the table are derived from the registry.
const doc = [
  `# ${THEMES.length} 主题对照表（chat 页实拍）`,
  '',
  '> 生成方式：`pnpm demo` + Playwright 逐主题设置 `localStorage.shannon-theme` 后截图。',
  '> 每张图均为 /chat 页 1280×800 实拍（玻璃 composer + 侧栏 + 消息流可见，界面语言固定 zh-CN）。',
  '> 位置：[screenshots/themes/](./screenshots/themes/)',
  '',
  ...table,
  '',
  '材料说明：全部主题共享同一 Liquid Glass token 公式（`--glass-tint-alpha` 按 mode 0.62/0.48），仅 base 色随主题变化——玻璃质感与可读性由 CI 的 contrast-audit（AA）与 token 门禁共同保证。',
  '',
  `由 \`pnpm gallery:shoot\` 生成于 ${today}。`,
  '',
].join('\n')
writeFileSync(DOC_PATH, doc)
console.log(`shoot-theme-gallery: regenerated ${DOC_PATH} (${dark.length} dark / ${light.length} light)`)

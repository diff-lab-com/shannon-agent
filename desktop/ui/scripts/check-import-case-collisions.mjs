#!/usr/bin/env node
// Import case-collision guard (macOS QA 2026-09-30):
//   A relative/alias import WITHOUT an explicit extension (e.g. `./chat`,
//   `@/pages/chat`) is ambiguous when a same-named file and directory
//   co-exist in one directory (`Chat.tsx` + `chat/`). TypeScript/Vite probe
//   the FILE first, so on Linux (case-sensitive) the specifier resolves to
//   `chat/index.ts`, while on macOS/Windows (case-insensitive filesystems)
//   it resolves to `Chat.tsx` itself — a self-import that breaks `tsc`,
//   `vite build` and vitest only on those platforms (invisible to a
//   Linux-only CI). This happened for `pages/Editor.tsx` + `pages/editor/`
//   and `pages/Chat.tsx` + `pages/chat/`.
//
// The guard rejects exactly that ambiguity: an extension-less specifier
// whose final segment matches BOTH a file (any supported module extension,
// case-insensitively) and a directory in the same directory. Exact-case
// file hits (`./pages/Chat` → Chat.tsx) are fine and not reported.
//
// Run: node scripts/check-import-case-collisions.mjs [--report-only]
// (wired into `pnpm lint`).

import { readdirSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const SRC = join(root, 'src')
const MODULE_EXTS = ['.ts', '.tsx', '.js', '.jsx', '.d.ts']
const SKIP_DIR_NAMES = new Set(['node_modules', 'dist', 'coverage', 'playwright-report', 'test-results'])

const files = []
const walk = d => {
  for (const e of readdirSync(d, { withFileTypes: true })) {
    if (e.isDirectory()) {
      if (!SKIP_DIR_NAMES.has(e.name)) walk(join(d, e.name))
    } else if (MODULE_EXTS.some(x => e.name.endsWith(x)) && !e.name.endsWith('.d.ts')) {
      files.push(join(d, e.name))
    }
  }
}
walk(SRC)

const collisions = []
const lc = s => s.toLowerCase()

for (const file of files) {
  const src = readFileSync(file, 'utf8')
  const re = /(?:from\s+|import\s*\(\s*|import\s+|require\s*\(\s*)["']([^"']+)["']/g
  let m
  while ((m = re.exec(src))) {
    const spec = m[1]
    if (!spec.startsWith('./') && !spec.startsWith('../') && !spec.startsWith('@')) continue
    // explicit extension → no probing, no ambiguity
    if (/\.(ts|tsx|js|jsx|json|css|svg|png|jpg|webp|woff2?)$/i.test(spec)) continue

    let dir
    let base
    if (spec.startsWith('@/')) {
      const rest = spec.slice(2)
      dir = join(SRC, dirname(rest))
      base = rest.split('/').pop()
    } else {
      dir = resolve(dirname(file), dirname(spec))
      base = spec.split('/').pop()
    }
    if (!base) continue // directory-level import of a path with trailing slash

    let entries
    try {
      entries = readdirSync(dir)
    } catch {
      continue
    }
    const probeFile = matcher => entries.find(e => {
      try { return matcher(e) && statSync(join(dir, e)).isFile() } catch { return false }
    })
    const fileExact = probeFile(e => MODULE_EXTS.some(x => e === base + x))
    const fileCaseInsensitive = fileExact ?? probeFile(e => MODULE_EXTS.some(x => lc(e) === lc(base) + x))
    const dirCaseInsensitive = entries.find(e => {
      try { return lc(e) === lc(base) && statSync(join(dir, e)).isDirectory() } catch { return false }
    })
    // Divergent resolution: on Linux the file probe misses (no exact-case
    // S+ext) so the specifier lands in the directory, while on macOS/Windows
    // the case-insensitive file probe wins and (self-)imports the file.
    // Exact-case file hits (`./pages/Chat` → Chat.tsx) resolve to the same
    // file everywhere and are fine.
    if (!fileExact && fileCaseInsensitive && dirCaseInsensitive) {
      collisions.push(
        `${relative(file)}\n    "${spec}" matches BOTH ${fileCaseInsensitive} and ${dirCaseInsensitive}/ in ${relative(dir)}/ — ` +
        `resolves differently on case-sensitive (Linux CI) vs case-insensitive (macOS/Windows) filesystems. ` +
        `Rename one of the two, or make the specifier unambiguous (explicit path/index).`,
      )
    }
  }
}

function relative(p) {
  return p.startsWith(root) ? p.slice(root.length + 1) : p
}

if (collisions.length) {
  console.error(`import case-collisions: ${collisions.length}\n\n${collisions.join('\n')}\n`)
  process.exit(1)
}
console.log(`import case-collisions: none (${files.length} files scanned)`)

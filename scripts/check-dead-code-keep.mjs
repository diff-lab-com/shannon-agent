#!/usr/bin/env node
// C5 — fast local guard for the CI invariant (architecture_invariants
// `dead_code_allow_keep_markers`): flag bare `#[allow(dead_code)]` lines
// ADDED by the working tree / current branch vs a base ref (default
// `ghmeta/dev`; pass any ref as argv[2]). Runs without compiling the
// workspace, so it is usable as a pre-commit/IDE check.
//
// Usage:  node scripts/check-dead-code-keep.mjs [base-ref]
import { execSync } from 'node:child_process'

const base = process.argv[2] ?? 'ghmeta/dev'
const cmd = `git diff --unified=0 ${base} -- crates desktop/src`
const diff = execSync(cmd, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 })

const violations = []
let currentFile = ''
for (const line of diff.split('\n')) {
  if (line.startsWith('+++ b/')) {
    currentFile = line.slice(6)
    continue
  }
  if (!line.startsWith('+') || line.startsWith('+++')) continue
  const body = line.slice(1)
  if (body.includes('#[allow(dead_code)]') && !body.includes('KEEP:')) {
    violations.push(`${currentFile}: ${body.trim().slice(0, 120)}`)
  }
}

if (violations.length > 0) {
  console.error('bare allow(dead_code) added — add // KEEP: or extend the invariant baseline:')
  for (const v of violations) console.error('  -', v)
  process.exit(1)
}
console.log('dead-code-keep guard: OK (no new bare allow(dead_code) vs', base + ')')

// Shared machinery for the Batch-1 (UI review 2026-09-29) token codemods.
//
// One-off migration tools: each codemod in this directory rewrites bypassed
// utility classes in desktop/ui/src to their design-token equivalents. They
// are kept in the repo as the traceable record of that migration (and so the
// mapping tables can be re-checked); they are NOT part of the build.
//
// Usage: node scripts/codemods/<name>.mjs [--write]   (dry run by default)
//
// Class-string detection is AST-based (TypeScript): a string is only touched
// inside a "class container" — a className-ish JSX attribute (className,
// *ClassName, …) or the arguments of cn/clsx/cva/cx/twMerge/tv/toHaveClass.
// Prose, comments, testids, and non-class strings are never rewritten.

import ts from 'typescript'
import { readFileSync, writeFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative } from 'node:path'

export const SRC = new URL('../../src', import.meta.url).pathname

const EXT = /\.(tsx?|css)$/

export function listSrcFiles({ includeCss = false } = {}) {
  const out = []
  const exts = includeCss ? /\.(tsx?|css)$/ : /\.tsx?$/
  ;(function walk(dir) {
    for (const name of readdirSync(dir)) {
      if (name === 'node_modules') continue
      const p = join(dir, name)
      const st = statSync(p)
      if (st.isDirectory()) walk(p)
      else if (exts.test(name)) out.push(p)
    }
  })(SRC)
  return out.sort()
}

const CLASS_FUNCS = new Set(['cn', 'clsx', 'cva', 'cx', 'twMerge', 'tv', 'toHaveClass'])

function mergeRanges(ranges) {
  const sorted = [...ranges].sort((a, b) => a.start - b.start || b.end - a.end)
  const out = []
  for (const r of sorted) {
    const last = out[out.length - 1]
    if (last && r.start < last.end) {
      if (r.end > last.end) last.end = r.end
    } else {
      out.push({ ...r })
    }
  }
  return out
}

/**
 * Ranges in `src` that hold class strings (outermost wins on nesting).
 */
export function classContainers(src) {
  const sf = ts.createSourceFile('file.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const ranges = []
  const isClassAttrName = name => name === 'className' || /([a-z])ClassName$/.test(name) || /Class$/.test(name)
  function visit(node) {
    if (ts.isJsxAttribute(node) && isClassAttrName(node.name.getText(sf)) && node.initializer) {
      ranges.push({ start: node.initializer.getStart(sf), end: node.initializer.getEnd() })
      ts.forEachChild(node, visit)
      return
    }
    if (ts.isCallExpression(node) || ts.isTaggedTemplateExpression(node)) {
      let name
      if (ts.isCallExpression(node)) {
        const callee = node.expression
        if (ts.isIdentifier(callee)) name = callee.text
        else if (ts.isPropertyAccessExpression(callee)) name = callee.name.text
      } else if (ts.isIdentifier(node.tag)) {
        name = node.tag.text
      }
      if (name !== undefined && CLASS_FUNCS.has(name)) {
        ranges.push({ start: node.getStart(sf), end: node.getEnd() })
        ts.forEachChild(node, visit)
        return
      }
    }
    ts.forEachChild(node, visit)
  }
  ts.forEachChild(sf, visit)
  return { containers: mergeRanges(ranges), sourceFile: sf }
}

/**
 * Ranges of JSX elements whose tag mentions `marker` (e.g.
 * material-symbols-outlined) — lets us rewrite conditional/template
 * className chunks that belong to an icon element even when the marker
 * lives in a sibling string literal.
 */
export function jsxElementsContaining(src, marker) {
  const sf = ts.createSourceFile('file.tsx', src, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
  const ranges = []
  function visit(node) {
    if (ts.isJsxElement(node) || ts.isJsxSelfClosingElement(node)) {
      const start = node.getStart(sf)
      // Only the opening tag can carry the class — scan up to the end of
      // the attributes so a marker inside the element's children does not
      // widen the range for unrelated className edits.
      const end = ts.isJsxSelfClosingElement(node) ? node.getEnd() : node.openingElement.getEnd()
      if (src.slice(start, end).includes(marker)) ranges.push({ start, end })
    }
    ts.forEachChild(node, visit)
  }
  ts.forEachChild(sf, visit)
  return ranges
}

/** Apply non-overlapping {start,end,replacement} edits to `src`. */
export function applyEdits(src, edits) {
  const sorted = [...edits].sort((a, b) => b.start - a.start)
  let out = src
  for (const e of sorted) out = out.slice(0, e.start) + e.replacement + out.slice(e.end)
  return out
}

/** Regex replacements restricted to the given ranges. Returns edits. */
export function replaceInRanges(src, ranges, regex, makeReplacement) {
  const edits = []
  for (const r of ranges) {
    const chunk = src.slice(r.start, r.end)
    for (const m of chunk.matchAll(regex)) {
      const rep = makeReplacement(m)
      if (rep === undefined || rep === m[0]) continue
      edits.push({ start: r.start + m.index, end: r.start + m.index + m[0].length, replacement: rep })
    }
  }
  return edits
}

export function relPath(p) {
  return relative(SRC, p)
}

export function finish({ changedFiles, edits, label, dryRun, leftovers = [] }) {
  const total = Object.values(edits).reduce((a, b) => a + b, 0)
  console.log(`\n${label}: ${total} replacement(s) in ${changedFiles} file(s)${dryRun ? ' [dry run — pass --write to apply]' : ''}`)
  for (const [k, v] of Object.entries(edits).sort((a, b) => b[1] - a[1])) console.log(`  ${k}: ${v}`)
  if (leftovers.length) {
    console.log(`  leftovers needing manual attention (${leftovers.length}):`)
    for (const l of leftovers) console.log(`    ${l}`)
  }
}

export function readWrite(p, fn, dryRun) {
  const src = readFileSync(p, 'utf8')
  const next = fn(src)
  if (next !== src && !dryRun) writeFileSync(p, next)
  return next !== src
}

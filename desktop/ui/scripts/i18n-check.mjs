#!/usr/bin/env node
// i18n-check.mjs — B6-35 CI gate (review doc 2026-09-26 §7 item 35, decision 5).
//
// Compares the `en` baseline key set against every other locale file in
// src/i18n/locales and fails with a missing-key list when any locale has
// drifted. This is the guard against a regression of P1-5 (locales missing
// whole batches of keys; before the B1-14 provider-level en merge those
// rendered as raw message ids).
//
// Guarantees:
//   1. Every locale defines every `en` key (missing keys are the failure).
//   2. No locale invents keys that `en` doesn't define (stray keys reported,
//      also a failure — they are dead weight the runtime can never show).
//
// Usage:
//   node scripts/i18n-check.mjs                    # real locales, exit 0/1
//   node scripts/i18n-check.mjs --dir <localesDir> # alternate dir (tests)
//   node scripts/i18n-check.mjs --report           # copy-rate report, exit 0
//
// `--report` is the B6b progress-tracking tool for decision 5 (tiered
// translation): it prints, per locale, how many values are still identical
// to the `en` baseline (the "en copy rate"). It is a pure report — it never
// evaluates key drift and always exits 0 (only a missing en.json baseline is
// an input error). The lint chain keeps running the gate mode unchanged.
//
// Scope note: values are NOT compared in gate mode — locales legitimately
// carry en text as the in-place fallback (decision 5: fill by frequency of
// use). Only the key SET is gated there; --report reads the values purely to
// surface how much tiered translation work remains.

import fs from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

// fileURLToPath, not `.pathname`: the URL form yields `/C:/…` on Windows,
// which path.resolve then anchors into a bogus `C:C:…` directory.
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url))

function parseArgs(argv) {
  const args = { dir: null, report: false }
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--dir') {
      args.dir = argv[i + 1]
      i++
    } else if (argv[i] === '--report') {
      args.report = true
    }
  }
  return args
}

/** Pure core, exported for tests: returns { ok, missing: {locale: [key]},
 *  extra: {locale: [key]} } for a map of locale name → parsed messages. */
export function checkKeySets(messages) {
  const base = messages.en
  if (!base) return { ok: false, missing: {}, extra: {}, error: 'no `en` baseline found' }
  const baseKeys = new Set(Object.keys(base))
  const missing = {}
  const extra = {}
  for (const [locale, data] of Object.entries(messages)) {
    if (locale === 'en') continue
    const keys = new Set(Object.keys(data))
    const miss = [...baseKeys].filter(k => !keys.has(k))
    const ext = [...keys].filter(k => !baseKeys.has(k))
    if (miss.length > 0) missing[locale] = miss
    if (ext.length > 0) extra[locale] = ext
  }
  const ok = Object.keys(missing).length === 0 && Object.keys(extra).length === 0
  return { ok, missing, extra }
}

/**
 * Pure core for `--report`, exported for tests: per-locale en-copy-rate rows
 * (decision 5 progress tracking — not a gate). `total` is the en baseline
 * size; `same` counts keys whose value is strictly identical to en's (the
 * in-place fallback). Rows are sorted by copy rate descending (most en
 * copies — most remaining translation work — first, ties by locale name).
 */
export function copyRates(messages) {
  const base = messages.en
  if (!base) return { error: 'no `en` baseline found', rows: [] }
  const baseEntries = Object.entries(base)
  const total = baseEntries.length
  const rows = []
  for (const [locale, data] of Object.entries(messages)) {
    if (locale === 'en') continue
    let same = 0
    for (const [key, value] of baseEntries) {
      if (key in data && data[key] === value) same++
    }
    const rate = total === 0 ? 0 : Math.round((same / total) * 1000) / 10
    rows.push({ locale, total, same, rate })
  }
  rows.sort((a, b) => b.rate - a.rate || a.locale.localeCompare(b.locale))
  return { error: null, rows }
}

// The fixed explanatory line required at the top of every --report output:
// en copies are the sanctioned in-place fallback (provider-level en merge
// since B1-14), not a bug, and translation proceeds tier-by-tier (decision 5).
const REPORT_NOTE =
  '说明：这些键经 B1-14 的 provider 层 en 兜底，运行时显示英文，功能无损；按决策 5 分档推进人工翻译（防止后来者误判为 bug）。'

function printReport(messages) {
  const { error, rows } = copyRates(messages)
  if (error) {
    console.error(`i18n-report: ${error}`)
    return 1
  }
  const width = Math.max('locale', ...rows.map(r => r.locale.length)).length
  console.log(`i18n-report: en-copy rate per locale (sorted by copy rate, highest first)`)
  console.log(REPORT_NOTE)
  console.log(`${'locale'.padEnd(width)}  same/total  copy-rate`)
  for (const { locale, total, same, rate } of rows) {
    console.log(`${locale.padEnd(width)}  ${`${same}/${total}`.padEnd(10)}  ${rate.toFixed(1)}%`)
  }
  console.log('i18n-report: pure progress report — never a gate (always exit 0).')
  return 0
}

export function main(argv = process.argv.slice(2)) {
  const { dir, report } = parseArgs(argv)
  const localesDir = dir ?? path.resolve(SCRIPT_DIR, '../src/i18n/locales')

  const files = fs.readdirSync(localesDir).filter(f => f.endsWith('.json')).sort()
  if (!files.includes('en.json')) {
    console.error(`i18n-check: no en.json in ${localesDir}`)
    return 1
  }

  const messages = {}
  for (const file of files) {
    messages[path.basename(file, '.json')] = JSON.parse(fs.readFileSync(path.join(localesDir, file), 'utf8'))
  }

  // Report mode never evaluates key drift and never gates (always exit 0).
  if (report) return printReport(messages)

  const { ok, missing, extra, error } = checkKeySets(messages)
  const baseCount = Object.keys(messages.en).length
  const localeNames = files.map(f => path.basename(f, '.json'))

  if (error) {
    console.error(`i18n-check: ${error}`)
    return 1
  }

  console.log(`i18n-check: en baseline ${baseCount} keys vs ${localeNames.length - 1} locales (${localeNames.join(', ')})`)

  let failures = 0
  for (const [locale, keys] of Object.entries(missing)) {
    console.error(`  [missing] ${locale}.json lacks ${keys.length} key(s):`)
    keys.forEach(k => console.error(`    - ${k} = ${JSON.stringify(messages.en[k])}`))
    failures += keys.length
  }
  for (const [locale, keys] of Object.entries(extra)) {
    console.error(`  [stray] ${locale}.json defines ${keys.length} key(s) absent from en:`)
    keys.forEach(k => console.error(`    + ${k}`))
    failures += keys.length
  }

  if (failures > 0) {
    console.error(`\ni18n-check: ${failures} key drift violation(s) — add the missing keys or remove the strays.`)
    return 1
  }
  console.log('i18n-check: OK (every locale matches the en key set)')
  return 0
}

// Run as CLI only when executed directly (imported by tests for checkKeySets).
const isMain = process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href
if (isMain) {
  process.exit(main())
}

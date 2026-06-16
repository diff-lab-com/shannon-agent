#!/usr/bin/env node
// i18n-coverage.mjs — Scan ui/src for components and report i18n migration status.
//
// Usage:
//   node scripts/i18n-coverage.mjs              # summary
//   node scripts/i18n-coverage.mjs --unmigrated # list only unmigrated files
//   node scripts/i18n-coverage.mjs --parity     # check en/zh-CN key parity
//
// A component is "migrated" if it imports useIntl OR calls intl.formatMessage.
// "Unmigrated" counts hardcoded English strings in JSX text, placeholder,
// aria-label, and title attributes.

import fs from 'node:fs'
import path from 'node:path'
import { execSync } from 'node:child_process'

const ROOT = path.resolve(path.dirname(new URL(import.meta.url).pathname), '..')
const SRC = path.join(ROOT, 'src')

function listTsxaFiles(dir) {
  const out = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name)
    if (entry.isDirectory()) out.push(...listTsxaFiles(full))
    else if (entry.name.endsWith('.tsx') || entry.name.endsWith('.ts')) out.push(full)
  }
  return out
}

function scanFile(file) {
  const text = fs.readFileSync(file, 'utf8')
  const migrated = /\buseIntl\b|intl\.formatMessage|useIntlContext\b/.test(text)
  if (migrated) return { migrated: true, stringCount: 0 }
  const jsxText = (text.match(/>[A-Z][a-z][a-zA-Z ]+</g) || []).length
  const attrs = (text.match(/\b(placeholder|aria-label|title|label)="[A-Z][a-zA-Z ][^"]*"/g) || []).length
  return { migrated: false, stringCount: jsxText + attrs }
}

function cmdReport() {
  const files = listTsxaFiles(SRC)
  const reports = files.map(f => ({ file: path.relative(ROOT, f), ...scanFile(f) }))
  const migrated = reports.filter(r => r.migrated)
  const unmigrated = reports.filter(r => !r.migrated && r.stringCount > 0)
  const untouched = reports.filter(r => !r.migrated && r.stringCount === 0)
  console.log('=== i18n Coverage ===')
  console.log(`Total .tsx/.ts files: ${reports.length}`)
  console.log(`Migrated:            ${migrated.length}`)
  console.log(`Unmigrated (w/text): ${unmigrated.length} (${unmigrated.reduce((s, r) => s + r.stringCount, 0)} strings)`)
  console.log(`No strings:          ${untouched.length}`)
  console.log('')
  console.log('=== Unmigrated by directory ===')
  const byDir = {}
  for (const r of unmigrated) {
    const dir = path.dirname(r.file).replace(/^src\/components\/?/, '') || '(root)'
    byDir[dir] = (byDir[dir] || 0) + 1
  }
  Object.entries(byDir).sort((a, b) => b[1] - a[1]).forEach(([d, n]) => console.log(`  ${n.toString().padStart(3)}  ${d}`))
}

function cmdUnmigrated() {
  const files = listTsxaFiles(SRC)
  const reports = files.map(f => ({ file: path.relative(ROOT, f), ...scanFile(f) }))
  reports
    .filter(r => !r.migrated && r.stringCount > 0)
    .sort((a, b) => b.stringCount - a.stringCount)
    .forEach(r => console.log(`${r.stringCount.toString().padStart(4)}  ${r.file}`))
}

function cmdParity() {
  const en = JSON.parse(fs.readFileSync(path.join(SRC, 'i18n/locales/en.json'), 'utf8'))
  const zh = JSON.parse(fs.readFileSync(path.join(SRC, 'i18n/locales/zh-CN.json'), 'utf8'))
  const enKeys = new Set(Object.keys(en))
  const zhKeys = new Set(Object.keys(zh))
  const missingInZh = [...enKeys].filter(k => !zhKeys.has(k))
  const missingInEn = [...zhKeys].filter(k => !enKeys.has(k))
  console.log(`en keys: ${enKeys.size}, zh-CN keys: ${zhKeys.size}`)
  console.log(`Missing in zh-CN: ${missingInZh.length}`)
  missingInZh.forEach(k => console.log(`  - ${k} = ${JSON.stringify(en[k])}`))
  console.log(`Missing in en:    ${missingInEn.length}`)
  missingInEn.forEach(k => console.log(`  - ${k} = ${JSON.stringify(zh[k])}`))
  if (missingInZh.length === 0 && missingInEn.length === 0) console.log('Parity OK')
}

const cmd = process.argv[2] || '--summary'
if (cmd === '--unmigrated') cmdUnmigrated()
else if (cmd === '--parity') cmdParity()
else cmdReport()

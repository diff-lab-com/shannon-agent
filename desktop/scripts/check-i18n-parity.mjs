#!/usr/bin/env node
// Verifies en.json and zh-CN.json have identical key sets.
// Exits 0 on parity, 1 on mismatch. No dependencies — pure node.
//
// Usage:
//   node scripts/check-i18n-parity.mjs
//
// CI job: .gitea/workflows/ci.yml :: i18n-parity

import { readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const localesDir = resolve(__dirname, '../ui/src/i18n/locales')

function loadKeys(locale) {
  const raw = readFileSync(resolve(localesDir, `${locale}.json`), 'utf8')
  return JSON.parse(raw)
}

function flattenKeys(obj, prefix = '') {
  const out = []
  for (const [k, v] of Object.entries(obj)) {
    const path = prefix ? `${prefix}.${k}` : k
    if (v !== null && typeof v === 'object' && !Array.isArray(v)) {
      out.push(...flattenKeys(v, path))
    } else {
      out.push(path)
    }
  }
  return out
}

const en = new Set(flattenKeys(loadKeys('en')))
const zhCN = new Set(flattenKeys(loadKeys('zh-CN')))

const missingInZh = [...en].filter(k => !zhCN.has(k)).sort()
const missingInEn = [...zhCN].filter(k => !en.has(k)).sort()

if (missingInZh.length === 0 && missingInEn.length === 0) {
  console.log(`i18n key parity OK (${en.size} keys)`)
  process.exit(0)
}

console.error('i18n key parity mismatch')
console.error('')
if (missingInZh.length > 0) {
  console.error(`  Missing in zh-CN.json (${missingInZh.length}):`)
  for (const k of missingInZh.slice(0, 10)) console.error(`    - ${k}`)
  if (missingInZh.length > 10) console.error(`    ... and ${missingInZh.length - 10} more`)
  console.error('')
}
if (missingInEn.length > 0) {
  console.error(`  Missing in en.json (${missingInEn.length}):`)
  for (const k of missingInEn.slice(0, 10)) console.error(`    - ${k}`)
  if (missingInEn.length > 10) console.error(`    ... and ${missingInEn.length - 10} more`)
  console.error('')
}
console.error('Fix: add the missing keys to the corresponding locale file.')
process.exit(1)

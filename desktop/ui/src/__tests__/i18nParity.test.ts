// Review §P2-18 hygiene batch — i18n completeness gate.
//
// Two guarantees, so hardcoded-English bypasses and en/zh drift can't
// regress:
//   1. Every literal message key used in `src/` (via useT()/t(), intl.
//      formatMessage({ id }), <FormattedMessage id>, messageFor()) is
//      defined in BOTH en.json and zh-CN.json.
//   2. en.json and zh-CN.json define exactly the same key set with
//      non-empty values (en is the canonical fallback; zh-CN must not
//      drift).
//
// Limitation (documented): only *literal* ids are statically auditable —
// keys built dynamically (e.g. `id: someVar`) are invisible to the scan.
// Comments are stripped first so doc-comment examples don't count as
// usage. Other locales (ja/es/…) intentionally copy en as fallback and
// are translated incrementally; they are out of scope here.

import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import en from '@/i18n/locales/en.json'
import zh from '@/i18n/locales/zh-CN.json'

// Vitest's root is desktop/ui (package dir), so src/ is <cwd>/src.
const SRC_ROOT = existsSync(resolve(process.cwd(), 'src'))
  ? resolve(process.cwd(), 'src')
  : resolve(process.cwd(), 'desktop/ui/src')

const KEY_PATTERNS = [
  /\bt\(\s*['"]([\w.]+)['"]/g,          // const t = useT(); t('a.b')
  /formatMessage\(\{\s*id:\s*['"]([\w.]+)['"]/g, // intl.formatMessage({ id: 'a.b'
  /<FormattedMessage\s+id=['"]([\w.]+)['"]/g,     // <FormattedMessage id="a.b"
  /\bmessageFor\(\s*['"]([\w.]+)['"]/g, // messageFor('a.b')
]

/** Strip block comments and line comments (URL `//` after a scheme is kept). */
function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .split('\n')
    .map(l => l.replace(/(?<!:)\/\/.*$/, ''))
    .join('\n')
}

function* walk(dir: string): Generator<string> {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name)
    const st = statSync(p)
    if (st.isDirectory()) {
      // Tests assert literal ids on purpose; locales hold values, not calls.
      if (name === '__tests__' || name === 'locales' || name === 'node_modules') continue
      yield* walk(p)
    } else if (/\.(tsx?)$/.test(name)) {
      yield p
    }
  }
}

function usedKeys(): Map<string, string[]> {
  const used = new Map<string, string[]>()
  for (const file of walk(SRC_ROOT)) {
    const src = stripComments(readFileSync(file, 'utf8'))
    for (const pat of KEY_PATTERNS) {
      for (const match of src.matchAll(pat)) {
        const key = match[1]
        const rel = relative(SRC_ROOT, file)
        used.set(key, [...(used.get(key) ?? []), rel])
      }
    }
  }
  return used
}

describe('i18n completeness (en ↔ zh-CN parity + used-key coverage)', () => {
  const used = usedKeys()

  it('finds a meaningful number of used keys (scanner sanity)', () => {
    expect(used.size).toBeGreaterThan(1000)
  })

  it('defines every used key in en.json', () => {
    const missing = [...used.keys()].filter(k => !(k in en))
    expect(missing).toEqual([])
  })

  it('defines every used key in zh-CN.json', () => {
    const missing = [...used.keys()].filter(k => !(k in zh))
    expect(missing).toEqual([])
  })

  it('en.json and zh-CN.json define identical key sets', () => {
    const enKeys = new Set(Object.keys(en))
    const zhKeys = new Set(Object.keys(zh))
    const onlyEn = [...enKeys].filter(k => !zhKeys.has(k))
    const onlyZh = [...zhKeys].filter(k => !enKeys.has(k))
    expect(onlyEn).toEqual([])
    expect(onlyZh).toEqual([])
  })

  it('leaves no empty values in either locale', () => {
    for (const [locale, messages] of [['en', en], ['zh-CN', zh]] as const) {
      const empty = Object.entries(messages)
        .filter(([, value]) => String(value).trim().length === 0)
        .map(([key]) => `${locale}:${key}`)
      expect(empty).toEqual([])
    }
  })
})

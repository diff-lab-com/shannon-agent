// B6-35 — self-verification for the scripts/i18n-check.mjs CI gate.
//
// Two layers:
//   1. Pure core (`checkKeySets`) — missing-key and stray-key detection on
//      synthetic message maps, including the "deliberately delete a key"
//      red path the review asked for.
//   2. CLI contract — spawn `node scripts/i18n-check.mjs` against the real
//      locales (must exit 0) and against a temp copy with one en key removed
//      from a locale (must exit 1 and name the key).

import { describe, expect, it } from 'vitest'
import { execFileSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync, writeFileSync, copyFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { checkKeySets } from '../../scripts/i18n-check.mjs'

const SCRIPT = resolve(process.cwd(), 'scripts/i18n-check.mjs')
const REAL_LOCALES = resolve(process.cwd(), 'src/i18n/locales')

describe('i18n-check core', () => {
  it('green when every locale matches the en key set', () => {
    const { ok, missing, extra } = checkKeySets({
      en: { 'a.b': 'A', 'c.d': 'C' },
      ja: { 'a.b': 'エー', 'c.d': 'シー' },
    })
    expect(ok).toBe(true)
    expect(missing).toEqual({})
    expect(extra).toEqual({})
  })

  it('red when a locale is missing a key (deliberately deleted key must fail)', () => {
    const en = { 'a.b': 'A', 'c.d': 'C' }
    const ja = { ...en }
    delete ja['c.d'] // ← the deliberate key deletion
    const { ok, missing, extra } = checkKeySets({ en, ja })
    expect(ok).toBe(false)
    expect(missing.ja).toEqual(['c.d'])
    expect(extra).toEqual({})
  })

  it('red when a locale invents a key en does not define', () => {
    const { ok, missing, extra } = checkKeySets({
      en: { 'a.b': 'A' },
      de: { 'a.b': 'A', 'zz.bogus': 'X' },
    })
    expect(ok).toBe(false)
    expect(missing).toEqual({})
    expect(extra.de).toEqual(['zz.bogus'])
  })

  it('red when the en baseline itself is absent', () => {
    const { ok, error } = checkKeySets({ ja: { 'a.b': 'x' } })
    expect(ok).toBe(false)
    expect(error).toMatch(/no `en` baseline/)
  })
})

describe('i18n-check CLI', () => {
  it('exits 0 against the real locale files', () => {
    const out = execFileSync(process.execPath, [SCRIPT], { encoding: 'utf8' })
    expect(out).toMatch(/i18n-check: OK/)
  })

  it('exits 1 and names the key when a locale lost one (red path)', () => {
    const tmp = mkdtempSync(join(tmpdir(), 'i18n-check-'))
    try {
      for (const f of ['en.json', 'zh-CN.json']) copyFileSync(join(REAL_LOCALES, f), join(tmp, f))
      const de = JSON.parse(readFileSync(join(REAL_LOCALES, 'de.json'), 'utf8'))
      delete de['common.retry']
      writeFileSync(join(tmp, 'de.json'), JSON.stringify(de, null, 2))

      let threw: unknown
      try {
        execFileSync(process.execPath, [SCRIPT, '--dir', tmp], { encoding: 'utf8', stdio: 'pipe' })
      } catch (e) {
        threw = e
      }
      expect(threw).toBeDefined()
      const err = (threw as { stderr: string }).stderr
      expect(err).toContain('[missing] de.json')
      expect(err).toContain('- common.retry')
    } finally {
      rmSync(tmp, { recursive: true, force: true })
    }
  })
})

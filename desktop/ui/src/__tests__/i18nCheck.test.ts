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
import { checkKeySets, copyRates } from '../../scripts/i18n-check.mjs'

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

// B6b — the `--report` copy-rate core (decision 5 progress tracking; never a
// gate). Three canonical cases: zero copies, all copies, mixed — plus the
// sort contract (copy rate descending, ties by locale name).

describe('i18n-report core (copyRates)', () => {
  it('zero copies: every value differs from en → rate 0', () => {
    const { error, rows } = copyRates({
      en: { 'a.b': 'Hello', 'c.d': 'Save' },
      ja: { 'a.b': 'こんにちは', 'c.d': '保存' },
      de: { 'a.b': 'Hallo', 'c.d': 'Speichern' },
    })
    expect(error).toBeNull()
    expect(rows).toHaveLength(2)
    for (const row of rows) {
      expect(row.total).toBe(2)
      expect(row.same).toBe(0)
      expect(row.rate).toBe(0)
    }
  })

  it('all copies: every value identical to en → rate 100', () => {
    const en = { 'a.b': 'Hello', 'c.d': 'Save', 'e.f': 'Cancel' }
    const { error, rows } = copyRates({ en, fr: { ...en }, 'pt-BR': { ...en } })
    expect(error).toBeNull()
    for (const row of rows) {
      expect(row.total).toBe(3)
      expect(row.same).toBe(3)
      expect(row.rate).toBe(100)
    }
  })

  it('mixed: exact same/total/rate counts and descending sort (ties by locale name)', () => {
    const en = { 'a.b': 'Hello', 'c.d': 'Save', 'e.f': 'Cancel', 'g.h': 'Open' }
    const { error, rows } = copyRates({
      en,
      // fr: 3/4 identical → 75%
      fr: { 'a.b': 'Hello', 'c.d': 'Save', 'e.f': 'Cancel', 'g.h': 'Ouvrir' },
      // de: 1/4 identical → 25%
      de: { 'a.b': 'Hallo', 'c.d': 'Speichern', 'e.f': 'Cancel', 'g.h': 'Öffnen' },
      // es: 2/4 identical → 50%
      es: { 'a.b': 'Hello', 'c.d': 'Save', 'e.f': 'Cancelar', 'g.h': 'Abrir' },
      // ko: 3/4 identical → 75% (tie with fr → name ascending)
      ko: { 'a.b': 'Hello', 'c.d': 'Save', 'e.f': '취소', 'g.h': 'Open' },
    })
    expect(error).toBeNull()
    expect(rows.map(r => r.locale)).toEqual(['fr', 'ko', 'es', 'de'])
    expect(rows.map(r => r.rate)).toEqual([75, 75, 50, 25])
    expect(rows.find(r => r.locale === 'es')).toMatchObject({ total: 4, same: 2, rate: 50 })
  })

  it('missing keys count as untranslated (not en copies) and an empty en baseline never divides by zero', () => {
    const absent = copyRates({
      en: { 'a.b': 'Hello' },
      ja: {}, // key dropped entirely → contributes to "work remaining", not to copies
    })
    expect(absent.rows[0]).toMatchObject({ locale: 'ja', total: 1, same: 0, rate: 0 })

    const emptyBase = copyRates({ en: {}, ja: { 'a.b': 'x' } })
    expect(emptyBase.rows[0]).toMatchObject({ locale: 'ja', total: 0, same: 0, rate: 0 })
  })
})

describe('i18n-check CLI', () => {
  it('exits 0 against the real locale files', () => {
    const out = execFileSync(process.execPath, [SCRIPT], { encoding: 'utf8' })
    expect(out).toMatch(/i18n-check: OK/)
  })

  it('--report exits 0, prints the fixed note and the per-locale table (pure report, never a gate)', () => {
    // execFileSync throws on a non-zero exit, so a clean return IS exit 0.
    const out = execFileSync(process.execPath, [SCRIPT, '--report'], { encoding: 'utf8' })
    expect(out).toContain('i18n-report')
    // The fixed B1-14/decision-5 explanatory line at the top of the report.
    expect(out).toContain('en 兜底')
    expect(out).toContain('决策 5')
    expect(out).toMatch(/locale\s+same\/total\s+copy-rate/)
    expect(out).toMatch(/zh-CN\s+\d+\/\d+\s+\d+\.\d%/)
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

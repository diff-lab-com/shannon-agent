// Task 2 (settings-parity R3) — the pure locale resolver behind the
// "follow system" language option.
//
// `resolveLocale(pref)` is the extracted first-run probe: 'system' (or a
// missing/unknown pref) reads `navigator.languages` live — deliberately
// uncached, so a window opened after the OS language changes follows it —
// while a concrete supported locale passes straight through (backward
// compat: stored locale values keep their exact pre-'system' behavior).
// The navigator is stubbed per test; `languages` can also be injected
// directly, but stubbing is the path production code takes.

import { describe, it, expect, afterEach, vi } from 'vitest'
import { resolveLocale } from '@/i18n'

afterEach(() => {
  vi.unstubAllGlobals()
})

function stubNavigatorLanguages(langs: readonly string[]) {
  vi.stubGlobal('navigator', { languages: langs })
}

describe('resolveLocale — follow system (navigator probe)', () => {
  it("pref 'system' resolves from navigator.languages", () => {
    stubNavigatorLanguages(['fr-FR', 'en-US'])
    expect(resolveLocale('system')).toBe('fr')
  })

  it('walks the preference list until a supported language matches', () => {
    stubNavigatorLanguages(['xx-YY', 'de-DE', 'ja-JP'])
    expect(resolveLocale('system')).toBe('de')
  })

  it('falls back to en when nothing matches', () => {
    stubNavigatorLanguages(['xx-YY', 'zz-ZZ'])
    expect(resolveLocale('system')).toBe('en')
  })

  it('falls back to en when navigator.languages is missing', () => {
    stubNavigatorLanguages(undefined)
    expect(resolveLocale('system')).toBe('en')
  })

  it('a missing pref (first run) behaves like system', () => {
    stubNavigatorLanguages(['ru-RU'])
    expect(resolveLocale(null)).toBe('ru')
    expect(resolveLocale(undefined)).toBe('ru')
  })

  it('an unknown junk pref behaves like system', () => {
    stubNavigatorLanguages(['es-419'])
    expect(resolveLocale('not-a-locale')).toBe('es')
  })

  it('is not cached: a later probe picks up an OS language change', () => {
    stubNavigatorLanguages(['en-US'])
    expect(resolveLocale('system')).toBe('en')
    // The user switches their OS to Japanese; the next resolve (new window)
    // must follow — no snapshot taken at the first call.
    stubNavigatorLanguages(['ja-JP'])
    expect(resolveLocale('system')).toBe('ja')
  })
})

describe('resolveLocale — zh-TW/Hant branches (first-run parity)', () => {
  it('zh-TW maps to zh-TW', () => {
    stubNavigatorLanguages(['zh-TW'])
    expect(resolveLocale('system')).toBe('zh-TW')
  })

  it('zh-HK maps to zh-TW (the pre-existing branch)', () => {
    stubNavigatorLanguages(['zh-HK'])
    expect(resolveLocale('system')).toBe('zh-TW')
  })

  it('plain zh (incl. zh-CN/zh-SG) maps to zh-CN', () => {
    for (const tag of ['zh', 'zh-CN', 'zh-SG', 'zh-Hans-CN']) {
      stubNavigatorLanguages([tag])
      expect(resolveLocale('system')).toBe('zh-CN')
    }
  })

  it('matching is case-insensitive', () => {
    stubNavigatorLanguages(['ZH-tw'])
    expect(resolveLocale('system')).toBe('zh-TW')
  })
})

describe('resolveLocale — explicit locales pass through', () => {
  it('a concrete stored locale ignores the navigator entirely', () => {
    stubNavigatorLanguages(['fr-FR'])
    expect(resolveLocale('ja')).toBe('ja')
    expect(resolveLocale('zh-TW')).toBe('zh-TW')
    expect(resolveLocale('pt-BR')).toBe('pt-BR')
  })

  it('every supported locale id round-trips', () => {
    stubNavigatorLanguages(['xx-XX']) // would fall to en if the pref were dropped
    for (const id of ['en', 'zh-CN', 'es', 'fr', 'de', 'ja', 'ko', 'pt-BR', 'ru', 'zh-TW']) {
      expect(resolveLocale(id)).toBe(id)
    }
  })

  it('accepts an injected languages list (pure path, no global navigator)', () => {
    expect(resolveLocale('system', ['ko-KR'])).toBe('ko')
    expect(resolveLocale(null, ['pt-PT'])).toBe('pt-BR')
  })
})

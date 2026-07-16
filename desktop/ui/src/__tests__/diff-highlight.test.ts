import { describe, it, expect } from 'vitest'
import { highlightLines, resolveDiffLang } from '@/lib/diff-highlight'

describe('resolveDiffLang', () => {
  it('resolves explicit language hint', () => {
    expect(resolveDiffLang('typescript', 'x.txt')).toBe('typescript')
    expect(resolveDiffLang('python', 'y.py')).toBe('python')
  })

  it('returns undefined for plain text', () => {
    expect(resolveDiffLang('text', 'x.txt')).toBeUndefined()
    expect(resolveDiffLang('plaintext', 'x.txt')).toBeUndefined()
  })

  it('falls back to file extension when language missing or unknown', () => {
    expect(resolveDiffLang(undefined, 'app.ts')).toBe('typescript')
    expect(resolveDiffLang(undefined, 'script.sh')).toBe('bash')
    expect(resolveDiffLang(undefined, 'config.yml')).toBe('yaml')
    expect(resolveDiffLang(undefined, 'readme.md')).toBe('markdown')
  })

  it('returns undefined for unknown extension', () => {
    expect(resolveDiffLang(undefined, 'weird.zomg')).toBeUndefined()
  })

  it('normalizes language aliases', () => {
    expect(resolveDiffLang('ts', undefined)).toBe('typescript')
    expect(resolveDiffLang('js', undefined)).toBe('javascript')
    expect(resolveDiffLang('py', undefined)).toBe('python')
  })
})

describe('highlightLines', () => {
  it('returns one HTML string per source line', () => {
    const out = highlightLines('a\nb\nc', 'typescript')
    expect(out).toHaveLength(3)
    expect(out[0]).toContain('a')
    expect(out[2]).toContain('c')
  })

  it('preserves escaped HTML across line splits when no language', () => {
    const out = highlightLines('<a>\nb', undefined)
    expect(out[0]).toBe('&lt;a&gt;')
    expect(out[1]).toBe('b')
  })

  it('wraps tokens in hljs spans when language is registered', () => {
    const out = highlightLines('const x = 1', 'typescript')
    expect(out.join('')).toContain('hljs-')
  })

  it('carries open spans across newlines so multiline constructs stay colored', () => {
    const src = '/* multi\nline\ncomment */'
    const out = hljsHighlightLines(src, 'javascript')
    expect(out.length).toBe(3)
    // First line opens the comment span; subsequent lines should still have
    // an hljs-comment span open so they render colored.
    expect(out[0]).toMatch(/<span class="hljs-comment"/)
    expect(out[1]).toMatch(/<span class="hljs-comment"/)
    expect(out[2]).toMatch(/<span class="hljs-comment"/)
  })

  it('falls back to escaped plaintext when language is unknown', () => {
    const out = highlightLines('<x>', 'totally-not-a-language')
    expect(out[0]).toBe('&lt;x&gt;')
  })
})

function hljsHighlightLines(source: string, lang: string): string[] {
  return highlightLines(source, lang)
}

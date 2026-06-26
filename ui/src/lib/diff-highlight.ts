import { hljs, resolveLanguage } from './hljs'

export function resolveDiffLang(language: string | undefined, fileName: string | undefined): string | undefined {
  return resolveLanguage(language, fileName)
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c] ?? c))
}

/**
 * Highlight a full source string and split the resulting HTML into per-line
 * HTML strings. Open highlight spans carry forward across line boundaries so
 * multi-line constructs (e.g. block comments) stay colored on every line.
 *
 * Output is safe by construction: hljs escapes all input characters before
 * wrapping tokens in <span class="hljs-*"> tags, so the only tags present in
 * the output are those hljs emits — never user-controlled.
 */
export function highlightLines(source: string, lang: string | undefined): string[] {
  if (!lang || !hljs.getLanguage(lang)) {
    return source.split('\n').map(escapeHtml)
  }
  try {
    const html = hljs.highlight(source, { language: lang }).value
    return splitHtmlByLines(html)
  } catch {
    return source.split('\n').map(escapeHtml)
  }
}

function splitHtmlByLines(html: string): string[] {
  const lines: string[] = []
  let current = ''
  const openSpans: string[] = []
  let i = 0
  while (i < html.length) {
    const ch = html[i]
    if (ch === '<') {
      const end = html.indexOf('>', i)
      if (end === -1) {
        current += html.slice(i)
        break
      }
      const tag = html.slice(i, end + 1)
      current += tag
      if (tag.startsWith('</')) {
        if (openSpans.length > 0) openSpans.pop()
      } else if (tag.startsWith('<span') && !tag.endsWith('/>')) {
        openSpans.push(tag)
      }
      i = end + 1
    } else if (ch === '\n') {
      current += openSpans.map(() => '</span>').join('')
      lines.push(current)
      current = openSpans.join('')
      i++
    } else {
      let next = html.length
      const lt = html.indexOf('<', i)
      if (lt !== -1) next = Math.min(next, lt)
      const nl = html.indexOf('\n', i)
      if (nl !== -1) next = Math.min(next, nl)
      current += html.slice(i, next)
      i = next
    }
  }
  lines.push(current)
  return lines
}

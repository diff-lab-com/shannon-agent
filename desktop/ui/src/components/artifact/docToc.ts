// Batch D1 (2026-09-20 delta analysis): pure heading parsing for the
// document TOC. Shared by DocumentRenderer (which assigns the matching DOM
// ids) and DocumentToc (which renders the rail + scroll-spy) — the dedup
// rules here must stay in lockstep with the renderer's id assignment.

export interface DocHeading {
  level: 1 | 2 | 3
  text: string
  id: string
}

/** GitHub-style slug: lowercase, punctuation stripped, spaces → dashes.
 *  CJK characters are kept (they carry the meaning in zh docs). */
export function slugifyHeading(text: string): string {
  return text
    .trim()
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s-]/gu, '')
    .replace(/\s+/g, '-')
}

/** Extract plain text from a markdown heading line (`**bold**`, links…). */
export function plainHeadingText(line: string): string {
  return line
    .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/[*_`~]/g, '')
    .replace(/<[^>]+>/g, '')
    .trim()
}

/**
 * Parse h1–h3 headings out of a markdown source, skipping fenced code
 * blocks, and assign the same ids the renderer will (`slug`, deduped with
 * `-1`/`-2` suffixes in document order).
 */
export function parseDocHeadings(source: string): DocHeading[] {
  const out: DocHeading[] = []
  const seen = new Map<string, number>()
  let inFence = false
  for (const line of source.split('\n')) {
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence
      continue
    }
    if (inFence) continue
    const m = /^(#{1,3})\s+(.+?)\s*#*\s*$/.exec(line)
    if (!m) continue
    const level = m[1].length as 1 | 2 | 3
    const text = plainHeadingText(m[2])
    if (!text) continue
    const base = slugifyHeading(text) || 'section'
    const n = seen.get(base) ?? 0
    seen.set(base, n + 1)
    out.push({ level, text, id: n === 0 ? base : `${base}-${n}` })
  }
  return out
}

/** Pull plain text out of a rendered React children tree (headings may
 *  contain emphasis/links/code — the DOM id must match the parsed text). */
export function reactNodeText(node: unknown): string {
  if (node == null || typeof node === 'boolean') return ''
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(reactNodeText).join('')
  if (typeof node === 'object' && 'props' in (node as Record<string, unknown>)) {
    const props = (node as { props?: { children?: unknown } }).props
    return reactNodeText(props?.children)
  }
  return ''
}

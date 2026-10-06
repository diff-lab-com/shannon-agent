export type ArtifactKind = 'html' | 'svg' | 'mermaid' | 'document' | 'image' | 'web' | 'other'

export interface DetectedArtifact {
  kind: ArtifactKind
  /** Text content, a URL (`web`) or an absolute file path (`image`/`other`). */
  source: string
  title: string
  confidence: 'high' | 'medium'
  /** Provenance when the artifact came from a real disk file (P1-C). */
  path?: string
  origin?: 'chat' | 'disk'
  /**
   * Explicit tab id. Disk artifacts use `disk:<path>`; chat-fence artifacts
   * get a content hash `<kind>:<fnv1aHex><lenHex>` (§P1-10) so identical
   * content converges on one dock tab instead of stacking duplicates.
   */
  id?: string
}

interface CodeFenceBlock {
  lang: string
  body: string
}

// B3 §P2-24: detect ``` and ~~~ fences. Two patterns, not one with a
// backreference alternation — in JS a backref to a non-participating group
// matches the empty string, which truncates every body at the first blank
// line. The closing marker is the same literal as the opening (CommonMark
// requires the same token; variable-length fences stay unsupported), and
// the info string may carry a bare language tag.
const FENCE_PATTERNS = [
  /```([a-zA-Z0-9]*)[ \t]*\n([\s\S]*?)```[ \t]*(?=\n|$)/g,
  /~~~([a-zA-Z0-9]*)[ \t]*\n([\s\S]*?)~~~[ \t]*(?=\n|$)/g,
]

interface ExtractedFence extends CodeFenceBlock {
  index: number
  end: number
}

function extractFences(markdown: string): CodeFenceBlock[] {
  const found: ExtractedFence[] = []
  for (const re of FENCE_PATTERNS) {
    re.lastIndex = 0
    let m: RegExpExecArray | null
    while ((m = re.exec(markdown)) !== null) {
      found.push({ index: m.index, end: m.index + m[0].length, lang: (m[1] || '').toLowerCase(), body: m[2] })
    }
  }
  // Document order; a match nested inside another marker's body (e.g. ~~~
  // appearing inside a ``` fence) is dropped, not reported twice.
  found.sort((a, b) => a.index - b.index)
  const out: CodeFenceBlock[] = []
  let lastEnd = -1
  for (const fence of found) {
    if (fence.index < lastEnd) continue
    out.push({ lang: fence.lang, body: fence.body })
    lastEnd = fence.end
  }
  return out
}

// Title fallbacks resolve at display time via artifactDisplayTitle() in
// labels.ts — detection stays locale-free and returns '' when the source
// carries no explicit title.

/** Strip markup tags and collapse whitespace — titles are plain text. */
function cleanTitle(raw: string): string {
  return raw
    .replace(/<[^>]+>/g, ' ')
    .replace(/\s+/g, ' ')
    .trim()
    .slice(0, 80)
}

function titleFromHtml(src: string): string {
  const m = src.match(/<title>([^<]+)<\/title>/i)
  return m ? m[1].trim().slice(0, 80) : ''
}

/** B3 §P2-24: prefer <title>, else the first non-empty <text> run. */
function titleFromSvg(src: string): string {
  const title = src.match(/<title\b[^>]*>([\s\S]*?)<\/title>/i)
  if (title) {
    const cleaned = cleanTitle(title[1])
    if (cleaned) return cleaned
  }
  for (const m of src.matchAll(/<text\b[^>]*>([\s\S]*?)<\/text>/gi)) {
    const cleaned = cleanTitle(m[1])
    if (cleaned) return cleaned
  }
  return ''
}

/** B3 §P2-24: first node label — `A[Start]` / `B(Road)` / `C{Gate}`. */
function titleFromMermaid(src: string): string {
  const m = src.match(/[[( {]([^\][(){}\n"]{2,80})[\])}]/)
  return m ? cleanTitle(m[1]) : ''
}

function titleFromDocument(markdown: string): string {
  const heading = markdown.match(/^#{1,3}\s+(.+)$/m)
  return heading ? heading[1].trim().slice(0, 80) : ''
}

const MIN_HTML_LINES = 5
const MIN_DOC_WORDS = 200

/**
 * FNV-1a 32-bit over the normalized source, rendered as 8 hex chars plus
 * the source length (hex) as a cheap second discriminator — a 32-bit hash
 * alone is fine for tab-dedup UX, but bundling the length costs nothing and
 * makes accidental collisions of different-size sources impossible.
 */
function fnv1a32(input: string): string {
  let hash = 0x811c9dc5
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193)
  }
  return (hash >>> 0).toString(16).padStart(8, '0')
}

/**
 * Stable chat-artifact tab id (§P1-10): hash the normalized full source
 * (CRLF folded to LF, outer whitespace trimmed — the same bytes the
 * renderer sees) instead of the old first-200-chars prefix, so re-generated
 * or re-emitted identical content reuses one tab. Same helper backs the
 * provider's auto-open bookkeeping for chips that arrive without an id.
 */
export function stableArtifactId(kind: ArtifactKind, source: string): string {
  const normalized = source.replace(/\r\n/g, '\n').trim()
  return `${kind}:${fnv1a32(normalized)}${normalized.length.toString(16)}`
}

export function detectArtifacts(content: string): DetectedArtifact[] {
  const out: DetectedArtifact[] = []
  // B3 §P1-10: dedup on the stable content id (exact, not a 200-char
  // prefix that could merge distinct blocks sharing a long prologue).
  const seen = new Set<string>()

  // Push helper: dedup on the stable id, then attach it.
  const push = (kind: ArtifactKind, title: string, confidence: 'high' | 'medium', body: string) => {
    const id = stableArtifactId(kind, body)
    if (seen.has(id)) return
    out.push({ kind, source: body, title, confidence, id })
    seen.add(id)
  }

  for (const fence of extractFences(content)) {
    const { lang, body } = fence
    if (!body.trim()) continue

    if ((lang === 'html' || lang === 'htm') && body.split('\n').length >= MIN_HTML_LINES) {
      push('html', titleFromHtml(body), 'high', body)
    } else if (lang === 'svg' || (lang === '' && body.trim().startsWith('<svg'))) {
      push('svg', titleFromSvg(body), 'high', body)
    } else if (lang === 'mermaid') {
      push('mermaid', titleFromMermaid(body), 'high', body)
    } else if (lang === 'markdown' || lang === 'md') {
      const words = body.split(/\s+/).filter(Boolean).length
      if (words >= MIN_DOC_WORDS) {
        push('document', titleFromDocument(body), 'medium', body)
      }
    }
  }

  return out
}

export function artifactIcon(kind: ArtifactKind): string {
  switch (kind) {
    case 'html': return 'web'
    case 'svg': return 'image'
    case 'mermaid': return 'account_tree'
    case 'document': return 'description'
    case 'image': return 'photo_library'
    case 'web': return 'language'
    case 'other': return 'draft'
  }
}

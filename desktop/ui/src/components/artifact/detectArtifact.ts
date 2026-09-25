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
   * Explicit tab id — disk artifacts use `disk:<path>` so re-opening the
   * same file replaces its tab instead of stacking duplicates.
   */
  id?: string
}

interface CodeFenceBlock {
  lang: string
  body: string
}

const FENCE_RE = /```([a-zA-Z0-9]+)?\n([\s\S]*?)```/g

function extractFences(markdown: string): CodeFenceBlock[] {
  const out: CodeFenceBlock[] = []
  let m: RegExpExecArray | null
  while ((m = FENCE_RE.exec(markdown)) !== null) {
    out.push({ lang: (m[1] || '').toLowerCase(), body: m[2] })
  }
  return out
}

// Title fallbacks resolve at display time via artifactDisplayTitle() in
// labels.ts — detection stays locale-free and returns '' when the source
// carries no explicit title.

function titleFromHtml(src: string): string {
  const m = src.match(/<title>([^<]+)<\/title>/i)
  return m ? m[1].trim().slice(0, 80) : ''
}

function titleFromSvg(_src: string): string {
  return ''
}

function titleFromMermaid(_src: string): string {
  return ''
}

function titleFromDocument(markdown: string): string {
  const heading = markdown.match(/^#{1,3}\s+(.+)$/m)
  return heading ? heading[1].trim().slice(0, 80) : ''
}

const MIN_HTML_LINES = 5
const MIN_DOC_WORDS = 200

export function detectArtifacts(content: string): DetectedArtifact[] {
  const out: DetectedArtifact[] = []
  const seen = new Set<string>()

  for (const fence of extractFences(content)) {
    const { lang, body } = fence
    if (!body.trim()) continue
    const key = body.trim().slice(0, 200)
    if (seen.has(key)) continue

    if (lang === 'html' && body.split('\n').length >= MIN_HTML_LINES) {
      out.push({ kind: 'html', source: body, title: titleFromHtml(body), confidence: 'high' })
      seen.add(key)
    } else if (lang === 'svg' || (lang === '' && body.trim().startsWith('<svg'))) {
      out.push({ kind: 'svg', source: body, title: titleFromSvg(body), confidence: 'high' })
      seen.add(key)
    } else if (lang === 'mermaid') {
      out.push({ kind: 'mermaid', source: body, title: titleFromMermaid(body), confidence: 'high' })
      seen.add(key)
    } else if (lang === 'markdown' || lang === 'md') {
      const words = body.split(/\s+/).filter(Boolean).length
      if (words >= MIN_DOC_WORDS) {
        out.push({ kind: 'document', source: body, title: titleFromDocument(body), confidence: 'medium' })
        seen.add(key)
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


export type ArtifactKind = 'html' | 'svg' | 'mermaid' | 'document'

export interface DetectedArtifact {
  kind: ArtifactKind
  source: string
  title: string
  confidence: 'high' | 'medium'
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

function titleFromHtml(src: string): string {
  const m = src.match(/<title>([^<]+)<\/title>/i)
  return m ? m[1].trim().slice(0, 80) : 'HTML document'
}

function titleFromSvg(_src: string): string {
  return 'SVG diagram'
}

function titleFromMermaid(_src: string): string {
  return 'Mermaid diagram'
}

function titleFromDocument(markdown: string): string {
  const heading = markdown.match(/^#{1,3}\s+(.+)$/m)
  return heading ? heading[1].trim().slice(0, 80) : 'Document'
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
  }
}

export function artifactKindLabel(kind: ArtifactKind): string {
  switch (kind) {
    case 'html': return 'HTML'
    case 'svg': return 'SVG'
    case 'mermaid': return 'Diagram'
    case 'document': return 'Document'
  }
}

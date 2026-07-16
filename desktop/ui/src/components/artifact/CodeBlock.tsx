import { useMemo } from 'react'
import { hljs } from '@/lib/hljs'

const KIND_TO_LANG: Record<string, string> = {
  html: 'html',
  svg: 'xml',
  mermaid: 'yaml',
  document: 'markdown',
}

interface CodeBlockProps {
  source: string
  kind?: string
  className?: string
}

export function CodeBlock({ source, kind, className }: CodeBlockProps) {
  const html = useMemo(() => {
    const lang = kind ? KIND_TO_LANG[kind] : undefined
    try {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(source, { language: lang }).value
      }
      return hljs.highlightAuto(source).value
    } catch {
      return source.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c] ?? c))
    }
  }, [source, kind])

  return (
    <pre
      className={`p-md text-body-sm font-mono text-on-surface whitespace-pre-wrap break-words overflow-x-auto ${className ?? ''}`}
    >
      <code
        className="hljs"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </pre>
  )
}

// Safe by construction: highlight.js escapes all input characters then wraps
// tokens in <span class="hljs-*">. Output never contains user-controlled tags,
// URLs, scripts, or event handlers — only escaped text + safe hljs spans.

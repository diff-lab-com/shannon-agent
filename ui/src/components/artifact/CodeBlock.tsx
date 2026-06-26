import { useMemo } from 'react'
import hljs from 'highlight.js/lib/core'
import javascript from 'highlight.js/lib/languages/javascript'
import typescript from 'highlight.js/lib/languages/typescript'
import xml from 'highlight.js/lib/languages/xml'
import css from 'highlight.js/lib/languages/css'
import json from 'highlight.js/lib/languages/json'
import markdown from 'highlight.js/lib/languages/markdown'
import python from 'highlight.js/lib/languages/python'
import rust from 'highlight.js/lib/languages/rust'
import bash from 'highlight.js/lib/languages/bash'
import yaml from 'highlight.js/lib/languages/yaml'

hljs.registerLanguage('javascript', javascript)
hljs.registerLanguage('typescript', typescript)
hljs.registerLanguage('xml', xml)
hljs.registerLanguage('html', xml)
hljs.registerLanguage('css', css)
hljs.registerLanguage('json', json)
hljs.registerLanguage('markdown', markdown)
hljs.registerLanguage('python', python)
hljs.registerLanguage('rust', rust)
hljs.registerLanguage('bash', bash)
hljs.registerLanguage('shell', bash)
hljs.registerLanguage('yaml', yaml)
hljs.registerLanguage('yml', yaml)

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

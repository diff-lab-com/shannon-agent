import { CodeBlock as SharedCodeBlock } from '@/components/code/CodeBlock'
import { cn } from '@/lib/utils'

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

// Artifact-panel code view: headerless shared primitive (no chrome) with
// self-highlighting from the raw source; soft-wrapped to match the panel.
export function CodeBlock({ source, kind, className }: CodeBlockProps) {
  const lang = kind ? KIND_TO_LANG[kind] : undefined
  return (
    <SharedCodeBlock
      code={source}
      language={lang}
      chrome={false}
      lineNumbers={false}
      className={cn('my-0 rounded-none border-0', className)}
      contentClassName="whitespace-pre-wrap break-words font-mono text-on-surface"
    />
  )
}

// Safe by construction: highlight.js escapes all input characters then wraps
// tokens in <span class="hljs-*">. Output never contains user-controlled tags,
// URLs, scripts, or event handlers — only escaped text + safe hljs spans.

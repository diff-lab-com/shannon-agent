import { memo, useMemo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize from 'rehype-sanitize'
import rehypeHighlight from 'rehype-highlight'
import { CodeBlock as SharedCodeBlock } from '@/components/code/CodeBlock'
import { cn } from '@/lib/utils'
import { reactNodeText, slugifyHeading } from './docToc'

interface DocumentRendererProps {
  source: string
}

type CodeChildProps = { className?: string; children?: React.ReactNode }

function getCodeChildProps(children: React.ReactNode): CodeChildProps | null {
  const child = Array.isArray(children) ? children[0] : children
  if (child && typeof child === 'object' && 'props' in child) {
    return (child as { props: CodeChildProps }).props ?? null
  }
  return null
}

function extractLanguage(className?: string): string | null {
  if (!className) return null
  const m = /language-([\w+-]+)/.exec(className)
  return m ? m[1] : null
}

function extractText(node: React.ReactNode): string {
  if (node == null || typeof node === 'boolean') return ''
  if (typeof node === 'string' || typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(extractText).join('')
  if (typeof node === 'object') {
    const props = (node as { props?: { children?: React.ReactNode } }).props
    if (props) return extractText(props.children)
  }
  return ''
}

export const DocumentRenderer = memo(function DocumentRenderer({ source }: DocumentRendererProps) {
  // Batch D1: heading ids share the slug+dedup rules of parseDocHeadings()
  // (docToc.ts) so the TOC rail can target and scroll-spy them. The dedup
  // counter lives in a per-source closure — react-markdown renders headings
  // in document order within one pass.
  const components = useMemo(() => {
    const seen = new Map<string, number>()
    const headingId = (children: React.ReactNode) => {
      const base = slugifyHeading(reactNodeText(children)) || 'section'
      const n = seen.get(base) ?? 0
      seen.set(base, n + 1)
      return n === 0 ? base : `${base}-${n}`
    }
    return {
      h1: ({ children }: { children?: React.ReactNode }) => <h1 id={headingId(children)} className="text-headline-md font-headline-md text-on-surface mt-lg mb-sm scroll-mt-md">{children}</h1>,
      h2: ({ children }: { children?: React.ReactNode }) => <h2 id={headingId(children)} className="text-headline-sm font-headline-sm text-on-surface mt-md mb-xs scroll-mt-md">{children}</h2>,
      h3: ({ children }: { children?: React.ReactNode }) => <h3 id={headingId(children)} className="text-label-lg font-bold text-on-surface mt-md mb-xs scroll-mt-md">{children}</h3>,
      p: ({ children }: { children?: React.ReactNode }) => <p className="text-body-md text-on-surface mb-sm leading-relaxed">{children}</p>,
      ul: ({ children }: { children?: React.ReactNode }) => <ul className="list-disc pl-md mb-sm text-body-md text-on-surface space-y-xs">{children}</ul>,
      ol: ({ children }: { children?: React.ReactNode }) => <ol className="list-decimal pl-md mb-sm text-body-md text-on-surface space-y-xs">{children}</ol>,
      // Batch D3: artifact code blocks go through the shared CodeBlock
      // primitive (copy button · language label · line-number gutter) —
      // same reader experience as conversation and plan code blocks.
      pre: ({ children }: { children?: React.ReactNode }) => {
        const codeProps = getCodeChildProps(children)
        const code = extractText(codeProps?.children)
        const language = extractLanguage(codeProps?.className)
        return (
          <SharedCodeBlock code={code} language={language} lineNumbers="toggle">
            {children}
          </SharedCodeBlock>
        )
      },
      code: ({ children, className }: { children?: React.ReactNode; className?: string }) => {
        if (className?.includes('language-')) return <code className={className}>{children}</code>
        return <code className="bg-surface-container-high text-on-surface rounded px-[2px] py-[1px] text-label-sm font-mono">{children}</code>
      },
      a: ({ children, href }: { children?: React.ReactNode; href?: string }) => (
        <a href={href} target="_blank" rel="noreferrer" className="text-primary underline hover:text-primary/80">{children}</a>
      ),
      blockquote: ({ children }: { children?: React.ReactNode }) => (
        <blockquote className="border-l-2 border-outline-variant pl-md italic text-on-surface-variant my-sm">{children}</blockquote>
      ),
      table: ({ children }: { children?: React.ReactNode }) => (
        <table className="w-full border-collapse text-label-sm text-on-surface my-sm">{children}</table>
      ),
      th: ({ children }: { children?: React.ReactNode }) => (
        <th className="border border-outline-variant/30 px-sm py-xs bg-surface-container-high text-left font-bold">{children}</th>
      ),
      td: ({ children }: { children?: React.ReactNode }) => (
        <td className="border border-outline-variant/30 px-sm py-xs">{children}</td>
      ),
    }
    // `source` never appears inside the callback, but it re-seeds the dedup
    // counters per document — dropping it would leak slugs across artifacts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source])

  return (
    <article className={cn('p-md max-w-none')}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeSanitize, rehypeHighlight]}
        components={components}
      >
        {source}
      </ReactMarkdown>
    </article>
  )
})

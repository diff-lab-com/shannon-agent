import { memo } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeSanitize from 'rehype-sanitize'
import rehypeHighlight from 'rehype-highlight'

interface DocumentRendererProps {
  source: string
}

const components = {
  h1: ({ children }: { children?: React.ReactNode }) => <h1 className="text-headline-md font-headline-md text-on-surface mt-lg mb-sm">{children}</h1>,
  h2: ({ children }: { children?: React.ReactNode }) => <h2 className="text-headline-sm font-headline-sm text-on-surface mt-md mb-xs">{children}</h2>,
  h3: ({ children }: { children?: React.ReactNode }) => <h3 className="text-label-lg font-bold text-on-surface mt-md mb-xs">{children}</h3>,
  p: ({ children }: { children?: React.ReactNode }) => <p className="text-body-md text-on-surface mb-sm leading-relaxed">{children}</p>,
  ul: ({ children }: { children?: React.ReactNode }) => <ul className="list-disc pl-md mb-sm text-body-md text-on-surface space-y-xs">{children}</ul>,
  ol: ({ children }: { children?: React.ReactNode }) => <ol className="list-decimal pl-md mb-sm text-body-md text-on-surface space-y-xs">{children}</ol>,
  code: ({ children, className }: { children?: React.ReactNode; className?: string }) => {
    const isBlock = className?.includes('language-')
    return isBlock ? (
      <code className={`block bg-surface-container-high text-on-surface rounded-md p-md overflow-x-auto text-label-sm font-mono ${className ?? ''}`}>{children}</code>
    ) : (
      <code className="bg-surface-container-high text-on-surface rounded px-[2px] py-[1px] text-label-sm font-mono">{children}</code>
    )
  },
  pre: ({ children }: { children?: React.ReactNode }) => <pre className="mb-sm">{children}</pre>,
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

export const DocumentRenderer = memo(function DocumentRenderer({ source }: DocumentRendererProps) {
  return (
    <article className="p-md max-w-none">
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

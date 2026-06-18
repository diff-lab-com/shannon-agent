import { useState, useEffect, memo, type ReactNode } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import { convertFileSrc } from '@tauri-apps/api/core'

interface MarkdownProps {
  children: string
  className?: string
}

export const Markdown = memo(function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={className}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[rehypeHighlight]}
        components={{
          pre: CodeBlock,
          img: LocalImage,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
})

function extractText(node: ReactNode): string {
  if (typeof node === 'string') return node
  if (typeof node === 'number') return String(node)
  if (Array.isArray(node)) return node.map(extractText).join('')
  if (node && typeof node === 'object' && 'props' in node) {
    return extractText((node as { props: { children?: ReactNode } }).props.children)
  }
  return ''
}

function CodeBlock({ children, ...props }: { children?: ReactNode } & React.HTMLAttributes<HTMLPreElement>) {
  const code = extractText(children)
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    }).catch(() => {})
  }

  return (
    <div className="relative group/code">
      <button
        type="button"
        onClick={handleCopy}
        aria-label="Copy code"
        className="absolute top-xs right-xs opacity-0 group-hover/code:opacity-100 focus-visible:opacity-100 transition-opacity px-xs py-[2px] rounded-md bg-surface-container-high/80 text-on-surface-variant hover:text-primary text-[11px] flex items-center gap-xs cursor-pointer"
      >
        <span className="material-symbols-outlined text-[14px]">{copied ? 'check' : 'content_copy'}</span>
        {copied ? 'Copied' : 'Copy'}
      </button>
      <pre {...props}>{children}</pre>
    </div>
  )
}

function LocalImage({ src, alt, ...rest }: React.ImgHTMLAttributes<HTMLImageElement>) {
  const [resolved, setResolved] = useState(src)
  useEffect(() => {
    if (typeof src !== 'string') { setResolved(src); return }
    let mounted = true
    try {
      if (src.startsWith('file://')) {
        const path = src.replace(/^file:\/\//, '')
        const converted = convertFileSrc(path)
        if (mounted) setResolved(converted)
      } else if (src.startsWith('/') && !src.startsWith('//')) {
        const converted = convertFileSrc(src)
        if (mounted) setResolved(converted)
      } else {
        if (mounted) setResolved(src)
      }
    } catch {
      if (mounted) setResolved(src)
    }
    return () => { mounted = false }
  }, [src])

  return <img src={resolved} alt={alt} className="max-w-full rounded-lg" {...rest} />
}

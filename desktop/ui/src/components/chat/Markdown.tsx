import { useState, useEffect, memo, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Chart, parseChartSpec } from '@/components/chat/Chart'

const sanitizeSchema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    '*': [
      ...((defaultSchema.attributes && defaultSchema.attributes['*']) || []),
      /^data-[a-z0-9-]+$/i,
    ],
  },
}

interface MarkdownProps {
  children: string
  className?: string
}

export const Markdown = memo(function Markdown({ children, className }: MarkdownProps) {
  return (
    <div className={className}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        rehypePlugins={[
          rehypeHighlight,
          [rehypeSanitize, sanitizeSchema],
        ]}
        components={{
          pre: PreOrChart,
          img: LocalImage,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
})

function PreOrChart(props: React.HTMLAttributes<HTMLPreElement>) {
  const child = Array.isArray(props.children) ? props.children[0] : props.children
  if (child && typeof child === 'object' && 'props' in child) {
    const codeProps = (child as { props: { className?: string; children?: ReactNode } }).props
    if (/language-chart/.test(codeProps.className ?? '')) {
      const raw = extractText(codeProps.children)
      const spec = parseChartSpec(raw)
      if (spec) return <Chart spec={spec} />
      return (
        <div className="my-md p-sm rounded-lg bg-error-container/20 border border-error/30 text-label-sm text-error">
          <span className="material-symbols-outlined text-[14px] align-middle mr-xs">error</span>
          Invalid chart spec — expected JSON with type and data[].
        </div>
      )
    }
  }
  return <CodeBlock {...props} />
}

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
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
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
        aria-label={t('chat.copyCode.aria')}
        className="absolute top-xs right-xs opacity-0 group-hover/code:opacity-100 focus-visible:opacity-100 transition-opacity px-xs py-[2px] rounded-md bg-surface-container-high/80 text-on-surface-variant hover:text-primary text-[11px] flex items-center gap-xs cursor-pointer"
      >
        <span className="material-symbols-outlined text-[14px]">{copied ? 'check' : 'content_copy'}</span>
        {copied ? t('chat.copyCode.copied') : t('chat.copyCode.copy')}
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

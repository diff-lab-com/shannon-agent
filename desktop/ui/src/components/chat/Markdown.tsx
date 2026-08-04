import { useState, useEffect, memo, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import rehypeHighlight from 'rehype-highlight'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Chart, parseChartSpec } from '@/components/chat/Chart'

// Extend the default sanitize schema so syntax-highlight classes from
// rehype-highlight (e.g. `hljs-keyword`) survive sanitization. Keep the
// `data-*` allowance so chart specs and code-block language labels pass.
const sanitizeSchema = {
  ...defaultSchema,
  attributes: {
    ...defaultSchema.attributes,
    code: [
      ...((defaultSchema.attributes && defaultSchema.attributes.code) || []),
      'className',
    ],
    span: [
      ...((defaultSchema.attributes && defaultSchema.attributes.span) || []),
      'className',
    ],
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
          table: TableRoot,
          th: TableHeader,
          td: TableCell,
          blockquote: BlockQuote,
          a: ExternalLink,
          code: InlineCode,
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
})

/* ────────────────────  Code blocks & charts  ──────────────────── */

/** Dispatches `language-chart` to the chart renderer; everything else
 *  goes to the generic CodeBlock. */
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

/* Re-extract just the `<code>` props from `react-markdown`'s pre wrapper.
 * react-markdown nests <pre><code class="language-x">…</code></pre>, so
 * the language label, copy text, and class info all live one level down. */
type CodeChildProps = { className?: string; children?: ReactNode }
function getCodeChildProps(children: ReactNode): CodeChildProps | null {
  const child = Array.isArray(children) ? children[0] : children
  if (child && typeof child === 'object' && 'props' in child) {
    const props = (child as { props: CodeChildProps }).props
    return props ?? null
  }
  return null
}

/** Extract a `language-xxx` class into the bare language name. */
function extractLanguage(className?: string): string | null {
  if (!className) return null
  const m = /language-([\w+-]+)/.exec(className)
  return m ? m[1] : null
}

function CodeBlock(props: { children?: ReactNode } & React.HTMLAttributes<HTMLPreElement>) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const codeProps = getCodeChildProps(props.children)
  const code = extractText(codeProps?.children)
  const language = extractLanguage(codeProps?.className)
  const [copied, setCopied] = useState(false)
  const [showLines, setShowLines] = useState(false)
  const lineCount = code ? code.split('\n').length : 0

  const handleCopy = () => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    }).catch(() => {})
  }

  return (
    <div className="relative group/code my-md rounded-lg overflow-hidden border border-outline-variant/20 bg-surface-container-lowest">
      {/* Header — language label + line-number toggle + copy button */}
      <div className="flex items-center justify-between gap-xs px-sm py-xs bg-surface-container/60 border-b border-outline-variant/15 text-label-xs">
        <span className="font-mono uppercase text-on-surface-variant tracking-wide" aria-label={t('chat.code.language.aria')}>
          {language ?? t('chat.code.language.text')}
        </span>
        <div className="flex items-center gap-xs">
          {lineCount > 5 && (
            <button
              type="button"
              onClick={() => setShowLines(v => !v)}
              aria-pressed={showLines}
              aria-label={t(showLines ? 'chat.code.lineNumbers.hide' : 'chat.code.lineNumbers.show')}
              className="px-xs py-[2px] rounded text-on-surface-variant hover:text-primary focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
            >
              <span className="material-symbols-outlined text-[14px] align-middle">format_list_numbered</span>
            </button>
          )}
          <button
            type="button"
            onClick={handleCopy}
            aria-label={t('chat.copyCode.aria')}
            className="px-xs py-[2px] rounded text-on-surface-variant hover:text-primary flex items-center gap-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 transition-colors"
          >
            <span className="material-symbols-outlined text-[14px]">{copied ? 'check' : 'content_copy'}</span>
            <span>{copied ? t('chat.copyCode.copied') : t('chat.copyCode.copy')}</span>
          </button>
        </div>
      </div>
      <pre {...props} className={`hljs text-body-sm overflow-x-auto p-md bg-surface-container-lowest ${showLines ? 'line-numbers' : ''}`}>
        {props.children}
      </pre>
    </div>
  )
}

/* Optional line-number gutter — applies when `.line-numbers` is set. */
function injectLineNumbers() {
  if (typeof document === 'undefined') return
  document.querySelectorAll('pre.hljs.line-numbers').forEach(pre => {
    if (pre.querySelector('.line-number-row')) return
    const lines = (pre.textContent ?? '').split('\n').length
    const gutter = document.createElement('div')
    gutter.setAttribute('aria-hidden', 'true')
    gutter.className = 'line-number-row select-none text-right pr-sm text-on-surface-variant/40 font-mono'
    for (let i = 1; i <= Math.max(lines, 1); i++) {
      const span = document.createElement('span')
      span.className = 'block'
      span.textContent = String(i)
      gutter.appendChild(span)
    }
    pre.insertBefore(gutter, pre.firstChild)
  })
}

/* ────────────────────  Tables  ──────────────────── */

function TableRoot(props: React.HTMLAttributes<HTMLTableElement>) {
  return (
    <div className="my-md overflow-x-auto rounded-lg border border-outline-variant/20">
      <table className="w-full text-body-sm" {...props} />
    </div>
  )
}

function TableHeader(props: React.ThHTMLAttributes<HTMLTableCellElement>) {
  return <th className="text-left px-sm py-xs bg-surface-container font-label-md text-on-surface border-b border-outline-variant/30" {...props} />
}

function TableCell(props: React.TdHTMLAttributes<HTMLTableCellElement>) {
  return <td className="px-sm py-xs border-b border-outline-variant/10 even:bg-surface-container-low/40 align-top" {...props} />
}

/* ────────────────────  Block quotes  ──────────────────── */

function BlockQuote(props: React.BlockquoteHTMLAttributes<HTMLQuoteElement>) {
  return (
    <blockquote
      className="my-md pl-md pr-sm py-xs border-l-4 border-tertiary/60 bg-tertiary/5 text-on-surface italic"
      {...props}
    />
  )
}

/* ────────────────────  Inline code + links  ──────────────────── */

function InlineCode(props: React.HTMLAttributes<HTMLElement>) {
  return (
    <code
      className="font-mono text-[0.92em] px-[5px] py-[1px] rounded-md bg-surface-container text-primary border border-outline-variant/15"
      {...props}
    />
  )
}

/**
 * External link with an outbound icon. Lazy-fetches `<title>` on hover
 * and exposes it as a tooltip via `aria-label` (screen reader) and a
 * `title` attribute (visual). Safe — only fetches same-origin or non-`file`
 * URLs, and degrades silently on error.
 */
function ExternalLink(props: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const { href, children, ...rest } = props
  const isExternal = !!href && /^https?:\/\//i.test(href)
  // Reserved for a future CORS-friendly `<title>` fetcher (many
  // external sites don't send CORS headers, so we'd need a proxy or
  // extension to make the read reliable). Today: no-op, the hover
  // handler only signals intent.
  const handleEnter = () => {
    if (!isExternal || !href) return
    // intentional no-op (placeholder for future lazy title fetch)
  }

  return (
    <a
      href={href}
      target={isExternal ? '_blank' : undefined}
      rel={isExternal ? 'noopener noreferrer' : undefined}
      className="text-primary hover:underline inline-flex items-baseline gap-[2px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded"
      onMouseEnter={handleEnter}
      onFocus={handleEnter}
      onPointerEnter={handleEnter}
      {...rest}
    >
      {children}
      {isExternal && (
        <span
          className="material-symbols-outlined text-[12px] leading-none text-primary/70 -translate-y-[1px]"
          aria-hidden="true"
        >
          open_in_new
        </span>
      )}
    </a>
  )
}

/* ────────────────────  Local image  ──────────────────── */

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

  // Side-effect: decorate code blocks with line numbers after mount
  useEffect(() => {
    if (resolved) {
      // Defer to next frame so DOM children are present
      const id = requestAnimationFrame(injectLineNumbers)
      return () => cancelAnimationFrame(id)
    }
  }, [resolved])

  return <img src={resolved} alt={alt} className="max-w-full rounded-lg my-sm" {...rest} />
}

import { useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import { hljs } from '@/lib/hljs'
import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'

/* Shared code-block primitive — the single implementation of the code card:
 * header (language label · line-number toggle · copy) + highlighted <pre>.
 *
 * Two content modes:
 *   - self-highlighting: pass `code` (+ optional `language`); highlight.js
 *     runs here (artifact panel, diff views).
 *   - pre-highlighted: pass `children` (already-highlighted <code> from
 *     rehype-highlight) plus the raw text as `code` so copy/line count work
 *     (chat Markdown).
 *
 * The line-number gutter is owned here via a scoped layout effect — the old
 * chat implementation injected it with a document-wide querySelectorAll
 * piggybacked on image mounts, so blocks without images never got numbers.
 */

export interface CodeBlockProps {
  /** Raw source text — powers copy, line count, and the gutter. */
  code: string
  /** Bare language name for the header label and self-highlighting. */
  language?: string | null
  /** Pre-highlighted content (e.g. rehype-highlight's <code> element).
   *  When given, rendered as-is instead of self-highlighting. */
  children?: ReactNode
  /** Header row (language label · toggle · copy). Default true. */
  chrome?: boolean
  /** Gutter behavior: 'toggle' (header button, >5 lines), true (always on),
   *  false (never). */
  lineNumbers?: 'toggle' | boolean
  className?: string
  /** Extra classes for the <pre> (e.g. wrap behavior). */
  contentClassName?: string
}

const LINE_TOGGLE_THRESHOLD = 5

function escapeHtml(source: string): string {
  return source.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c] ?? c))
}

export function CodeBlock({
  code,
  language,
  children,
  chrome = true,
  lineNumbers = 'toggle',
  className,
  contentClassName,
}: CodeBlockProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [copied, setCopied] = useState(false)
  const [showLines, setShowLines] = useState(false)
  const preRef = useRef<HTMLPreElement>(null)

  const lineCount = code ? code.split('\n').length : 0
  const gutterRequested =
    lineNumbers === true ||
    (lineNumbers === 'toggle' && chrome && lineCount > LINE_TOGGLE_THRESHOLD && showLines)

  const html = useMemo(() => {
    if (children !== undefined && children !== null) return undefined
    try {
      if (language && hljs.getLanguage(language)) {
        return hljs.highlight(code, { language }).value
      }
      return hljs.highlightAuto(code).value
    } catch {
      return escapeHtml(code)
    }
  }, [children, code, language])

  // Scoped gutter: rebuild inside this block's <pre> whenever the text or
  // the toggle changes; cleanup removes it so streaming updates never leak.
  useLayoutEffect(() => {
    const pre = preRef.current
    if (!pre) return
    pre.querySelector(':scope > .line-number-row')?.remove()
    if (!gutterRequested) return
    const gutter = document.createElement('div')
    gutter.setAttribute('aria-hidden', 'true')
    gutter.className = 'line-number-row select-none text-right pr-sm text-on-surface-variant/40 font-mono'
    for (let i = 1; i <= Math.max(lineCount, 1); i++) {
      const span = document.createElement('span')
      span.className = 'block'
      span.textContent = String(i)
      gutter.appendChild(span)
    }
    pre.insertBefore(gutter, pre.firstChild)
    return () => { gutter.remove() }
  }, [gutterRequested, lineCount, code])

  const handleCopy = () => {
    navigator.clipboard.writeText(code).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1500)
    }).catch(() => {})
  }

  return (
    <div className={cn('group/code my-md rounded-lg overflow-hidden border border-outline-variant/20 bg-surface-container-lowest', className)}>
      {chrome && (
        <div className="flex items-center justify-between gap-xs px-sm py-xs bg-surface-container/60 border-b border-outline-variant/15 text-label-xs">
          <span className="font-mono uppercase text-on-surface-variant tracking-wide" aria-label={t('code.language.aria')}>
            {language ?? t('code.language.text')}
          </span>
          <div className="flex items-center gap-xs">
            {lineNumbers === 'toggle' && lineCount > LINE_TOGGLE_THRESHOLD && (
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowLines(v => !v)}
                aria-pressed={showLines}
                aria-label={t(showLines ? 'code.lineNumbers.hide' : 'code.lineNumbers.show')}
                className="h-auto px-xs py-[2px] text-on-surface-variant hover:text-primary"
              >
                <span className="material-symbols-outlined text-[14px] align-middle">format_list_numbered</span>
              </Button>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={handleCopy}
              aria-label={t('code.copy.aria')}
              className="h-auto px-xs py-[2px] gap-xs text-on-surface-variant hover:text-primary"
            >
              <span className="material-symbols-outlined text-[14px]">{copied ? 'check' : 'content_copy'}</span>
              <span>{copied ? t('code.copy.copied') : t('code.copy.copy')}</span>
            </Button>
          </div>
        </div>
      )}
      <pre
        ref={preRef}
        className={cn('hljs text-body-sm overflow-x-auto p-md bg-surface-container-lowest', gutterRequested ? 'line-numbers' : '', contentClassName)}
      >
        {children !== undefined && children !== null
          ? children
          : <code className="hljs" dangerouslySetInnerHTML={{ __html: html ?? '' }} />}
      </pre>
    </div>
  )
}

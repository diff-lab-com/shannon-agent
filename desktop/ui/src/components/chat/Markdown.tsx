import { useState, useEffect, memo, createContext, useContext, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import remarkMath from 'remark-math'
import rehypeHighlight from 'rehype-highlight'
import rehypeSanitize, { defaultSchema } from 'rehype-sanitize'
// B2 (§4-14): katex runs LAST — after sanitize — so its generated markup is
// never stripped and the sanitize schema needs no widening for it.
import rehypeKatex from 'rehype-katex'
import 'katex/dist/katex.min.css'
import { convertFileSrc } from '@tauri-apps/api/core'
import { Chart, parseChartSpec } from '@/components/chat/Chart'
import { CodeBlock as SharedCodeBlock } from '@/components/code/CodeBlock'
import { Button } from '@/components/ui/button'
import { FileRefChip } from '@/components/shared/FileRefChip'
import { matchSourceLine, SourcePill } from '@/components/chat/SourcePill'
import { basenameOf, looksLikeFilePath } from '@/lib/fileRefs'
import { isGatedRemoteImageSrc, remoteImageHost } from '@/lib/remoteImages'
import { useRemoteImagesAllowed } from '@/hooks/useRemoteImagesAllowed'

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
  /** Batch D5: when set, GFM task-list checkboxes render enabled and the
   *  callback fires with the DOM input (the caller resolves which checklist
   *  item it is — e.g. by DOM order inside its own container). */
  onCheckboxToggle?: (input: HTMLInputElement) => void
}

export const Markdown = memo(function Markdown({ children, className, onCheckboxToggle }: MarkdownProps) {
  return (
    <div className={className}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[
          rehypeHighlight,
          [rehypeSanitize, sanitizeSchema],
          // B2 §4-14: math → KaTeX. Must stay AFTER rehype-sanitize so the
          // KaTeX markup (and its MathML twin) is never stripped; the math
          // *input* (`code.language-math`) already survives the schema.
          [rehypeKatex, { throwOnError: false }],
        ]}
        components={{
          pre: PreOrChart,
          // office Wave 3 C8: a B3 source line (`[Source: <name>] (<target>)`)
          // on its own paragraph renders as a citation pill instead of text.
          p: SourceParagraph,
          img: LocalImage,
          table: TableRoot,
          th: TableHeader,
          td: TableCell,
          blockquote: BlockQuote,
          a: MarkdownLink,
          code: InlineCode,
          li: FootnoteListItem,
          section: FootnotesSection,
          ...(onCheckboxToggle
            ? {
                input: (props: React.InputHTMLAttributes<HTMLInputElement>) => (
                  <input
                    {...props}
                    disabled={false}
                    onChange={e => onCheckboxToggle(e.currentTarget)}
                  />
                ),
              }
            : {}),
        }}
      >
        {children}
      </ReactMarkdown>
    </div>
  )
})

/* ────────────────────  Code blocks & charts  ──────────────────── */

/** Dispatches `language-chart` to the chart renderer; display math (which
 *  rehype-katex leaves in place inside this `pre`) skips the code-block
 *  chrome; everything else goes to the generic CodeBlock. */
function PreOrChart(props: React.HTMLAttributes<HTMLPreElement>) {
  const intl = useIntl()
  const child = Array.isArray(props.children) ? props.children[0] : props.children
  if (child && typeof child === 'object' && 'props' in child) {
    const codeProps = (child as { props: { className?: string; children?: ReactNode } }).props
    if (/language-chart/.test(codeProps.className ?? '')) {
      const raw = extractText(codeProps.children)
      const spec = parseChartSpec(raw)
      if (spec) return <Chart spec={spec} />
      return (
        <div className="my-md p-sm rounded-lg bg-error-container/20 border border-error/30 text-label-sm text-error">
          <span className="material-symbols-outlined icon-sm align-middle mr-xs">error</span>
          {intl.formatMessage({ id: 'chat.chart.invalidSpec' })}
        </div>
      )
    }
    // B2 §4-14: `$$…$$` flow math keeps its <pre> wrapper, but rehype-katex
    // has already replaced the <code> with KaTeX markup — render it as a
    // bare scrollable block instead of dressing it in code-chrome.
    if (/\bkatex\b/.test(codeProps.className ?? '')) {
      return <div className="my-md overflow-x-auto">{props.children}</div>
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

/**
 * US4 (plan Task 3.1, direction A — user-initiated only): every chat fenced
 * code block carries a "run in terminal" action in the block's header
 * chrome, next to copy. The click dispatches the `shannon:terminal-run`
 * window CustomEvent with the raw code; the integrated terminal panel (the
 * owner of the drawer's open/spawn state) listens while mounted and does
 * the actual open/spawn/write. The render path stays cheap — the event is
 * only built on click, and the Markdown memo means idle messages re-render
 * nothing.
 */
const TERMINAL_RUN_EVENT = 'shannon:terminal-run'

function CodeBlock(props: { children?: ReactNode } & React.HTMLAttributes<HTMLPreElement>) {
  const intl = useIntl()
  const codeProps = getCodeChildProps(props.children)
  const code = extractText(codeProps?.children)
  const language = extractLanguage(codeProps?.className)
  // The shared primitive owns the header (language · line-number toggle ·
  // copy · run-in-terminal) and the gutter; the already-highlighted <code>
  // from rehype passes through as children so streaming re-renders stay cheap.
  const runLabel = intl.formatMessage({ id: 'terminal.runInTerminal.title' })
  const runInTerminal = () => {
    // The fence's parsed text ends with a newline; the panel appends the
    // execution newline itself, so hand over the code without it.
    window.dispatchEvent(new CustomEvent(TERMINAL_RUN_EVENT, {
      detail: { code: code.replace(/\n+$/, '') },
    }))
  }
  return (
    <SharedCodeBlock
      code={code}
      language={language}
      lineNumbers="toggle"
      actions={
        <Button
          variant="ghost"
          size="sm"
          onClick={runInTerminal}
          aria-label={runLabel}
          title={runLabel}
          className="h-auto px-xs py-[2px] gap-xs text-on-surface-variant hover:text-primary"
        >
          <span className="material-symbols-outlined icon-xs" aria-hidden="true">terminal</span>
        </Button>
      }
    >
      {props.children}
    </SharedCodeBlock>
  )
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

/** C8 guard: true while rendering inside a blockquote — source lines quoted
 *  verbatim must stay literal markdown, not turn into clickable pills. */
const InsideQuoteContext = createContext(false)

function BlockQuote(props: React.BlockquoteHTMLAttributes<HTMLQuoteElement>) {
  return (
    <InsideQuoteContext.Provider value={true}>
      <blockquote
        className="my-md pl-md pr-sm py-xs border-l-4 border-tertiary/60 bg-tertiary/5 text-on-surface italic"
        {...props}
      />
    </InsideQuoteContext.Provider>
  )
}

/* ────────────────────  Source pills (office Wave 3 C8)  ──────────────────── */

/**
 * Paragraph override implementing the C8 citation pill. Chosen intrusion
 * point: the rendered-paragraph level, NOT string preprocessing or a remark
 * plugin — a whole-line `[Source: <name>] (<target>)` parses as plain
 * paragraph text anyway (the space after `]` blocks link parsing), so
 * inspecting the extracted paragraph text catches exactly the injected
 * lines with zero sanitize-schema changes and no risk to code spans/fences
 * (those never flow through `p`). Every non-empty line of the paragraph
 * must match the B3 convention; any other line keeps the `<p>` untouched.
 * Inside blockquotes (context guard above) the line stays literal.
 */
function SourceParagraph(props: React.HTMLAttributes<HTMLParagraphElement>) {
  const inQuote = useContext(InsideQuoteContext)
  const text = extractText(props.children)
  const sources: { name: string; target: string }[] = []
  for (const line of text.split('\n')) {
    if (line.trim() === '') continue
    const m = matchSourceLine(line)
    if (!m) return <p {...stripNodeProp(props as unknown as Record<string, unknown>)} />
    sources.push(m)
  }
  if (inQuote || sources.length === 0) {
    return <p {...stripNodeProp(props as unknown as Record<string, unknown>)} />
  }
  return (
    <div className="my-sm flex flex-wrap items-center gap-xs" data-testid="source-pill-row">
      {sources.map((s, i) => (
        <SourcePill key={`${s.target}-${i}`} name={s.name} target={s.target} />
      ))}
    </div>
  )
}

/* ────────────────────  Inline code  ──────────────────── */

function InlineCode(props: React.HTMLAttributes<HTMLElement>) {
  // P0-B: a token that looks like a file path becomes a FileRefChip — it
  // highlights only after the backend existence probe confirms it, and
  // degrades to this exact inline-code style otherwise.
  const text = extractText(props.children)
  if (text && looksLikeFilePath(text)) {
    return <FileRefChip raw={text} />
  }
  return (
    <code
      className="font-mono text-[0.92em] px-[5px] py-[1px] rounded-md bg-surface-container text-primary border border-outline-variant/15"
      {...props}
    />
  )
}

/* ────────────────────  Links + GFM footnotes  ──────────────────── */

/** rehype-sanitize clobber-prefixes `id`/`aria-*` with `user-content-`, and
 *  remark-gfm's footnote anchors *already* ship that prefix — the result is
 *  a doubled `user-content-user-content-…` that no href points at (P1-6).
 *  Strip one copy so `#user-content-fn-x` ⇄ `li#user-content-fn-x` meet. */
const CLOBBER_PREFIX = 'user-content-'

function normalizeClobberedId(id?: string): string | undefined {
  return id && id.startsWith(`${CLOBBER_PREFIX}${CLOBBER_PREFIX}`)
    ? id.slice(CLOBBER_PREFIX.length)
    : id
}

/** react-markdown hands every `node` prop down to component overrides;
 *  spreading it onto a DOM element leaks `node="[object Object]"`. */
function stripNodeProp<T extends Record<string, unknown>>(props: T): Record<string, unknown> {
  const { node: _node, ...rest } = props
  return rest
}

/**
 * P1-6: GFM footnote list items (`section[data-footnotes] > ol > li`).
 * Only fixes the doubled clobber prefix on footnote ids — regular list
 * items pass through untouched.
 */
function FootnoteListItem(props: React.LiHTMLAttributes<HTMLLIElement>) {
  const { id, ...rest } = props
  return <li id={normalizeClobberedId(id)} {...stripNodeProp(rest as unknown as Record<string, unknown>)} />
}

/**
 * P1-6: the GFM footnotes section, styled like the old FootnoteMarkdown
 * footer (top border, muted small text, ordered list with ↩ back-links).
 * The localized aria-label supersedes the English sr-only heading inside.
 */
function FootnotesSection(props: React.HTMLAttributes<HTMLElement>) {
  const intl = useIntl()
  const rest = stripNodeProp(props as unknown as Record<string, unknown>)
  return (
    <section
      {...rest}
      aria-label={intl.formatMessage({ id: 'chat.footnotes.section' })}
      className="mt-lg pt-sm border-t border-outline-variant/30 text-body-sm text-on-surface-variant [&_ol]:list-decimal [&_ol]:pl-md [&_ol]:space-y-xs"
    />
  )
}

/**
 * Link override covering three flavors:
 *  - GFM footnote refs (`sup > a[data-footnote-ref]`) → superscript badge
 *    in the old FootnoteMarkdown visual language;
 *  - GFM footnote back-refs (`a[data-footnote-backref]`) → localized ↩;
 *  - everything else → the external-link treatment (P0-A interceptor owns
 *    the actual opening).
 */
function MarkdownLink(props: React.AnchorHTMLAttributes<HTMLAnchorElement>) {
  const intl = useIntl()
  const { href, children, ...rest } = props
  const raw = rest as Record<string, unknown>
  const id = normalizeClobberedId(props.id)

  if (raw['data-footnote-ref']) {
    // Spread first, then set the fields we normalize — `rest` still carries
    // the doubled clobber-prefixed id from the sanitizer.
    const { 'data-footnote-ref': _ref, node: _node, ...anchorRest } = raw
    return (
      <a
        {...anchorRest}
        href={href}
        id={id}
        aria-label={`Footnote ${extractText(children)}`}
        className="inline-flex items-center mx-[1px] px-[3px] h-[16px] rounded-full bg-primary text-on-primary text-label-2xs font-bold leading-none align-super hover:bg-secondary hover:text-on-secondary transition-colors no-underline"
      >
        {children}
      </a>
    )
  }

  if ('data-footnote-backref' in raw) {
    const { 'data-footnote-backref': _back, node: _node, ...anchorRest } = raw
    // href looks like `#user-content-fnref-<id>` — recover the footnote id
    // for the localized back-link label (replaces mdast's English default).
    const refId = href?.replace(/^#user-content-fnref-/, '') ?? ''
    return (
      <a
        {...anchorRest}
        href={href}
        aria-label={intl.formatMessage({ id: 'chat.footnotes.back' }, { id: refId })}
        className="ml-xs text-label-xs text-primary hover:underline"
      >
        {children}
      </a>
    )
  }

  const isExternal = !!href && /^https?:\/\//i.test(href)

  return (
    <a
      href={href}
      target={isExternal ? '_blank' : undefined}
      rel={isExternal ? 'noopener noreferrer' : undefined}
      className="text-link hover:underline inline-flex items-baseline gap-[2px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30 rounded-sm"
      {...stripNodeProp(rest)}
    >
      {children}
      {isExternal && (
        <span
          className="material-symbols-outlined icon-xs leading-none text-primary/70 -translate-y-[1px]"
          aria-hidden="true"
        >
          open_in_new
        </span>
      )}
    </a>
  )
}

/* ────────────────────  Local image  ──────────────────── */

/** P2-15 (§4-16): markdown images load lazily, keep a max height, and open
 *  in the Dock's image tab on click — the same `shannon:open-artifact-file`
 *  pipeline FileRefChip uses (ArtifactLinkHost owns the receiving end).
 *
 *  P2-4 (R9-④): remote http(s) sources render a gate placeholder instead of
 *  the image — no request leaves the webview until the user admits that one
 *  src (or flips the global switch in AdvancedSettings). Local sources
 *  (file://, /abs, asset protocol) pass through exactly as before. */
function LocalImage(props: React.ImgHTMLAttributes<HTMLImageElement>) {
  const intl = useIntl()
  const { src, alt, ...rest } = props
  const [resolved, setResolved] = useState(src)
  const [localPath, setLocalPath] = useState<string | null>(null)
  // Per-image admits, keyed by src: a Set (not a boolean) so a later edit of
  // the same element's src can't inherit an allow the user never gave it.
  const [admitted, setAdmitted] = useState<Set<string>>(() => new Set())
  const allowAllRemote = useRemoteImagesAllowed()
  useEffect(() => {
    if (typeof src !== 'string') { setResolved(src); setLocalPath(null); return }
    let mounted = true
    try {
      if (src.startsWith('file://')) {
        const path = src.replace(/^file:\/\//, '')
        const converted = convertFileSrc(path)
        if (mounted) { setResolved(converted); setLocalPath(path) }
      } else if (src.startsWith('/') && !src.startsWith('//')) {
        const converted = convertFileSrc(src)
        if (mounted) { setResolved(converted); setLocalPath(src) }
      } else {
        if (mounted) { setResolved(src); setLocalPath(null) }
      }
    } catch {
      if (mounted) { setResolved(src); setLocalPath(null) }
    }
    return () => { mounted = false }
  }, [src])

  // The gate reads the raw src (not `resolved`): remote sources are exactly
  // the ones the resolver leaves untouched, so this holds before and after
  // the effect settles.
  const srcStr = typeof src === 'string' ? src : null
  if (srcStr && isGatedRemoteImageSrc(srcStr) && !allowAllRemote && !admitted.has(srcStr)) {
    return <RemoteImageGate src={srcStr} alt={alt} onAllow={() => setAdmitted(prev => new Set(prev).add(srcStr))} />
  }

  const openLabel = intl.formatMessage({ id: 'chat.markdown.image.open' })

  if (!localPath) {
    // The line-number gutter used to be injected from this effect via a
    // document-wide scan — it now lives in the shared code-block primitive,
    // scoped per block (components/code/CodeBlock.tsx). Admitted remote
    // images land here too, keeping their pre-gate behavior (lazy, plain).
    return <img src={resolved} alt={alt} loading="lazy" className="max-w-full max-h-96 object-contain rounded-lg my-sm" {...rest} />
  }

  const openInDock = () => {
    window.dispatchEvent(new CustomEvent('shannon:open-artifact-file', { detail: { path: localPath } }))
  }

  return (
    <img
      src={resolved}
      alt={alt}
      loading="lazy"
      role="button"
      tabIndex={0}
      title={openLabel}
      aria-label={alt ? `${alt} — ${openLabel}` : openLabel}
      onClick={openInDock}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault()
          openInDock()
        }
      }}
      className="max-w-full max-h-96 object-contain rounded-lg my-sm cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/30"
      {...rest}
    />
  )
}

/** P2-4 (R9-④) placeholder for a not-yet-admitted remote image. Renders
 *  zero network surface (no `<img src>` at all), shows the host the request
 *  would go to plus the alt/filename, and a button that admits exactly this
 *  src. The alt stays visible text so the content survives for readers. */
function RemoteImageGate({ src, alt, onAllow }: { src: string; alt?: ReactNode; onAllow: () => void }) {
  const intl = useIntl()
  const host = remoteImageHost(src)
  // alt first (author intent); otherwise the filename from the URL path.
  let label = typeof alt === 'string' ? alt : ''
  if (!label) {
    try {
      const name = basenameOf(decodeURIComponent(new URL(src, 'https://shannon.invalid').pathname))
      if (name && name !== '/') label = name
    } catch { /* keep empty — the host line alone is fine */ }
  }
  return (
    <span
      data-testid="remote-image-gate"
      title={intl.formatMessage({ id: 'chat.markdown.image.gated.why' })}
      className="my-sm flex max-w-full flex-wrap items-center gap-xs rounded-lg border border-outline-variant/30 bg-surface-container-low px-sm py-xs text-on-surface-variant"
    >
      <span className="material-symbols-outlined icon-sm shrink-0" aria-hidden="true">hide_image</span>
      <span className="min-w-0 flex-1 truncate text-label-sm">
        <span className="font-medium">{host}</span>
        {label ? ` · ${label}` : ''}
      </span>
      <Button
        variant="outline"
        size="sm"
        className="shrink-0"
        onClick={onAllow}
        aria-label={intl.formatMessage({ id: 'chat.markdown.image.gated.allowAria' }, { host })}
      >
        <span className="material-symbols-outlined icon-xs" aria-hidden="true">image</span>
        {intl.formatMessage({ id: 'chat.markdown.image.gated.allow' })}
      </Button>
    </span>
  )
}

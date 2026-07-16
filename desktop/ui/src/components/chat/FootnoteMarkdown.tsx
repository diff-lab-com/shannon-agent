import { useMemo, memo, useId, type ReactNode } from 'react'
import { useIntl } from 'react-intl'
import { Markdown } from '@/components/chat/Markdown'

interface FootnoteMarkdownProps {
  children: string
  className?: string
}

interface ParsedFootnotes {
  body: string
  definitions: { id: string; text: string }[]
}

const DEFINITION_RE = /^ {0,3}\[\^([^\]]+)\]:\s+(.+)$/gm

function parseFootnotes(input: string): ParsedFootnotes {
  const definitions: { id: string; text: string }[] = []
  let body = input.replace(DEFINITION_RE, (_match, id: string, text: string) => {
    definitions.push({ id: id.trim(), text: text.trim() })
    return ''
  })
  body = body.replace(/\n{3,}/g, '\n\n').trim() + '\n'
  return { body, definitions }
}

export const FootnoteMarkdown = memo(function FootnoteMarkdown({
  children,
  className,
}: FootnoteMarkdownProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const listId = useId().replace(/[:]/g, '')

  const { body, definitions, hasRefs } = useMemo(() => {
    const parsed = parseFootnotes(children)
    const hasRefs = /\[\^[^\]]+\]/.test(parsed.body)
    return { ...parsed, hasRefs }
  }, [children])

  if (!hasRefs || definitions.length === 0) {
    return <Markdown className={className}>{children}</Markdown>
  }

  return (
    <div className={className}>
      <BodyWithFootnoteRefs
        body={body}
        listId={listId}
      />
      <footer
        className="mt-lg pt-sm border-t border-outline-variant/30 text-body-sm text-on-surface-variant"
        aria-label={t('chat.footnotes.section')}
      >
        <ol className="space-y-xs list-decimal pl-md">
          {definitions.map((def) => (
            <li
              key={def.id}
              id={`fn-${listId}-${def.id}`}
              className="leading-relaxed"
            >
              <span className="align-super text-[10px] font-bold text-primary mr-xs">^{def.id}</span>
              <span>{def.text}</span>
              <a
                href={`#fnref-${listId}-${def.id}`}
                className="ml-xs text-[11px] text-primary hover:underline"
                aria-label={intl.formatMessage({ id: 'chat.footnotes.back' }, { id: def.id })}
              >
                ↩
              </a>
            </li>
          ))}
        </ol>
      </footer>
    </div>
  )
})

interface BodyProps {
  body: string
  listId: string
}

function BodyWithFootnoteRefs({ body, listId }: BodyProps): ReactNode {
  const segments = useMemo(() => splitByFootnoteRefs(body), [body])
  return (
    <div>
      {segments.map((seg, i) =>
        seg.kind === 'text' ? (
          <Markdown key={i}>{seg.value}</Markdown>
        ) : (
          <a
            key={i}
            href={`#fn-${listId}-${seg.id}`}
            id={`fnref-${listId}-${seg.id}`}
            className="inline-flex items-center align-super mx-[1px] px-[3px] h-[16px] rounded-full bg-secondary-container text-on-secondary-container text-[10px] font-bold leading-none hover:bg-secondary hover:text-on-secondary transition-colors no-underline"
            aria-label={`Footnote ${seg.id}`}
          >
            {seg.id}
          </a>
        ),
      )}
    </div>
  )
}

const REF_PATTERN = /\[\^([^\]]+)\]/g

type Segment = { kind: 'text'; value: string } | { kind: 'ref'; id: string }

function splitByFootnoteRefs(text: string): Segment[] {
  const out: Segment[] = []
  let last = 0
  for (const match of text.matchAll(REF_PATTERN)) {
    const idx = match.index ?? 0
    if (idx > last) out.push({ kind: 'text', value: text.slice(last, idx) })
    out.push({ kind: 'ref', id: match[1].trim() })
    last = idx + match[0].length
  }
  if (last < text.length) out.push({ kind: 'text', value: text.slice(last) })
  return out
}

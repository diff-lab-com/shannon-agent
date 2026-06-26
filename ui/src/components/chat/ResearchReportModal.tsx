import { useEffect, useMemo, useRef, useState, memo, useCallback } from 'react'
import { useIntl } from 'react-intl'
import { toast } from 'sonner'
import { useModalFocus } from '@/hooks/useModalFocus'
import { Markdown } from '@/components/chat/Markdown'
import { Button } from '@/components/ui/button'
import { buildPrintStyles } from '@/lib/printStyles'
import type { ResearchReport } from '@/types'

interface ResearchReportModalProps {
  report: ResearchReport
  open: boolean
  onClose: () => void
}

export const ResearchReportModal = memo(function ResearchReportModal({
  report,
  open,
  onClose,
}: ResearchReportModalProps) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const modalRef = useRef<HTMLDivElement>(null)
  const citationsRef = useRef<HTMLDivElement>(null)
  const [activeCitation, setActiveCitation] = useState<number | null>(null)

  useModalFocus(open, modalRef)

  useEffect(() => {
    if (!open) return
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [open, onClose])

  const handleCitationClick = (id: number) => {
    setActiveCitation(id)
    const el = citationsRef.current?.querySelector<HTMLElement>(`[data-citation-id="${id}"]`)
    el?.scrollIntoView({ behavior: 'smooth', block: 'center' })
  }

  const handleExportPdf = useCallback(() => {
    try {
      const printWindow = window.open('', '_blank', 'width=900,height=700')
      if (!printWindow) {
        toast.error(t('chat.report.printFailed'), { description: t('chat.report.popupBlocked') })
        return
      }
      const doc = printWindow.document
      doc.title = report.title
      doc.head.appendChild(buildStyleElement(doc))
      doc.body.appendChild(buildReportDom(doc, report))
      printWindow.focus()
      setTimeout(() => printWindow.print(), 250)
    } catch (e) {
      toast.error(t('chat.report.printFailed'), { description: String(e) })
    }
  }, [report, t])

  if (!open) return null

  return (
    <div
      ref={modalRef}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-md"
      onClick={onClose}
      role="dialog"
      aria-modal="true"
      aria-label={report.title}
    >
      <div
        className="bg-surface-container-lowest rounded-2xl shadow-2xl w-full max-w-6xl h-[85vh] flex flex-col overflow-hidden"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-center justify-between gap-md px-lg py-md border-b border-outline-variant/30">
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-sm mb-xs">
              <span className="material-symbols-outlined text-primary text-[20px]">article</span>
              <span className="text-label-sm text-on-surface-variant uppercase tracking-wide font-bold">
                {t('chat.report.badge')}
              </span>
            </div>
            <h2 className="text-headline-sm font-bold text-on-surface truncate">{report.title}</h2>
          </div>
          <div className="flex items-center gap-xs shrink-0">
            <Button
              aria-label={t('chat.report.export.aria')}
              onClick={handleExportPdf}
              className="rounded-lg px-sm py-xs hover:bg-surface-container text-on-surface-variant"
              title={t('chat.report.export')}
            >
              <span className="material-symbols-outlined text-[18px]">picture_as_pdf</span>
              <span className="text-label-sm ml-xs hidden sm:inline">{t('chat.report.export')}</span>
            </Button>
            <Button
              aria-label={t('chat.report.close.aria')}
              onClick={onClose}
              className="rounded-full p-sm hover:bg-surface-container"
            >
              <span className="material-symbols-outlined">close</span>
            </Button>
          </div>
        </header>

        <div className="flex-1 grid grid-cols-1 lg:grid-cols-[1.6fr_1fr] overflow-hidden">
          <div className="overflow-y-auto px-lg py-md border-r border-outline-variant/20">
            <section className="mb-lg">
              <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide mb-xs">
                {t('chat.report.summary')}
              </h3>
              <p className="text-body-md text-on-surface leading-relaxed">{report.summary}</p>
            </section>

            {report.sections.map((s, i) => (
              <section key={i} className="mb-lg">
                <h3 className="text-title-md font-bold text-on-surface mb-xs">{s.heading}</h3>
                <div className="text-body-md text-on-surface leading-relaxed prose prose-sm max-w-none prose-p:my-2 prose-pre:bg-surface-container prose-pre:p-md prose-pre:rounded-lg prose-code:text-primary prose-code:before:content-[''] prose-code:after:content-['']">
                  <CitationMarkdown onCitationClick={handleCitationClick}>
                    {s.body}
                  </CitationMarkdown>
                </div>
              </section>
            ))}
          </div>

          <div ref={citationsRef} className="overflow-y-auto px-lg py-md bg-surface-container-low/30">
            <div className="flex items-center justify-between mb-md">
              <h3 className="text-label-lg font-bold text-on-surface-variant uppercase tracking-wide">
                {t('chat.report.citations')}
              </h3>
              <span className="text-label-sm text-on-surface-variant">
                {report.citations.length}
              </span>
            </div>
            {report.citations.length === 0 ? (
              <p className="text-body-sm text-on-surface-variant italic">
                {t('chat.report.noCitations')}
              </p>
            ) : (
              <ul className="space-y-sm">
                {report.citations.map((c) => (
                  <li
                    key={c.id}
                    data-citation-id={c.id}
                    className={`rounded-lg p-sm border transition-colors ${
                      activeCitation === c.id
                        ? 'border-primary/50 bg-primary-container/20'
                        : 'border-outline-variant/20 bg-surface-container-lowest/60'
                    }`}
                  >
                    <div className="flex items-start gap-xs">
                      <span className="shrink-0 inline-flex items-center justify-center w-5 h-5 rounded-full bg-primary-container text-on-primary-container text-label-xs font-bold">
                        {c.id}
                      </span>
                      <div className="flex-1 min-w-0">
                        <div className="text-label-md font-bold text-on-surface break-words">
                          {c.title}
                        </div>
                        {c.source && (
                          <div className="text-label-xs text-on-surface-variant mt-[2px]">
                            {c.source}
                          </div>
                        )}
                        {c.snippet && (
                          <p className="text-body-sm text-on-surface-variant mt-xs line-clamp-3">
                            {c.snippet}
                          </p>
                        )}
                        {c.url && (
                          <a
                            href={c.url}
                            target="_blank"
                            rel="noreferrer"
                            className="inline-flex items-center gap-xs text-label-sm text-primary hover:underline mt-xs"
                          >
                            <span className="material-symbols-outlined text-[14px]">open_in_new</span>
                            {t('chat.report.openSource')}
                          </a>
                        )}
                      </div>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>

        <footer className="flex items-center justify-between px-lg py-sm border-t border-outline-variant/30 bg-surface-container-low/20">
          <span className="text-label-xs text-on-surface-variant">
            {intl.formatMessage(
              { id: 'chat.report.generatedAt' },
              { ts: new Date(report.generated_at).toLocaleString(intl.locale) },
            )}
          </span>
          <span className="text-label-xs text-on-surface-variant">
            {report.sections.length} {t('chat.report.sections')} · {report.citations.length} {t('chat.report.citations')}
          </span>
        </footer>
      </div>
    </div>
  )
})

const CITATION_PATTERN = /\[(\d+)\]/g

interface CitationMarkdownProps {
  children: string
  onCitationClick: (id: number) => void
}

function CitationMarkdown({ children, onCitationClick }: CitationMarkdownProps) {
  const segments = useMemo(() => splitByCitations(children), [children])
  return (
    <>
      {segments.map((seg, i) =>
        seg.kind === 'text' ? (
          <Markdown key={i}>{seg.value}</Markdown>
        ) : (
          <button
            key={i}
            type="button"
            onClick={() => onCitationClick(seg.id)}
            className="inline-flex items-center align-super mx-[1px] px-[3px] h-[16px] rounded-full bg-primary-container text-on-primary-container text-[10px] font-bold leading-none hover:bg-primary hover:text-on-primary transition-colors cursor-pointer"
            aria-label={`Citation ${seg.id}`}
          >
            {seg.id}
          </button>
        ),
      )}
    </>
  )
}

type Segment = { kind: 'text'; value: string } | { kind: 'citation'; id: number }

function splitByCitations(text: string): Segment[] {
  const out: Segment[] = []
  let last = 0
  for (const match of text.matchAll(CITATION_PATTERN)) {
    const idx = match.index ?? 0
    if (idx > last) out.push({ kind: 'text', value: text.slice(last, idx) })
    out.push({ kind: 'citation', id: Number(match[1]) })
    last = idx + match[0].length
  }
  if (last < text.length) out.push({ kind: 'text', value: text.slice(last) })
  return out
}

const CITATION_PRINT_PATTERN = /\[(\d+)\]/g

function stripCitations(text: string): string {
  return text.replace(CITATION_PRINT_PATTERN, (_m, n: string) => `[${n}]`)
}

function buildStyleElement(doc: Document): HTMLStyleElement {
  const style = doc.createElement('style')
  style.textContent = buildPrintStyles({ variant: 'report' })
  return style
}

function buildReportDom(doc: Document, report: ResearchReport): HTMLElement {
  const root = doc.createElement('div')

  const h1 = doc.createElement('h1')
  h1.textContent = report.title
  root.appendChild(h1)

  const meta = doc.createElement('div')
  meta.className = 'meta'
  meta.textContent = new Date(report.generated_at).toLocaleString()
  root.appendChild(meta)

  const summary = doc.createElement('p')
  summary.className = 'summary'
  summary.textContent = report.summary
  root.appendChild(summary)

  for (const s of report.sections) {
    const h2 = doc.createElement('h2')
    h2.textContent = s.heading
    root.appendChild(h2)
    appendParagraphsWithCitations(doc, root, s.body)
  }

  if (report.citations.length > 0) {
    const h2 = doc.createElement('h2')
    h2.textContent = 'Citations'
    root.appendChild(h2)
    const ol = doc.createElement('div')
    ol.className = 'citations'
    for (const c of report.citations) {
      const row = doc.createElement('div')
      row.className = 'citation'
      const num = doc.createElement('span')
      num.className = 'num'
      num.textContent = `[${c.id}]`
      row.appendChild(num)
      if (c.url) {
        const a = doc.createElement('a')
        a.href = c.url
        a.target = '_blank'
        a.rel = 'noreferrer'
        a.textContent = c.title
        row.appendChild(a)
      } else {
        const titleSpan = doc.createElement('span')
        titleSpan.textContent = c.title
        row.appendChild(titleSpan)
      }
      if (c.source) {
        const src = doc.createElement('span')
        src.textContent = ` — ${c.source}`
        row.appendChild(src)
      }
      ol.appendChild(row)
    }
    root.appendChild(ol)
  }

  return root
}

function appendParagraphsWithCitations(doc: Document, parent: HTMLElement, body: string): void {
  for (const paragraph of body.split(/\n{2,}/)) {
    const p = doc.createElement('p')
    p.textContent = stripCitations(paragraph.trim())
    parent.appendChild(p)
  }
}

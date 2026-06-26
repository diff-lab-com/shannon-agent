// Unified-diff renderer with per-hunk accept/reject controls (P1.1 M1 Day 3).
//
// Hunks come from the shared `computeHunks` pure function (lib/diff-merge.ts),
// so the merge logic that runs on Apply is consistent with what the user
// reviewed here. The component is controlled — caller owns `decisions` state
// and supplies an `onToggleHunk(id)` callback. Clicking the hunk header row
// cycles the decision: pending → accept → reject → pending.
//
// The viewer is presentational — fetch + apply lives in the caller.

import { Fragment, useMemo } from 'react'
import { useIntl } from 'react-intl'
import type { FileDiff } from '@/types'
import { computeHunks, type HunkDecision } from '@/lib/diff-merge'
import { highlightLines, resolveDiffLang } from '@/lib/diff-highlight'
import 'highlight.js/styles/github.css'

interface DiffViewerProps {
  diff: FileDiff
  /** Hunk id → decision. Hunks absent from the map are treated as pending. */
  decisions: Map<string, HunkDecision>
  onToggleHunk?: (hunkId: string) => void
  className?: string
}

type LineKind = 'context' | 'add' | 'del'

interface FlatLine {
  kind: LineKind
  oldNo: number | null
  newNo: number | null
  text: string
  /** Hunk id this line belongs to (null for context lines outside any hunk). */
  hunkId: string | null
}

const KIND_STYLES: Record<LineKind, { sign: string; signColor: string }> = {
  context: { sign: ' ', signColor: 'text-outline' },
  add: { sign: '+', signColor: 'text-tertiary' },
  del: { sign: '−', signColor: 'text-error' },
}

/**
 * Flatten computeHunks output into a single line array for table rendering.
 * Each line is tagged with its owning hunk id (or null) so the row can pick
 * up the decision color from the surrounding hunk.
 */
function flattenHunks(hunks: ReturnType<typeof computeHunks>): FlatLine[] {
  const out: FlatLine[] = []
  let lastCtxOldNo = 0
  let lastCtxNewNo = 0
  for (const h of hunks) {
    for (const line of h.lines) {
      const kind: LineKind = line.type === 'added' ? 'add' : line.type === 'removed' ? 'del' : 'context'
      out.push({
        kind,
        oldNo: line.oldLineNo,
        newNo: line.newLineNo,
        text: line.text,
        hunkId: h.id,
      })
      if (kind === 'context') {
        if (line.oldLineNo !== null) lastCtxOldNo = line.oldLineNo
        if (line.newLineNo !== null) lastCtxNewNo = line.newLineNo
      }
    }
  }
  // Note: lastCtxOldNo / lastCtxNewNo aren't used right now — kept for a
  // future "context collapse" affordance that shows 3 lines around each
  // hunk instead of the whole file.
  void lastCtxOldNo
  void lastCtxNewNo
  return out
}

/**
 * Compute the insertion point (flat array index) where each hunk header
 * should render — always directly above the hunk's first line.
 */
function hunkHeaderIndices(lines: FlatLine[]): Map<string, number> {
  const out = new Map<string, number>()
  for (let i = 0; i < lines.length; i++) {
    const id = lines[i].hunkId
    if (id !== null && !out.has(id)) out.set(id, i)
  }
  return out
}

function decisionBorderStyle(decision: HunkDecision): string {
  switch (decision) {
    case 'accept': return 'border-l-4 border-l-tertiary'
    case 'reject': return 'border-l-4 border-l-error'
    default: return 'border-l-4 border-l-transparent'
  }
}

function decisionHeaderStyle(decision: HunkDecision): string {
  switch (decision) {
    case 'accept': return 'bg-tertiary-container/40 text-tertiary'
    case 'reject': return 'bg-error-container/40 text-error'
    default: return 'bg-surface-container-low text-on-surface-variant'
  }
}

function escapePlain(s: string): string {
  return s.replace(/[&<>]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c] ?? c))
}

export default function DiffViewer({ diff, decisions, onToggleHunk, className }: DiffViewerProps) {
  const intl = useIntl()
  const hunks = useMemo(
    () => computeHunks(diff.old_content, diff.new_content),
    [diff.old_content, diff.new_content],
  )
  const lines = useMemo(() => flattenHunks(hunks), [hunks])
  const headerIndices = useMemo(() => hunkHeaderIndices(lines), [lines])

  const lang = useMemo(
    () => resolveDiffLang(diff.language, diff.file_name),
    [diff.language, diff.file_name],
  )
  const oldHighlighted = useMemo(() => highlightLines(diff.old_content, lang), [diff.old_content, lang])
  const newHighlighted = useMemo(() => highlightLines(diff.new_content, lang), [diff.new_content, lang])

  const lineHtml = (line: FlatLine): string => {
    if (line.kind === 'del' && line.oldNo !== null) return oldHighlighted[line.oldNo - 1] ?? escapePlain(line.text)
    if (line.kind === 'add' && line.newNo !== null) return newHighlighted[line.newNo - 1] ?? escapePlain(line.text)
    if (line.newNo !== null) return newHighlighted[line.newNo - 1] ?? escapePlain(line.text)
    if (line.oldNo !== null) return oldHighlighted[line.oldNo - 1] ?? escapePlain(line.text)
    return escapePlain(line.text)
  }

  const addCount = lines.filter(l => l.kind === 'add').length
  const delCount = lines.filter(l => l.kind === 'del').length

  const gutterWidth = Math.max(
    String(Math.max(...lines.map(l => l.oldNo ?? 0), 1)).length,
    String(Math.max(...lines.map(l => l.newNo ?? 0), 1)).length,
  )

  const stateLabel = (d: HunkDecision): string => {
    switch (d) {
      case 'accept': return intl.formatMessage({ id: 'diff.review.state.accept' })
      case 'reject': return intl.formatMessage({ id: 'diff.review.state.reject' })
      default: return intl.formatMessage({ id: 'diff.review.state.pending' })
    }
  }

  return (
    <div className={`rounded-xl border border-outline-variant/30 overflow-hidden bg-surface-container-lowest ${className ?? ''}`}>
      <header className="flex items-center justify-between px-md py-sm border-b border-outline-variant/30 bg-surface-container-low">
        <div className="flex items-center gap-md min-w-0">
          <span className="material-symbols-outlined text-[18px] text-on-surface-variant">difference</span>
          <span className="font-label-md text-on-surface truncate">{diff.file_name || 'untitled'}</span>
          {diff.language ? (
            <span className="font-label-sm text-on-surface-variant uppercase tracking-wider">{diff.language}</span>
          ) : null}
        </div>
        <div className="flex items-center gap-md shrink-0">
          <span className="font-label-sm text-tertiary">+{addCount}</span>
          <span className="font-label-sm text-error">−{delCount}</span>
        </div>
      </header>
      <div className="overflow-x-auto font-mono text-[12px] leading-[1.5]">
        {hunks.length === 0 ? (
          <p className="px-md py-lg text-body-sm text-on-surface-variant italic">No changes.</p>
        ) : (
          <table className="w-full border-collapse">
            <tbody>
              {lines.map((line, idx) => {
                const style = KIND_STYLES[line.kind]
                const decision = line.hunkId !== null ? (decisions.get(line.hunkId) ?? 'pending') : 'pending'
                const bgClass = line.kind === 'add'
                  ? (decision === 'reject' ? 'bg-surface-container-lowest' : 'bg-tertiary-container/30')
                  : line.kind === 'del'
                    ? (decision === 'accept' ? 'bg-surface-container-lowest opacity-40' : 'bg-error-container/30')
                    : 'bg-surface-container-lowest'
                const headerIdx = line.hunkId !== null ? headerIndices.get(line.hunkId) : undefined
                const renderHeader = headerIdx === idx
                const hunkId = line.hunkId
                return (
                  <Fragment key={idx}>
                    {renderHeader && hunkId !== null && (
                      <tr className={decisionHeaderStyle(decision)}>
                        <td colSpan={4} className="px-md py-xs">
                          <button
                            type="button"
                            onClick={() => onToggleHunk?.(hunkId)}
                            disabled={!onToggleHunk}
                            className="flex items-center gap-sm w-full text-left cursor-pointer disabled:cursor-default focus-visible:outline focus-visible:outline-2 focus-visible:outline-primary rounded"
                            aria-label={intl.formatMessage(
                              { id: 'diff.review.hunk.aria' },
                              { state: stateLabel(decision) },
                            )}
                          >
                            <span className="material-symbols-outlined icon-sm">
                              {decision === 'accept' ? 'check_circle' : decision === 'reject' ? 'cancel' : 'radio_button_unchecked'}
                            </span>
                            <span className="font-label-sm uppercase tracking-wider">{stateLabel(decision)}</span>
                            <span className="font-label-sm opacity-60 ml-auto">
                              {hunks.find(h => h.id === hunkId)?.lines.length ?? 0} lines
                            </span>
                          </button>
                        </td>
                      </tr>
                    )}
                    <tr className={`${bgClass} ${line.hunkId !== null ? decisionBorderStyle(decision) : ''}`}>
                      <td className={`w-[1ch] px-xs text-center select-none ${style.signColor}`}>{style.sign}</td>
                      <td className="px-xs text-right text-outline select-none" style={{ width: `${gutterWidth + 1}ch` }}>
                        {line.oldNo ?? ''}
                      </td>
                      <td className="px-xs text-right text-outline select-none border-r border-outline-variant/20" style={{ width: `${gutterWidth + 1}ch` }}>
                        {line.newNo ?? ''}
                      </td>
                      <td className="px-md whitespace-pre text-on-surface">
                        <span className="hljs" dangerouslySetInnerHTML={{ __html: lineHtml(line) || '&nbsp;' }} />
                      </td>
                    </tr>
                  </Fragment>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}

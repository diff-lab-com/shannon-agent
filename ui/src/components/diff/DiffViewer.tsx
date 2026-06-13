// Unified-diff renderer for FileDiff payloads from get_file_diff().
//
// Computes a line-by-line LCS over old/new content (small N, no need
// for a real diff library) and renders + / − / context rows with MD3
// tokens. Deleted lines keep the old line number; added lines keep
// the new line number; context lines show both.
//
// The viewer is presentational — fetch + apply lives in the caller.

import { useMemo } from 'react'
import type { FileDiff } from '@/types'

interface DiffViewerProps {
  diff: FileDiff
  className?: string
}

type LineKind = 'context' | 'add' | 'del'

interface DiffLine {
  kind: LineKind
  oldNo?: number
  newNo?: number
  text: string
}

function computeLines(oldStr: string, newStr: string): DiffLine[] {
  const a = oldStr.split('\n')
  const b = newStr.split('\n')
  const m = a.length
  const n = b.length

  // LCS DP table. For files under a few thousand lines this is fine.
  // For huge files we'd want Myers, but the desktop preview pane isn't
  // expected to handle multi-MB sources.
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0))
  for (let i = m - 1; i >= 0; i--) {
    for (let j = n - 1; j >= 0; j--) {
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1])
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < m && j < n) {
    if (a[i] === b[j]) {
      out.push({ kind: 'context', oldNo: i + 1, newNo: j + 1, text: a[i] })
      i++
      j++
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      out.push({ kind: 'del', oldNo: i + 1, text: a[i] })
      i++
    } else {
      out.push({ kind: 'add', newNo: j + 1, text: b[j] })
      j++
    }
  }
  while (i < m) {
    out.push({ kind: 'del', oldNo: i + 1, text: a[i] })
    i++
  }
  while (j < n) {
    out.push({ kind: 'add', newNo: j + 1, text: b[j] })
    j++
  }
  return out
}

const KIND_STYLES: Record<LineKind, { bg: string; sign: string; signColor: string; textColor: string }> = {
  context: { bg: 'bg-surface-container-lowest', sign: ' ', signColor: 'text-outline', textColor: 'text-on-surface' },
  add: { bg: 'bg-tertiary-container/30', sign: '+', signColor: 'text-tertiary', textColor: 'text-on-surface' },
  del: { bg: 'bg-error-container/30', sign: '−', signColor: 'text-error', textColor: 'text-on-surface' },
}

export default function DiffViewer({ diff, className }: DiffViewerProps) {
  const lines = useMemo(() => computeLines(diff.old_content, diff.new_content), [diff.old_content, diff.new_content])
  const adds = lines.filter(l => l.kind === 'add').length
  const dels = lines.filter(l => l.kind === 'del').length

  const gutterWidth = Math.max(
    String(Math.max(...lines.map(l => l.oldNo ?? 0), 1)).length,
    String(Math.max(...lines.map(l => l.newNo ?? 0), 1)).length,
  )

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
          <span className="font-label-sm text-tertiary">+{adds}</span>
          <span className="font-label-sm text-error">−{dels}</span>
        </div>
      </header>
      <div className="overflow-x-auto font-mono text-[12px] leading-[1.5]">
        {adds === 0 && dels === 0 ? (
          <p className="px-md py-lg text-body-sm text-on-surface-variant italic">No changes.</p>
        ) : (
          <table className="w-full border-collapse">
            <tbody>
              {lines.map((line, idx) => {
                const style = KIND_STYLES[line.kind]
                return (
                  <tr key={idx} className={style.bg}>
                    <td className={`w-[1ch] px-xs text-center select-none ${style.signColor}`}>{style.sign}</td>
                    <td className="px-xs text-right text-outline select-none" style={{ width: `${gutterWidth + 1}ch` }}>
                      {line.oldNo ?? ''}
                    </td>
                    <td className="px-xs text-right text-outline select-none border-r border-outline-variant/20" style={{ width: `${gutterWidth + 1}ch` }}>
                      {line.newNo ?? ''}
                    </td>
                    <td className={`px-md whitespace-pre ${style.textColor}`}>{line.text || ' '}</td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}

// InlineDiffCard — Aurora redesign 2026-10 (docs/design/ui-redesign-2026-10/
// pages/02-chat.html 内联 diff 卡): the compact diff strip rendered at a
// COMPLETED file-mutating tool card's position (ToolCallDisplay), where the
// old header "Diff" chip used to live.
//
// One row, per the design's `.diffcard .dh` header: edit glyph, the file
// path in mono, "+N −N" line counts when they are actually computable, and
// the 「打开 Diff」 action that reuses Chat.tsx's existing diffPath chain
// (onViewDiff → RightDock diff tab). HONESTY CONTRACT: the counts come from
// the same cached `getFileDiff` computation the FileChangesCard uses
// (diffStats) — when the diff cannot be fetched (demo mode, deleted file,
// binary) the numbers are simply absent, never fabricated. There is no
// rollback/「可回滚」 pill: rewind is a turn-level affordance, not a
// per-card fact.

import { useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { fetchDiffLineStats, type DiffLineStats } from '@/components/chat/diffStats'

export interface InlineDiffCardProps {
  /** The written/edited file's path (rendered in full via tooltip). */
  path: string
  /** Open this path's diff — Chat.tsx's setDiffPath chain (RightDock tab). */
  onOpenDiff: () => void
}

export function InlineDiffCard({ path, onOpenDiff }: InlineDiffCardProps) {
  const intl = useIntl()
  const [stats, setStats] = useState<DiffLineStats | null>(null)

  // The stats cache makes this one IPC per path for the window's life;
  // failures resolve null and the card renders without numbers.
  useEffect(() => {
    let cancelled = false
    fetchDiffLineStats(path).then(s => { if (!cancelled) setStats(s) })
    return () => { cancelled = true }
  }, [path])

  const hasCounts = stats != null && (stats.additions > 0 || stats.deletions > 0)

  return (
    <div
      data-testid="inline-diff-card"
      className="mt-xs flex items-center gap-sm px-sm py-xs rounded-lg bg-tertiary/5 border border-tertiary/20 min-w-0"
    >
      <span className="material-symbols-outlined icon-sm text-tertiary shrink-0" aria-hidden="true">edit_note</span>
      <span className="font-mono text-label-xs text-on-surface truncate flex-1 min-w-0" title={path}>
        {path}
      </span>
      {hasCounts && (
        <span
          className="font-mono text-label-xs tabular-nums shrink-0"
          aria-label={intl.formatMessage({ id: 'chat.inlineDiff.stats.aria' }, {
            additions: stats!.additions,
            deletions: stats!.deletions,
          })}
        >
          <span className="text-tertiary">+{stats!.additions}</span>{' '}
          <span className="text-error">−{stats!.deletions}</span>
        </span>
      )}
      <Button
        variant="ghost"
        size="sm"
        data-testid="inline-diff-open"
        aria-label={intl.formatMessage({ id: 'chat.message.diff.aria' }, { path })}
        className="gap-xs px-xs py-[2px] shrink-0 text-tertiary hover:bg-tertiary-container/40"
        onClick={onOpenDiff}
      >
        <span className="material-symbols-outlined icon-sm" aria-hidden="true">difference</span>
        {intl.formatMessage({ id: 'chat.inlineDiff.open' })}
      </Button>
    </div>
  )
}

export default InlineDiffCard

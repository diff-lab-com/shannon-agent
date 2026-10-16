// ReviewWorkspace — the 03 页 (docs/design/ui-redesign-2026-10/pages/
// 03-chat-review.html) main-area full-width review form, T7 (R3 §4.1 挂账):
// while a diff review session is active the chat's main region swaps the
// message transcript for THIS workspace, and closing the review returns the
// chat (Chat.tsx owns the swap). The dock keeps its non-review tabs; its
// Diff tab stays the design's summary surface (checkpoint chips + file
// list), exactly the 03 mockup's dock.
//
// Form fidelity (the page's in-page style block):
//   .review-wrap  { padding: 24px; min-height: 100%; display: flex; gap: 14px }
//   .review-main  { flex: 1; min-width: 0; flex-direction: column; gap: 14px }
//   .hintbar      { margin-top: auto; padding-top: 12px; border-top hairline;
//                   mono 11px; muted; flex; wrap }
// mapped onto the app tokens: p-lg (24px), gap-[14px], text-label-xs (11px),
// font-mono, border-outline-variant hairline.
//
// Honesty note: the mockup's hintbar documents 「Tab 在文件与 hunk 间跳转 ·
// 输入 c 写行内评论」 — neither key exists in the app (inline comments never
// shipped; the mockup itself records that /review automation was not
// approved, R2 §7.1-⑤). The hintbar therefore renders the REAL key map
// (useDiffKeyboard's j/k/a/r/u/Enter) via the existing `diff.review.keys`
// string, in the design's hintbar FORM. The design top row's session-level
// 「全部接受 / 全部拒绝」 stay per-file bulk buttons inside the review body —
// the app's decision model is per-file (plus DiffDialogMulti for batches);
// the row here carries the changed-file summary and the workspace exit.

import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import DiffReviewBody from '@/components/diff/DiffReviewBody'
import ReviewFileList from '@/components/diff/ReviewFileList'
import type { HunkDecision } from '@/lib/diff-merge'

interface ReviewWorkspaceProps {
  /** The review target under the workspace's Apply/Cancel flow. */
  filePath: string | null
  /** The session's changed files — the design's file-list column. */
  changedPaths: string[]
  /** Lifted per-file hunk decisions — keeps the file list badges live. */
  decisionsByPath: Map<string, Map<string, HunkDecision>>
  /** Publishes the current file's decisions (Chat lifts into decisionsByPath). */
  onDecisionsChange?: (decisions: Map<string, HunkDecision>) => void
  /** Picks a file to review (Chat owns setDiffPath). */
  onSelectPath: (path: string) => void
  /** Leaves the review — the main region returns to the transcript. */
  onClose: () => void
}

export default function ReviewWorkspace({
  filePath,
  changedPaths,
  decisionsByPath,
  onDecisionsChange,
  onSelectPath,
  onClose,
}: ReviewWorkspaceProps) {
  const intl = useIntl()
  const closeLabel = intl.formatMessage({ id: 'diff.review.workspace.close' })
  return (
    // .review-wrap: 24px padding, full-height flex, 14px gap.
    <div
      data-testid="review-workspace"
      role="region"
      aria-label={intl.formatMessage({ id: 'diff.review.workspace.aria' })}
      className="flex-1 min-h-0 p-lg flex gap-[14px]"
    >
      {changedPaths.length > 0 && (
        <ReviewFileList
          files={changedPaths}
          currentPath={filePath}
          decisionsByPath={decisionsByPath}
          onSelectPath={onSelectPath}
        />
      )}
      {/* .review-main: flex-1, min-width 0, column, 14px gap. */}
      <div className="flex-1 min-w-0 min-h-0 flex flex-col gap-[14px]">
        {/* 顶部操作行 (03 页 row 1): the 「评审 n 个文件」 summary + the
            workspace exit. The exit must exist independently of the body's
            footer (which needs loaded hunks) — an errored/empty review can
            never trap the main region. */}
        <div className="flex items-center gap-md shrink-0 min-w-0">
          <span className="font-label-sm text-on-surface-variant truncate">
            {intl.formatMessage({ id: 'diff.multi.title' }, { count: changedPaths.length })}
          </span>
          <span className="flex-1" />
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={onClose}
            aria-label={closeLabel}
            title={closeLabel}
            className="h-auto px-sm py-xs rounded-lg font-label-sm text-on-surface-variant hover:bg-surface-container hover:text-on-surface shrink-0"
          >
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">close</span>
            <span className="ml-xs">{closeLabel}</span>
          </Button>
        </div>
        <div className="flex-1 min-h-0 flex flex-col">
          <DiffReviewBody
            filePath={filePath}
            onClose={onClose}
            active
            onDecisionsChange={onDecisionsChange}
            // The keys map renders as the pinned hintbar below — one keys
            // surface per surface, not two.
            showKeysHint={false}
          />
        </div>
        {/* .hintbar: pinned to the bottom edge by a hairline — mono 11px,
            muted, wraps. Content is the real key map (see header note). */}
        <div
          data-testid="review-hintbar"
          className="mt-auto pt-3 border-t border-outline-variant/20 font-mono text-label-xs text-on-surface-variant flex items-center gap-[6px] flex-wrap shrink-0"
        >
          <span>{intl.formatMessage({ id: 'diff.review.keys' })}</span>
        </div>
      </div>
    </div>
  )
}

// Side-by-side batch branch comparison (P1-2).
//
// Modal over the Tasks page: one column per (selectable) branch showing its
// worktree diff against the batch's base commit, fetched lazily via
// `get_batch_branch_diff` when the dialog opens. Defaults to every
// `completed` branch selected; >3 columns scroll horizontally (simplest
// robust layout for N=2..4).
//
// Reuse note: `DiffViewer`/`FileDiffList` render old/new content pairs, but
// the frozen backend contract returns a raw unified patch — so each column
// renders the patch read-only with add/del/context coloring instead.
//
// Adopt flow: 「采纳此份」→ confirm dialog (explains merge-to-base + cleanup
// of the others) → `adopt_batch_branch`. Conflicts → the conflicting file
// list plus guidance that the worktrees were kept for manual handling.

import { useEffect, useMemo, useRef, useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { Modal, ModalBody } from '@/components/ui/modal'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Spinner } from '@/components/ui/loading-state'
import { cn } from '@/lib/utils'
import * as api from '@/lib/tauri-api'
import type { BatchBranch, BatchRunDto } from '@/types'

/** Read-only unified-patch renderer: +/-/@@ line coloring, monospace. */
export function DiffPatchView({ patch, className }: { patch: string; className?: string }) {
  const lines = useMemo(() => patch.split('\n'), [patch])
  const lineClass = (line: string): string => {
    if (line.startsWith('+++') || line.startsWith('---')) return 'text-on-surface-variant'
    if (line.startsWith('diff ') || line.startsWith('index ')) return 'text-on-surface-variant font-bold'
    if (line.startsWith('@@')) return 'text-primary'
    if (line.startsWith('+')) return 'bg-success-container text-on-success-container'
    if (line.startsWith('-')) return 'bg-error-container text-on-error-container'
    return 'text-on-surface'
  }
  return (
    <pre
      className={cn(
        'font-mono text-label-xs leading-[1.5] whitespace-pre overflow-x-auto p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/20',
        className,
      )}
      data-testid="batch-diff-patch"
    >
      {lines.map((line, i) => (
        <div key={i} className={cn('px-xs -mx-xs', lineClass(line))}>
          {line || ' '}
        </div>
      ))}
    </pre>
  )
}

interface BranchColumnProps {
  batchId: string
  branch: BatchBranch
  canAdopt: boolean
  adopted: boolean
  onAdopt: (index: number) => void
}

function BranchColumn({ batchId, branch, canAdopt, adopted, onAdopt }: BranchColumnProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const [patch, setPatch] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    setPatch(null)
    setError(null)
    api
      .getBatchBranchDiff(batchId, branch.index)
      .then(({ diff }) => {
        if (!cancelled) setPatch(diff)
      })
      .catch(e => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e))
      })
    return () => {
      cancelled = true
    }
  }, [batchId, branch.index])

  return (
    <div
      className="flex-1 min-w-[280px] max-w-[480px] flex flex-col border border-outline-variant/20 rounded-xl bg-surface-container-low overflow-hidden"
      data-testid={`batch-diff-column-${branch.index}`}
    >
      <div className="flex items-center justify-between gap-xs px-sm py-sm border-b border-outline-variant/20">
        <div className="min-w-0">
          <p className="font-label-md font-bold text-on-surface">
            {t('batch.compare.branchTitle', { index: branch.index })}
          </p>
          <p className="font-label-xs text-on-surface-variant truncate">
            {branch.summary
              ? t('batch.compare.branchStat', {
                  files: branch.summary.filesChanged,
                  additions: branch.summary.additions,
                  deletions: branch.summary.deletions,
                })
              : branch.status === 'failed'
                ? branch.error ?? t(`batch.branchStatus.${branch.status}`)
                : t(`batch.branchStatus.${branch.status}`)}
          </p>
        </div>
        {adopted ? (
          <span className="font-label-xs font-bold text-success shrink-0 flex items-center gap-xs">
            <span className="material-symbols-outlined icon-sm" aria-hidden="true">
              call_merge
            </span>
            {t('batch.compare.adopted')}
          </span>
        ) : canAdopt ? (
          <Button
            size="sm"
            aria-label={t('batch.compare.adoptAria', { index: branch.index })}
            className="h-8 px-sm bg-primary text-on-primary font-label-xs cursor-pointer shrink-0"
            onClick={() => onAdopt(branch.index)}
          >
            {t('batch.compare.adopt')}
          </Button>
        ) : null}
      </div>
      <div className="p-sm overflow-auto max-h-[50vh]">
        {error ? (
          <p role="note" className="font-label-sm text-error">
            {error}
          </p>
        ) : patch === null ? (
          <div className="flex items-center justify-center py-lg">
            <Spinner />
          </div>
        ) : patch.trim().length === 0 ? (
          <p className="font-label-sm text-on-surface-variant">{t('batch.compare.emptyDiff')}</p>
        ) : (
          <DiffPatchView patch={patch} />
        )}
      </div>
    </div>
  )
}

interface BatchDiffCompareProps {
  run: BatchRunDto | null
  onClose: () => void
  /** Full adopt result (null = the call failed; the hook already toasted). */
  onAdopt: (
    batchId: string,
    index: number,
  ) => Promise<{ merged: boolean; conflicts: string[] | null } | null>
}

export default function BatchDiffCompare({ run, onClose, onAdopt }: BatchDiffCompareProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)

  const open = run !== null
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [confirmIndex, setConfirmIndex] = useState<number | null>(null)
  const [adopting, setAdopting] = useState(false)
  const [conflicts, setConflicts] = useState<{ index: number; files: string[] } | null>(null)
  // 2b — worktree-path copy feedback, same delayed-callback hygiene as
  // WebhookTriggerCard: a re-copy restarts the window instead of stacking
  // timers, and a pending timer never fires into an unmounted component.
  const [worktreeCopied, setWorktreeCopied] = useState(false)
  const copiedResetTimer = useRef<ReturnType<typeof setTimeout> | null>(null)
  useEffect(
    () => () => {
      if (copiedResetTimer.current !== null) clearTimeout(copiedResetTimer.current)
    },
    [],
  )

  // Reset the selection whenever a different batch opens: default to every
  // completed branch (all of them for N≤3 anyway).
  const batchId = run?.batchId
  useEffect(() => {
    if (!run) return
    setSelected(new Set(run.branches.filter(b => b.status === 'completed').map(b => b.index)))
    setConflicts(null)
    setConfirmIndex(null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [batchId])

  if (!run) return null

  const terminal = run.status !== 'running'
  // 2b — the conflicting branch's worktree (contract field `worktreePath`)
  // is what the user must open to resolve conflicts manually.
  const conflictBranch =
    conflicts ? run.branches.find(b => b.index === conflicts.index) ?? null : null
  const toggle = (index: number) => {
    setSelected(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
  }

  const copyConflictWorktree = async () => {
    if (!conflictBranch) return
    try {
      await navigator.clipboard.writeText(conflictBranch.worktreePath)
      setWorktreeCopied(true)
      if (copiedResetTimer.current !== null) clearTimeout(copiedResetTimer.current)
      copiedResetTimer.current = setTimeout(() => setWorktreeCopied(false), 1500)
    } catch {
      // Clipboard may be denied in the webview — the path stays visible text.
    }
  }

  const doAdopt = async () => {
    if (confirmIndex === null) return
    const index = confirmIndex
    setAdopting(true)
    const result = await onAdopt(run.batchId, index)
    setAdopting(false)
    setConfirmIndex(null)
    if (result === null) return // call failed — the hook already toasted
    if (result.merged) {
      onClose()
    } else {
      // Conflicts: keep the dialog open and show the file list + guidance;
      // the backend left every worktree in place for manual handling.
      setConflicts({ index, files: result.conflicts ?? [] })
      setWorktreeCopied(false)
    }
  }

  return (
    <>
      <Modal open={open} onClose={onClose} title={t('batch.compare.title', { title: run.title })} size="full">
        <ModalBody className="pt-0">
          {/* Branch selection chips (defaults: all completed branches).
              2a — each chip also surfaces the branch's diff summary and
              spend, both already on the `BatchBranch` contract: the summary
              is truncated in the chip (narrow branch names get max-w) and
              the full text rides on the `title`; spentUsd uses the same
              currency formatting as the card's branch chips. */}
          <div className="flex flex-wrap items-center gap-xs mb-sm">
            {run.branches.map(branch => {
              const summary = branch.summary
                ? t('batch.compare.branchStat', {
                    files: branch.summary.filesChanged,
                    additions: branch.summary.additions,
                    deletions: branch.summary.deletions,
                  })
                : null
              const spent = intl.formatNumber(branch.spentUsd, {
                style: 'currency',
                currency: 'USD',
              })
              return (
                <button
                  key={branch.index}
                  type="button"
                  role="checkbox"
                  aria-checked={selected.has(branch.index)}
                  onClick={() => toggle(branch.index)}
                  data-testid={`batch-compare-chip-${branch.index}`}
                  title={[`#${branch.index}`, branch.branchName, summary, spent]
                    .filter(Boolean)
                    .join(' · ')}
                  className={cn(
                    'flex items-center gap-xs whitespace-nowrap min-w-0 px-sm py-xs rounded-lg border font-label-xs cursor-pointer transition-colors',
                    selected.has(branch.index)
                      ? 'bg-primary-container text-on-primary-container border-primary/30'
                      : 'bg-surface-container-low text-on-surface-variant border-outline-variant/30',
                  )}
                >
                  <span className="font-bold shrink-0">#{branch.index}</span>
                  <span className="truncate max-w-[16ch]">{branch.branchName}</span>
                  {summary && (
                    <span className="tabular-nums truncate max-w-[20ch] text-on-surface-variant">
                      {summary}
                    </span>
                  )}
                  <span className="tabular-nums shrink-0">{spent}</span>
                </button>
              )
            })}
          </div>

          {/* Conflict guidance banner. */}
          {conflicts && (
            <div
              role="alert"
              data-testid="batch-conflict-banner"
              className="mb-sm p-sm rounded-lg border border-error/30 bg-error/5 text-on-surface"
            >
              <p className="font-label-md font-bold text-error flex items-center gap-xs">
                <span className="material-symbols-outlined icon-md" aria-hidden="true">
                  warning
                </span>
                {t('batch.compare.conflictTitle', { index: conflicts.index })}
              </p>
              <p className="font-label-sm mt-xs">{t('batch.compare.conflictGuidance')}</p>
              {/* 2b — where "manual handling" actually happens: the kept
                  worktree, mono + copyable (same copy affordance as the
                  webhook trigger card; a denied clipboard just leaves the
                  path as visible text). */}
              {conflictBranch && (
                <div className="mt-xs flex items-center gap-xs flex-wrap">
                  <span className="font-label-sm text-on-surface-variant shrink-0">
                    {t('batch.compare.worktreeLabel')}
                  </span>
                  <code
                    data-testid="batch-conflict-worktree"
                    className="font-mono text-label-xs text-on-surface px-sm py-xs rounded-sm bg-surface-container-lowest border border-outline-variant/20 break-all min-w-0"
                  >
                    {conflictBranch.worktreePath}
                  </code>
                  <Button
                    variant="outline"
                    size="sm"
                    aria-label={t('batch.compare.worktreeCopyAria', { index: conflicts.index })}
                    className="cursor-pointer shrink-0"
                    onClick={() => void copyConflictWorktree()}
                  >
                    <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                      {worktreeCopied ? 'check' : 'content_copy'}
                    </span>
                    {worktreeCopied
                      ? t('batch.compare.worktreeCopied')
                      : t('batch.compare.worktreeCopy')}
                  </Button>
                </div>
              )}
              <ul className="font-label-sm font-mono mt-xs list-disc list-inside">
                {conflicts.files.map(f => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            </div>
          )}

          {/* Side-by-side columns; >3 scroll horizontally. */}
          <div className="flex gap-md overflow-x-auto pb-sm" data-testid="batch-diff-columns">
            {run.branches
              .filter(b => selected.has(b.index))
              .map(branch => (
                <BranchColumn
                  key={branch.index}
                  batchId={run.batchId}
                  branch={branch}
                  canAdopt={terminal && branch.status === 'completed' && run.status !== 'adopted' && run.status !== 'discarded'}
                  adopted={run.adoptedIndex === branch.index}
                  onAdopt={index => setConfirmIndex(index)}
                />
              ))}
          </div>
        </ModalBody>
      </Modal>

      <ConfirmDialog
        open={confirmIndex !== null}
        title={t('batch.adopt.title', { index: confirmIndex ?? 0 })}
        message={t('batch.adopt.message', { count: run.count })}
        confirmLabel={t('batch.adopt.confirm')}
        cancelLabel={t('batch.adopt.cancel')}
        busy={adopting}
        busyLabel={t('batch.adopt.busy')}
        onConfirm={() => void doAdopt()}
        onCancel={() => setConfirmIndex(null)}
      />
    </>
  )
}

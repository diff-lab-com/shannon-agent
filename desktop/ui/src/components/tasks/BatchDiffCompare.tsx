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

import { useEffect, useMemo, useState } from 'react'
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
    if (line.startsWith('+')) return 'bg-success/10 text-success'
    if (line.startsWith('-')) return 'bg-error/10 text-error'
    return 'text-on-surface'
  }
  return (
    <pre
      className={cn(
        'font-mono text-[11px] leading-[1.5] whitespace-pre overflow-x-auto p-sm rounded-lg bg-surface-container-lowest border border-outline-variant/20',
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
          <span className="font-label-xs font-bold text-success shrink-0 flex items-center gap-1">
            <span className="material-symbols-outlined text-[14px]" aria-hidden="true">
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
  const toggle = (index: number) => {
    setSelected(prev => {
      const next = new Set(prev)
      if (next.has(index)) next.delete(index)
      else next.add(index)
      return next
    })
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
    }
  }

  return (
    <>
      <Modal open={open} onClose={onClose} title={t('batch.compare.title', { title: run.title })} size="full">
        <ModalBody className="pt-0">
          {/* Branch selection chips (defaults: all completed branches). */}
          <div className="flex flex-wrap items-center gap-xs mb-sm">
            {run.branches.map(branch => (
              <button
                key={branch.index}
                type="button"
                role="checkbox"
                aria-checked={selected.has(branch.index)}
                onClick={() => toggle(branch.index)}
                className={cn(
                  'px-sm py-1 rounded-lg border font-label-xs cursor-pointer transition-colors',
                  selected.has(branch.index)
                    ? 'bg-primary/10 text-primary border-primary/30'
                    : 'bg-surface-container-low text-on-surface-variant border-outline-variant/30',
                )}
              >
                #{branch.index} · {branch.branchName}
              </button>
            ))}
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

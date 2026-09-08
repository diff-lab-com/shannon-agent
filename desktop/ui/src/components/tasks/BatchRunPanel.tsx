// Best-of-N batch cards (P1-2) — live view of the desktop batch runner on
// the Tasks page's Active tab. Data comes from `useBatchRuns` (list +
// `batch:updated` push), so branch chips (status color + filesChanged +
// spentUsd) update the moment anything changes, without polling. Hidden
// entirely when there is nothing to show — the entry point is the
// Tasks-page「并行方案」form.
//
// When every branch is terminal the card's primary action is Compare, which
// opens the side-by-side BatchDiffCompare dialog (diff review + adopt).

import { useState } from 'react'
import { useIntl } from 'react-intl'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { cn } from '@/lib/utils'
import { useBatchRuns } from '@/hooks/batchRuns'
import type { BatchBranch, BatchRunDto, BatchRunStatus } from '@/types'
import BatchDiffCompare from '@/components/tasks/BatchDiffCompare'

/** MD3 badge classes per batch status (theme semantic tokens). */
function batchStatusBadge(status: BatchRunStatus): {
  bg: string
  dot: string
  icon: string
  labelId: string
} {
  // Badge text is always `text-on-surface`: status-tinted text on its own
  // 10% tint (e.g. green-600 on green-600/10 ≈ 2.9:1) is far below AA for
  // the 11px uppercase label, and the card itself sits on a translucent
  // glass panel. Status stays encoded in the dot + border tint.
  switch (status) {
    case 'running':
      return {
        bg: 'bg-primary/10 text-on-surface border-primary/20',
        dot: 'bg-primary animate-pulse',
        icon: 'autorenew',
        labelId: 'batch.status.running',
      }
    case 'completed':
      return {
        bg: 'bg-green-600/10 text-on-surface border-green-600/20',
        dot: 'bg-green-600',
        icon: 'check_circle',
        labelId: 'batch.status.completed',
      }
    case 'partially_failed':
      return {
        bg: 'bg-tertiary/10 text-on-surface border-tertiary/20',
        dot: 'bg-tertiary',
        icon: 'report',
        labelId: 'batch.status.partially_failed',
      }
    case 'failed':
      return {
        bg: 'bg-error/10 text-on-surface border-error/20',
        dot: 'bg-error',
        icon: 'error',
        labelId: 'batch.status.failed',
      }
    case 'adopted':
      return {
        bg: 'bg-green-600/10 text-on-surface border-green-600/20',
        dot: 'bg-green-600',
        icon: 'call_merge',
        labelId: 'batch.status.adopted',
      }
    case 'discarded':
      return {
        bg: 'bg-surface-container-high text-on-surface-variant border-outline-variant/30',
        dot: 'bg-outline-variant',
        icon: 'delete_sweep',
        labelId: 'batch.status.discarded',
      }
  }
}

function branchChipClasses(status: BatchBranch['status']): string {
  // Text is always `text-on-surface`: these chips sit on a translucent
  // glass card, and low-margin pairs (on-primary-container on
  // primary-container is only ~4.6:1 in material; `.opacity-80` drops any
  // pair below AA) passed the sweep locally but failed on CI's compositor.
  // Status lives in the dot + border color; the text keeps the
  // maximum-margin surface pair.
  switch (status) {
    case 'running':
      return 'bg-primary/10 text-on-surface border-primary/30'
    case 'completed':
      return 'bg-green-600/10 text-on-surface border-green-600/20'
    case 'failed':
      return 'bg-error/10 text-on-surface border-error/20'
  }
}

interface BatchRunCardProps {
  run: BatchRunDto
  onCompare: (run: BatchRunDto) => void
  onDiscard: (run: BatchRunDto) => void
}

export function BatchRunCard({ run, onCompare, onDiscard }: BatchRunCardProps) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, string | number>) =>
    intl.formatMessage({ id }, values)
  const badge = batchStatusBadge(run.status)
  const terminal = run.status !== 'running'
  const totalSpent = run.branches.reduce((sum, b) => sum + b.spentUsd, 0)

  return (
    <div
      className="glass-panel border border-outline-variant/10 rounded-xl p-md shadow-sm bg-surface-container-lowest/80"
      data-testid="batch-run-card"
      data-status={run.status}
    >
      <div className="flex items-start justify-between gap-md">
        <div className="flex items-start gap-md min-w-0">
          <div className="w-10 h-10 rounded-xl bg-tertiary/10 flex items-center justify-center text-tertiary shrink-0">
            <span className="material-symbols-outlined text-[24px]">call_split</span>
          </div>
          <div className="min-w-0">
            <h3 className="font-body-lg font-semibold text-on-surface truncate">
              {run.title}
            </h3>
            <p className="font-label-sm text-on-surface-variant line-clamp-2">{run.prompt}</p>
          </div>
        </div>
        <div
          title={t(badge.labelId)}
          className={cn(
            'flex items-center gap-xs px-sm py-1 rounded-full border shrink-0',
            badge.bg,
          )}
        >
          <span className={cn('w-2 h-2 rounded-full', badge.dot)} />
          <span className="font-label-sm text-[11px] font-bold uppercase tracking-wider">
            {t(badge.labelId)}
          </span>
        </div>
      </div>

      {/* Branch chips: status color + filesChanged + spentUsd (frozen data). */}
      <ul className="mt-sm flex flex-wrap gap-xs" aria-label={t('batch.card.branchesAria')}>
        {run.branches.map(branch => (
          <li
            key={branch.index}
            data-testid={`batch-branch-chip-${branch.index}`}
            title={branch.error ?? branch.branchName}
            className={cn(
              'flex items-center gap-1 px-sm py-1 rounded-lg border font-label-xs',
              branchChipClasses(branch.status),
            )}
          >
            <span className="font-bold">#{branch.index}</span>
            <span
              className={cn('w-1.5 h-1.5 rounded-full', branch.status === 'running' && 'animate-pulse', branch.status === 'completed' ? 'bg-green-600' : branch.status === 'failed' ? 'bg-error' : 'bg-current')}
            />
            {branch.summary ? (
              <span className="tabular-nums">
                {intl.formatNumber(branch.summary.filesChanged)}
                {' '}
                {t('batch.card.files')}
              </span>
            ) : (
              <span>…</span>
            )}
            <span className="tabular-nums">
              {intl.formatNumber(branch.spentUsd, { style: 'currency', currency: 'USD' })}
            </span>
          </li>
        ))}
      </ul>

      <div className="mt-sm flex items-center justify-between gap-md">
        <span className="font-label-sm text-on-surface-variant tabular-nums">
          {t('batch.card.totalSpent', {
            count: run.branches.length,
            spent: intl.formatNumber(totalSpent, { style: 'currency', currency: 'USD' }),
          })}
        </span>
        <div className="flex items-center gap-xs">
          {terminal && run.status !== 'discarded' && (
            <Button
              variant="ghost"
              size="sm"
              aria-label={t('batch.card.discardAria')}
              className="h-8 font-label-sm text-error hover:bg-error/10 cursor-pointer"
              onClick={() => onDiscard(run)}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                delete_sweep
              </span>
              {t('batch.card.discard')}
            </Button>
          )}
          {terminal && (
            <Button
              size="sm"
              aria-label={t('batch.card.compareAria')}
              className="h-8 px-md bg-primary text-on-primary font-label-sm cursor-pointer"
              onClick={() => onCompare(run)}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                compare
              </span>
              {t('batch.card.compare')}
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

export default function BatchRunPanel() {
  const intl = useIntl()
  const { runs, adopt, discard } = useBatchRuns()
  // Track the compared batch by id and derive the live run from `runs`, so
  // the dialog reflects `batch:updated` refreshes (statuses, adopted state).
  const [compareBatchId, setCompareBatchId] = useState<string | null>(null)
  const [discardTarget, setDiscardTarget] = useState<BatchRunDto | null>(null)
  const compareTarget = runs.find(r => r.batchId === compareBatchId) ?? null

  if (runs.length === 0) return null

  return (
    <section aria-labelledby="batch-runs-heading" className="mb-lg" data-testid="batch-run-panel">
      <h2
        id="batch-runs-heading"
        className="font-label-lg font-bold text-on-surface mb-sm flex items-center gap-xs"
      >
        <span className="material-symbols-outlined text-[18px] text-tertiary" aria-hidden="true">
          call_split
        </span>
        {intl.formatMessage({ id: 'batch.panel.heading' })}
      </h2>
      <div className="space-y-sm">
        {runs.map(run => (
          <BatchRunCard
            key={run.batchId}
            run={run}
            onCompare={() => setCompareBatchId(run.batchId)}
            onDiscard={setDiscardTarget}
          />
        ))}
      </div>

      <BatchDiffCompare
        run={compareTarget}
        onClose={() => setCompareBatchId(null)}
        onAdopt={(batchId, index) => adopt(batchId, index)}
      />

      {/* Discard deletes worktrees + branches — confirm first (the un-merged
          attempts are gone for good). */}
      <ConfirmDialog
        open={discardTarget !== null}
        title={intl.formatMessage({ id: 'batch.discard.title' })}
        message={intl.formatMessage({ id: 'batch.discard.message' })}
        confirmLabel={intl.formatMessage({ id: 'batch.discard.confirm' })}
        cancelLabel={intl.formatMessage({ id: 'batch.discard.cancel' })}
        destructive
        onConfirm={() => {
          const batchId = discardTarget?.batchId
          setDiscardTarget(null)
          if (batchId) void discard(batchId)
        }}
        onCancel={() => setDiscardTarget(null)}
      />
    </section>
  )
}

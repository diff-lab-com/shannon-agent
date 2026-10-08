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
import { useIntl, type IntlShape } from 'react-intl'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { Modal, ModalBody } from '@/components/ui/modal'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Spinner } from '@/components/ui/loading-state'
import { cn } from '@/lib/utils'
import { toastError } from '@/lib/errorToast'
import * as api from '@/lib/tauri-api'
import type { BatchBranch, BatchRunDto, BatchVerification } from '@/types'

/** The component's localized-string helper shape (matches `t` below). */
type Translate = (id: string, values?: Record<string, string | number>) => string

/**
 * W4 — deterministic "smallest diff" fact: the completed branch whose
 * worktree changed the fewest lines (+additions −deletions). The per-branch
 * +/− already rides the `BatchBranch.summary` contract (same numbers the
 * chips render), so this needs no extra IPC — and unlike a per-file helper
 * (`fetchDiffLineStats` is single-file over `getFileDiff`), it covers every
 * branch, selected or not. Needs ≥2 branches with stats to say "smallest"
 * of something; ties keep the lowest index (strict `<`).
 */
export function smallestDiffBranchIndex(run: BatchRunDto): number | null {
  const candidates = run.branches.filter(b => b.status === 'completed' && b.summary !== null)
  if (candidates.length < 2) return null
  let best = candidates[0]
  for (const b of candidates.slice(1)) {
    const size = (x: typeof b) => x.summary!.additions + x.summary!.deletions
    if (size(b) < size(best)) best = b
  }
  return best.index
}

/**
 * Full localized verification phrase (verdict card + Markdown export).
 * null = the branch carries no verification data (old backend payload) —
 * callers must not render a row at all.
 */
export function verificationFactText(v: BatchVerification, t: Translate): string {
  if (v.exitOk) {
    if (v.testsPassed !== undefined && v.testsTotal !== undefined) {
      return t('batch.compare.testsPassedCount', { passed: v.testsPassed, total: v.testsTotal })
    }
    return t('batch.compare.testsPassed')
  }
  if (v.testsPassed !== undefined && v.testsTotal !== undefined) {
    return t('batch.compare.testsFailedCount', {
      passed: v.testsPassed,
      total: v.testsTotal,
      failed: v.testsTotal - v.testsPassed,
    })
  }
  return t('batch.compare.verificationFailed')
}

/**
 * Markdown export for the compare dialog (same "decidable facts only"
 * discipline as the verdict card): batch title, then one block per branch —
 * name, status, diff summary, spend, verification facts (only when the
 * backend sent them), worktree path — closed by the derived conclusions.
 */
export function buildCompareMarkdown(run: BatchRunDto, t: Translate, intl: IntlShape): string {
  const spent = (usd: number) => intl.formatNumber(usd, { style: 'currency', currency: 'USD' })
  const lines: string[] = [`# ${t('batch.compare.title', { title: run.title })}`, '']
  for (const b of run.branches) {
    lines.push(`## ${t('batch.compare.export.mdBranch', { index: b.index })} — ${b.branchName}`, '')
    lines.push(`- ${t('batch.compare.export.mdStatus')}: ${t(`batch.branchStatus.${b.status}`)}`)
    if (b.summary) {
      lines.push(
        `- ${t('batch.compare.export.mdChanges')}: ${t('batch.compare.branchStat', {
          files: b.summary.filesChanged,
          additions: b.summary.additions,
          deletions: b.summary.deletions,
        })}`,
      )
    }
    lines.push(`- ${t('batch.compare.export.mdSpent')}: ${spent(b.spentUsd)}`)
    if (b.verification) {
      lines.push(
        `- ${t('batch.compare.export.mdVerification')}: ${verificationFactText(b.verification, t)}`,
      )
    }
    lines.push(`- ${t('batch.compare.export.mdWorktree')}: \`${b.worktreePath}\``)
    lines.push('')
  }
  // Derived conclusions, only when decidable (mirrors the verdict card).
  const verdict: string[] = []
  for (const b of run.branches) {
    if (b.verification) {
      verdict.push(`- #${b.index}: ${verificationFactText(b.verification, t)}`)
    }
  }
  const smallest = smallestDiffBranchIndex(run)
  const smallestBranch = smallest === null ? null : run.branches.find(b => b.index === smallest)
  if (smallestBranch?.summary) {
    verdict.push(
      `- #${smallestBranch.index}: ${t('batch.compare.smallestDiff')} — ${t('batch.compare.branchStat', {
        files: smallestBranch.summary.filesChanged,
        additions: smallestBranch.summary.additions,
        deletions: smallestBranch.summary.deletions,
      })}`,
    )
  }
  if (smallest !== null && run.branches.some(b => b.index === smallest && b.verification?.exitOk)) {
    verdict.push(`- #${smallest}: ${t('batch.compare.recommended')}`)
  }
  if (verdict.length > 0) {
    lines.push(`## ${t('batch.compare.verdictTitle')}`, '', ...verdict, '')
  }
  return lines.join('\n')
}

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
  // W4 — Markdown export: same backend-dialog flow as the timeline HTML
  // export (the user's dialog pick is the authorization; cancelling is a
  // decision, not an error).
  const [exporting, setExporting] = useState(false)
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

  // W4 — decidable facts shared by the chip badges, the verdict card and the
  // Markdown export. Verification is contract-optional (`verification?`):
  // when NO branch carries it (old payload), none of the verification UI
  // renders at all — no silent "unknown" placeholders.
  const smallestIndex = smallestDiffBranchIndex(run)
  const verifiedBranches = run.branches.filter(b => b.verification !== undefined)
  const hasVerification = verifiedBranches.length > 0
  // 设计 04b (审查 R1 §4): the branches' combined spend — the live per-branch
  // `spentUsd` summed, not an estimate.
  const totalSpent = run.branches.reduce((sum, b) => sum + b.spentUsd, 0)
  // Recommendation = tests passed AND smallest diff (design 04b); when no
  // branch satisfies both, no branch is marked — never a guessed pick.
  const recommendedIndex =
    smallestIndex !== null &&
    run.branches.some(b => b.index === smallestIndex && b.verification?.exitOk)
      ? smallestIndex
      : null

  const handleExport = async () => {
    setExporting(true)
    try {
      const savedPath = await api.saveTextFileViaDialog(
        buildCompareMarkdown(run, t, intl),
        `batch-${run.batchId}-compare.md`,
      )
      if (savedPath === null) return // dialog cancelled — back out silently
      toast.success(t('batch.compare.export.success'))
    } catch (e) {
      toastError(t('batch.compare.export.failed'), e)
    } finally {
      setExporting(false)
    }
  }

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
              currency formatting as the card's branch chips.
              W4 — the header row also carries the Markdown export (ghost,
              right-aligned per design 04b) and each chip can surface three
              derived badges: the deterministic 「smallest diff」 fact, the
              contract-optional verification outcome, and the tests-passed ∧
              smallest-diff recommendation. */}
          <div className="flex items-start justify-between gap-md mb-sm">
            <div className="flex flex-wrap items-center gap-xs flex-1 min-w-0">
              {/* 设计 04b (审查 R1 §4): the dialog header states the diff's
                  baseline and the branches' combined spend. The base commit
                  is contract-optional (older payloads render the total
                  alone) and shown short (7 chars) like git log. */}
              {(run.baseCommit || totalSpent > 0) && (
                <span
                  data-testid="batch-compare-header-meta"
                  className="font-label-sm text-label-sm text-on-surface-variant tabular-nums shrink-0 max-w-full"
                >
                  {run.baseCommit
                    ? t('batch.compare.baseAndTotal', {
                        commit: run.baseCommit.slice(0, 7),
                        cost: intl.formatNumber(totalSpent, { style: 'currency', currency: 'USD' }),
                      })
                    : t('batch.compare.totalOnly', {
                        cost: intl.formatNumber(totalSpent, { style: 'currency', currency: 'USD' }),
                      })}
                </span>
              )}
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
                // Verification badge: short on the chip (icon + counts when
                // the backend parsed them), full phrase on the `title`.
                const v = branch.verification
                const vBadge = v
                  ? v.exitOk
                    ? v.testsPassed !== undefined && v.testsTotal !== undefined
                      ? `${v.testsPassed}/${v.testsTotal}`
                      : t('batch.compare.testsPassed')
                    : t('batch.compare.verificationFailed')
                  : null
                const vTitle = v ? verificationFactText(v, t) : null
                const isSmallest = smallestIndex === branch.index
                const isRecommended = recommendedIndex === branch.index
                return (
                  <button
                    key={branch.index}
                    type="button"
                    role="checkbox"
                    aria-checked={selected.has(branch.index)}
                    onClick={() => toggle(branch.index)}
                    data-testid={`batch-compare-chip-${branch.index}`}
                    title={[
                      `#${branch.index}`,
                      branch.branchName,
                      summary,
                      vTitle,
                      spent,
                    ]
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
                    {isSmallest && (
                      <span
                        data-testid={`batch-compare-smallest-${branch.index}`}
                        title={t('batch.compare.smallestDiff')}
                        className="shrink-0 flex items-center gap-[2px] font-bold text-on-surface"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                          compress
                        </span>
                        {t('batch.compare.smallestDiff')}
                      </span>
                    )}
                    {vBadge && (
                      <span
                        data-testid={`batch-compare-verification-${branch.index}`}
                        title={vTitle ?? undefined}
                        className={cn(
                          'shrink-0 flex items-center gap-[2px] font-bold tabular-nums',
                          v?.exitOk ? 'text-success' : 'text-error',
                        )}
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                          {v?.exitOk ? 'check' : 'close'}
                        </span>
                        {vBadge}
                      </span>
                    )}
                    {isRecommended && (
                      <span
                        data-testid={`batch-compare-recommended-${branch.index}`}
                        className="shrink-0 flex items-center gap-[2px] font-bold text-primary"
                      >
                        <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                          check
                        </span>
                        {t('batch.compare.recommended')}
                      </span>
                    )}
                  </button>
                )
              })}
            </div>
            <Button
              variant="ghost"
              size="sm"
              aria-label={t('batch.compare.export.aria')}
              data-testid="batch-compare-export"
              disabled={exporting}
              className="cursor-pointer shrink-0"
              onClick={() => void handleExport()}
            >
              <span className="material-symbols-outlined icon-sm" aria-hidden="true">
                download
              </span>
              {exporting ? t('batch.compare.export.busy') : t('batch.compare.export.button')}
            </Button>
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

          {/* W4 — compare verdict (design 04b): only decidable facts, and
              only when the backend actually sent verification data for at
              least one branch. All-old payload → the whole card stays
              unrendered (honest absence, no "unknown" rows). */}
          {hasVerification && (
            <div
              data-testid="batch-compare-verdict"
              className="mb-sm p-sm rounded-xl border border-outline-variant/20 bg-surface-container-low"
            >
              <div className="flex items-center gap-xs">
                <p className="font-label-md font-bold text-on-surface">
                  {t('batch.compare.verdictTitle')}
                </p>
                <span className="ml-auto font-label-xs font-bold text-primary shrink-0">
                  {t('batch.compare.verdictAuto')}
                </span>
              </div>
              <ul className="mt-xs">
                {verifiedBranches.map(b => (
                  <li
                    key={b.index}
                    data-testid={`batch-compare-verdict-row-${b.index}`}
                    className="font-label-sm text-on-surface-variant flex items-start gap-xs py-xs"
                  >
                    <span
                      className={cn(
                        'material-symbols-outlined icon-sm shrink-0',
                        b.verification!.exitOk ? 'text-success' : 'text-error',
                      )}
                      aria-hidden="true"
                    >
                      {b.verification!.exitOk ? 'check' : 'close'}
                    </span>
                    <span>
                      <span className="font-bold text-on-surface">#{b.index}</span>{' '}
                      {verificationFactText(b.verification!, t)}
                    </span>
                  </li>
                ))}
                {smallestIndex !== null &&
                  (() => {
                    const b = run.branches.find(x => x.index === smallestIndex)
                    if (!b?.summary) return null
                    return (
                      <li
                        data-testid="batch-compare-verdict-smallest"
                        className="font-label-sm text-on-surface-variant flex items-start gap-xs py-xs"
                      >
                        <span
                          className="material-symbols-outlined icon-sm shrink-0 text-success"
                          aria-hidden="true"
                        >
                          check
                        </span>
                        <span>
                          <span className="font-bold text-on-surface">#{b.index}</span>{' '}
                          {t('batch.compare.smallestDiff')}:{' '}
                          {t('batch.compare.branchStat', {
                            files: b.summary.filesChanged,
                            additions: b.summary.additions,
                            deletions: b.summary.deletions,
                          })}
                        </span>
                      </li>
                    )
                  })()}
              </ul>
              <p className="font-label-xs text-on-surface-variant mt-xs pt-xs border-t border-outline-variant/20">
                {t('batch.compare.verdictFootnote')}
              </p>
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

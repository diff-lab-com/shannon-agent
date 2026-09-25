// Dream panel — 梦境提炼 (dream distillation) section of the Memory page.
//
// SCOPE: run one review-gated distillation pass (run_dream_pass), browse the
// pending shadow proposals (list_dream_proposals), apply selected actions or
// discard each proposal, read the newest pass report (read_dream_report,
// rendered as plain pre-wrap text — no markdown dependency), and show the
// persisted last-run stats on cold start (read_dream_state).
//
// Non-destructive by design: proposals live under ~/.shannon/dreams/ and the
// only path into ~/.shannon/memories/ is applyDreamProposal — the user-
// approved subset of a proposal's actions. Live runs announce themselves via
// the `dream-pass-finished` window event (same listen pattern as
// `skill-candidates-changed`), so the panel also reflects nightly/`/dream`
// passes started elsewhere.

import { useCallback, useEffect, useState } from 'react'
import { useIntl } from 'react-intl'
import { listen } from '@tauri-apps/api/event'
import { toast } from 'sonner'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Modal } from '@/components/ui/modal'
import { Spinner } from '@/components/ui/loading-state'
import {
  applyDreamProposal,
  discardDreamProposal,
  listDreamProposals,
  readDreamReport,
  readDreamState,
  runDreamPass,
  type DreamAction,
  type DreamPassResult,
  type DreamProposal,
  type DreamState,
} from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

/// Outcome → toast key for a skipped pass (disabled / throttled / in-progress).
function skippedToastKey(reason: NonNullable<DreamPassResult['skipped_reason']>): string {
  switch (reason) {
    case 'disabled': return 'memory.dream.skipped.disabled'
    case 'throttled': return 'memory.dream.skipped.throttled'
    case 'in-progress': return 'memory.dream.skipped.inProgress'
  }
}

function countByKind(proposal: DreamProposal, kind: DreamAction['kind']): number {
  return proposal.actions.filter(a => a.kind === kind).length
}

const KIND_META: Record<DreamAction['kind'], { icon: string; labelKey: string; badgeClass: string }> = {
  merge: { icon: 'call_merge', labelKey: 'memory.dream.action.merge', badgeClass: 'bg-secondary-container text-on-secondary-container' },
  remove: { icon: 'delete', labelKey: 'memory.dream.action.remove', badgeClass: 'bg-error/10 text-error' },
  add: { icon: 'add_circle', labelKey: 'memory.dream.action.add', badgeClass: 'bg-primary-container text-on-primary-container' },
}

export default function DreamPanel() {
  const intl = useIntl()
  // Stable across renders (memoized on the provider's intl object) — the
  // fetch effect below keys on this, so an inline arrow would loop fetches.
  const t = useCallback(
    (id: string, values?: Record<string, string | number>) => intl.formatMessage({ id }, values),
    [intl],
  )

  const [proposals, setProposals] = useState<DreamProposal[]>([])
  const [loading, setLoading] = useState(true)
  const [running, setRunning] = useState(false)
  /// Stats of the most recent pass that actually ran in this window (from
  /// its result or the `dream-pass-finished` event). Before the first run
  /// here, the persisted `read_dream_state` read-back below covers the line.
  const [lastRun, setLastRun] = useState<DreamPassResult | null>(null)
  /// Persisted state read back on mount (卡C): the last pass's timestamp +
  /// stats, so the panel shows 「上次提炼」 on cold start. Null → no line.
  const [persisted, setPersisted] = useState<DreamState | null>(null)
  /// Per-proposal selection of action ids; defaults to all selected
  /// (「应用所选，其余丢弃」).
  const [selected, setSelected] = useState<Record<string, Set<string>>>({})
  const [busyProposalId, setBusyProposalId] = useState<string | null>(null)
  const [pendingDiscardId, setPendingDiscardId] = useState<string | null>(null)
  const [reportOpen, setReportOpen] = useState(false)
  const [reportLoading, setReportLoading] = useState(false)
  const [report, setReport] = useState<string | null>(null)
  /// Distinguishes a failed fetch (error body) from a fetched-but-empty
  /// report (empty-state body) — review finding #8.
  const [reportError, setReportError] = useState(false)

  const fetchProposals = useCallback(async () => {
    try {
      const rows = await listDreamProposals()
      setProposals(rows)
      setSelected(prev => {
        // Rebuild keyed on the still-pending proposals: keeps the user's
        // selection edits, prunes keys whose proposal was consumed
        // (applied/discarded, possibly elsewhere) so a later proposal
        // reusing the id gets fresh defaults instead of a stale selection.
        const next: Record<string, Set<string>> = {}
        for (const p of rows) {
          next[p.id] = prev[p.id] ?? new Set(p.actions.map(a => a.id))
        }
        return next
      })
    } catch (e) {
      toast.error(t('memory.dream.proposals.loadFailed'), {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setLoading(false)
    }
  }, [t])

  useEffect(() => {
    void fetchProposals()
  }, [fetchProposals])

  // Cold-start read-back (卡C): the persisted last-pass timestamp + stats.
  // Best-effort — a failed read just leaves the 「上次提炼」 line off.
  useEffect(() => {
    let cancelled = false
    readDreamState()
      .then((state) => {
        if (!cancelled) setPersisted(state)
      })
      .catch(() => { /* demo/browser mode or unreadable state file */ })
    return () => {
      cancelled = true
    }
  }, [])

  // A pass that finished anywhere in the window (nightly scheduler, /dream
  // slash, another panel) refreshes the review list + the stats line.
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | undefined
    listen<DreamPassResult>('dream-pass-finished', (event) => {
      if (cancelled) return
      setLastRun(event.payload)
      void fetchProposals()
    })
      .then((fn) => {
        if (cancelled) fn()
        else unlisten = fn
      })
      .catch(() => { /* demo/browser mode — no live events */ })
    return () => {
      cancelled = true
      unlisten?.()
    }
  }, [fetchProposals])

  const handleRun = async () => {
    setRunning(true)
    try {
      const result = await runDreamPass(null)
      if (result.skipped_reason == null) {
        setLastRun(result)
        toast.success(t('memory.dream.toast.finished'), {
          description: t('memory.dream.stats', {
            scanned: result.scanned_sessions,
            merge: result.merge_proposed,
            remove: result.remove_proposed,
            add: result.add_proposed,
            candidates: result.candidates_detected,
          }),
        })
      } else {
        toast.info(t(skippedToastKey(result.skipped_reason)))
      }
      await fetchProposals()
    } catch (e) {
      toast.error(t('memory.dream.runFailed'), {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setRunning(false)
    }
  }

  const toggleAction = (proposalId: string, actionId: string) => {
    setSelected(prev => {
      const set = new Set(prev[proposalId] ?? [])
      if (set.has(actionId)) set.delete(actionId)
      else set.add(actionId)
      return { ...prev, [proposalId]: set }
    })
  }

  const handleApply = async (proposal: DreamProposal) => {
    const actionIds = [...(selected[proposal.id] ?? [])]
    if (actionIds.length === 0) return
    setBusyProposalId(proposal.id)
    try {
      const outcome = await applyDreamProposal(proposal.id, actionIds)
      toast.success(t('memory.dream.applyDone', { count: outcome.applied.length }), {
        // Review finding #9: actions whose targets vanished between the pass
        // and the apply are skipped by the backend — surface the count so
        // "applied 2" never silently means "you asked for 5".
        description:
          outcome.skipped.length > 0
            ? t('memory.dream.apply.skippedToast', { count: outcome.skipped.length })
            : undefined,
      })
      await fetchProposals()
    } catch (e) {
      toast.error(t('memory.dream.applyFailed'), {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setBusyProposalId(null)
    }
  }

  const confirmDiscard = async () => {
    const id = pendingDiscardId
    if (!id) return
    setPendingDiscardId(null)
    try {
      await discardDreamProposal(id)
      toast.success(t('memory.dream.discardDone'))
      await fetchProposals()
    } catch (e) {
      toast.error(t('memory.dream.discardFailed'), {
        description: e instanceof Error ? e.message : String(e),
      })
    }
  }

  const openReport = async () => {
    setReportOpen(true)
    setReportLoading(true)
    setReportError(false)
    try {
      setReport(await readDreamReport(null))
    } catch (e) {
      setReportError(true)
      toast.error(t('memory.dream.report.loadFailed'), {
        description: e instanceof Error ? e.message : String(e),
      })
    } finally {
      setReportLoading(false)
    }
  }

  // 「上次提炼：<时间> · <统计摘要>」 (卡C cold start): only when a previous
  // run exists on disk — `last_dream_at` null shows nothing, no empty-state
  // clutter. The stats summary reuses the live-run ICU message; a state
  // file written without stats falls back to the timestamp-only form.
  let coldStartLine: string | null = null
  if (persisted?.last_dream_at) {
    const when = new Date(persisted.last_dream_at)
    const time = isNaN(when.getTime())
      ? persisted.last_dream_at
      : intl.formatDate(when, { dateStyle: 'medium', timeStyle: 'short' })
    const stats = persisted.last_stats
    if (stats) {
      coldStartLine = t('memory.dream.lastDistilled', {
        time,
        stats: t('memory.dream.stats', {
          scanned: Number(stats.scanned_sessions ?? 0),
          merge: Number(stats.merge_proposed ?? 0),
          remove: Number(stats.remove_proposed ?? 0),
          add: Number(stats.add_proposed ?? 0),
          candidates: Number(stats.candidates_detected ?? 0),
        }),
      })
    } else {
      coldStartLine = t('memory.dream.lastDistilledNoStats', { time })
    }
  }

  return (
    <section
      aria-label={t('memory.dream.title')}
      data-testid="dream-panel"
      className="bg-surface-container-lowest p-lg rounded-xl shadow-sm border border-outline-variant/30 mb-xl"
    >
      <div className="flex items-center gap-md mb-sm">
        <div className="p-2 bg-primary/10 rounded-lg text-primary flex items-center justify-center shrink-0">
          <span className="material-symbols-outlined">bedtime</span>
        </div>
        <h2 className="font-headline-sm text-on-surface">{t('memory.dream.title')}</h2>
        <Button
          onClick={() => void handleRun()}
          disabled={running}
          className="ml-auto gap-xs px-md py-sm text-[14px] font-bold shrink-0"
          aria-label={t('memory.dream.run.aria')}
        >
          {running ? <Spinner className="text-[18px]" /> : <span className="material-symbols-outlined text-[18px]">auto_awesome</span>}
          {running ? t('memory.dream.running') : t('memory.dream.run')}
        </Button>
        <Button
          variant="ghost"
          onClick={() => void openReport()}
          className="gap-xs px-sm py-sm text-[14px] text-on-surface-variant hover:text-primary shrink-0"
        >
          <span className="material-symbols-outlined text-[18px]">description</span>
          {t('memory.dream.viewReport')}
        </Button>
      </div>
      <p className="text-on-surface-variant text-body-sm mb-md">{t('memory.dream.subtitle')}</p>

      {lastRun ? (
        <div className="flex items-center gap-xs px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant/30 text-label-sm text-on-surface-variant mb-md">
          <span className="material-symbols-outlined text-[16px] text-primary" aria-hidden="true">insights</span>
          {t('memory.dream.stats', {
            scanned: lastRun.scanned_sessions,
            merge: lastRun.merge_proposed,
            remove: lastRun.remove_proposed,
            add: lastRun.add_proposed,
            candidates: lastRun.candidates_detected,
          })}
        </div>
      ) : coldStartLine ? (
        <div className="flex items-center gap-xs px-md py-sm rounded-lg bg-surface-container-low border border-outline-variant/30 text-label-sm text-on-surface-variant mb-md">
          <span className="material-symbols-outlined text-[16px] text-primary" aria-hidden="true">history</span>
          {coldStartLine}
        </div>
      ) : null}

      <div className="flex items-center gap-sm mb-xs">
        <h3 className="font-label-md text-[14px] font-bold text-on-surface">{t('memory.dream.proposals.title')}</h3>
        {proposals.length > 0 && (
          <span className="px-sm py-[2px] rounded-full bg-primary-container text-on-primary-container text-label-xs font-bold">
            {t('memory.dream.proposals.count', { count: proposals.length })}
          </span>
        )}
      </div>

      {loading ? (
        <div className="text-label-sm text-on-surface-variant py-md">{t('memory.dream.proposals.loading')}</div>
      ) : proposals.length === 0 ? (
        <div className="text-label-sm text-on-surface-variant py-md">{t('memory.dream.proposals.empty')}</div>
      ) : (
        <div
          role="list"
          aria-label={t('memory.dream.proposals.title')}
          // Audit §P2-2: scrollable review list stays keyboard-reachable.
          tabIndex={0}
          className="space-y-md max-h-[480px] overflow-y-auto pr-xs outline-none focus-visible:ring-2 focus-visible:ring-primary/20 rounded-xl"
        >
          {proposals.map((proposal) => {
            const selectedIds = selected[proposal.id] ?? new Set<string>()
            const busy = busyProposalId === proposal.id
            return (
              <div
                key={proposal.id}
                role="listitem"
                className="rounded-xl border border-outline-variant/30 bg-surface-container-low p-md"
              >
                <div className="flex items-center gap-sm flex-wrap mb-sm">
                  <span className="font-label-md text-[14px] font-bold text-on-surface break-all">{proposal.project}</span>
                  <span className="font-label-sm text-[12px] text-on-surface-variant">
                    {new Date(proposal.created_at).toLocaleString([], { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
                  </span>
                  <span className="ml-auto flex items-center gap-xs">
                    {(['merge', 'remove', 'add'] as const).map((kind) => {
                      const n = countByKind(proposal, kind)
                      if (n === 0) return null
                      const meta = KIND_META[kind]
                      return (
                        <span key={kind} className={cn('inline-flex items-center gap-[2px] px-sm py-[2px] rounded-full text-label-xs font-bold', meta.badgeClass)}>
                          <span className="material-symbols-outlined text-[13px]" aria-hidden="true">{meta.icon}</span>
                          {t(meta.labelKey)} {n}
                        </span>
                      )
                    })}
                  </span>
                </div>

                <div className="space-y-xs mb-md">
                  {proposal.actions.map((action) => {
                    const checked = selectedIds.has(action.id)
                    return (
                      <label
                        key={action.id}
                        className="flex items-start gap-sm px-sm py-xs rounded-lg hover:bg-surface-container cursor-pointer"
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={() => toggleAction(proposal.id, action.id)}
                          aria-label={t('memory.dream.action.select.aria', { index: action.id })}
                          className="w-4 h-4 mt-[2px] accent-primary cursor-pointer shrink-0"
                        />
                        <span className="min-w-0">
                          <span className={cn('block text-body-sm font-medium', checked ? 'text-on-surface' : 'text-on-surface-variant')}>
                            {action.rationale}
                          </span>
                          {action.kind === 'add' && action.add_entry && (
                            <span className="block text-label-sm text-on-surface-variant mt-[2px]">
                              <span className="px-xs py-[1px] mr-xs rounded bg-tertiary-container text-on-tertiary-container text-[11px] font-bold uppercase">
                                {action.add_entry.category}
                              </span>
                              {action.add_entry.content}
                            </span>
                          )}
                          {action.kind !== 'add' && (
                            <span className="block text-label-sm text-on-surface-variant mt-[2px]">
                              {t('memory.dream.action.entries', { count: action.entry_ids.length })}
                            </span>
                          )}
                        </span>
                      </label>
                    )
                  })}
                </div>

                <div className="flex items-center gap-sm">
                  <Button
                    size="sm"
                    disabled={selectedIds.size === 0 || busy}
                    onClick={() => void handleApply(proposal)}
                    className="gap-xs px-md py-sm text-[13px] font-bold"
                  >
                    {busy ? <Spinner className="text-[16px]" /> : <span className="material-symbols-outlined text-[16px]">check</span>}
                    {t('memory.dream.apply')}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy}
                    onClick={() => setPendingDiscardId(proposal.id)}
                    className="gap-xs px-sm py-sm text-[13px] text-on-surface-variant hover:text-error"
                  >
                    <span className="material-symbols-outlined text-[16px]">delete_sweep</span>
                    {t('memory.dream.discard')}
                  </Button>
                  <span className="ml-auto font-label-sm text-[12px] text-on-surface-variant">
                    {t('memory.dream.selectedCount', { count: selectedIds.size, total: proposal.actions.length })}
                  </span>
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* Report viewer — the backend emits markdown; plain pre-wrap text is
          intentional (no markdown dependency was added for this). */}
      <Modal
        open={reportOpen}
        onClose={() => setReportOpen(false)}
        title={t('memory.dream.report.title')}
        size="2xl"
      >
        <div className="px-xl pb-xl">
          {reportLoading ? (
            <div className="text-label-sm text-on-surface-variant py-md">{t('memory.dream.report.loading')}</div>
          ) : reportError ? (
            <div
              role="alert"
              className="text-body-sm text-error py-md"
            >
              {t('memory.dream.report.error')}
            </div>
          ) : (
            <div
              tabIndex={0}
              aria-label={t('memory.dream.report.title')}
              className="max-h-[60vh] overflow-y-auto rounded-xl bg-surface-container-low border border-outline-variant/30 p-md outline-none focus-visible:ring-2 focus-visible:ring-primary/20"
            >
              <pre className="whitespace-pre-wrap break-words font-body-sm text-body-sm text-on-surface-variant m-0">
                {report ?? t('memory.dream.report.empty')}
              </pre>
            </div>
          )}
        </div>
      </Modal>

      <ConfirmDialog
        open={pendingDiscardId !== null}
        title={t('memory.dream.discardConfirm.title')}
        message={t('memory.dream.discardConfirm.message')}
        confirmLabel={t('memory.dream.discardConfirm.confirm')}
        cancelLabel={t('memory.dream.discardConfirm.cancel')}
        destructive
        onConfirm={() => void confirmDiscard()}
        onCancel={() => setPendingDiscardId(null)}
      />
    </section>
  )
}

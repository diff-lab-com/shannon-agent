// Triage page — the automation inbox (P0-3). Backed by the SQLite inbox
// Tauri commands (`list_inbox_items` / `update_inbox_item_status` /
// `get_inbox_stats` / `rerun_inbox_item` / `continue_inbox_item_session`).
// Items are produced by scheduled routine runs (`routine`/`scheduled_task`),
// goal events, the external trigger endpoint, parallel batch-run
// completions (`batch`), session permission prompts / failed turns
// (`session_approval`/`session_failed`) and detected skill candidates
// (`skill_candidate`) — see `inbox_commands.rs` / `inbox_session_events.rs`.
//
// Layout: header with stats summary → filter bar (status chips + source
// chips + sort) → bulk-action bar when items selected → list of InboxCard
// cards with checkbox + Mark Read / Archive / View-or-Continue session /
// Rerun / Review-candidate actions, expandable error details, empty states.
//
// The page keeps the previous triage page's visual language (glass-panel
// cards, chip filters, keyboard j/k navigation, bulk selection bar).

import { useState, useMemo, useCallback, useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent } from 'react'
import { useLocation, useNavigate, useSearchParams } from 'react-router-dom'
import { useIntl, type PrimitiveType } from 'react-intl'
import { toast } from 'sonner'
import EmptyState from '@/components/ui/empty-state'
import ErrorState from '@/components/ui/error-state'
import { CardSkeleton } from '@/components/SkeletonLoader'
import { Button } from '@/components/ui/button'
import { DropdownMenu, type DropdownMenuItem } from '@/components/ui/dropdown-menu'
import { useInboxItems, useInboxStats } from '@/hooks/inbox'
import { useProjectDeepLink } from '@/hooks/projectDeepLink'
import ProjectFilterChip from '@/components/ProjectFilterChip'
import { useSessions } from '@/context/SessionContext'
import { projectKeyOf } from '@/components/SidebarSessions'
import type { InboxItem, InboxItemStatus, InboxSource } from '@/types'
import * as api from '@/lib/tauri-api'
import { cn } from '@/lib/utils'

type StatusFilter = InboxItemStatus | undefined
type SourceFilter = InboxSource | undefined
type SortOrder = 'newest' | 'oldest'

/// Sources whose backing routine can be re-executed via `rerun_inbox_item`.
/// `goal`/`trigger` items have no runnable task behind them (the backend
/// rejects reruns for them), so the UI disables the action instead.
const RERUNNABLE_SOURCES: readonly InboxSource[] = ['routine', 'scheduled_task']

/// IA T6: session-scoped sources (permission prompt / failed turn). Their
/// card's primary action is "View session" — the session *is* the thing to
/// resolve — instead of the automation-facing "Continue session" wording.
const SESSION_SOURCES: readonly InboxSource[] = ['session_approval', 'session_failed']

function canRerun(item: InboxItem): boolean {
  return RERUNNABLE_SOURCES.includes(item.source) && item.status !== 'archived'
}

// IA 2026-09 (T2 互链闭环): routine/scheduled_task items carry the id of the
// automation that produced them, so the card can link back to /tasks and
// open its RoutineDetailDrawer.
function canOpenSource(item: InboxItem): boolean {
  return RERUNNABLE_SOURCES.includes(item.source) && item.sourceId != null
}

/// Exported for tests (source → icon/colour/label mapping is a contract the
/// i18n keys depend on).
export function sourceMeta(source: InboxSource): { icon: string; color: string; labelKey: string } {
  switch (source) {
    case 'routine':
      return { icon: 'event_repeat', color: 'text-primary', labelKey: 'inbox.source.routine' }
    case 'scheduled_task':
      return { icon: 'task_alt', color: 'text-primary', labelKey: 'inbox.source.scheduled_task' }
    case 'goal':
      return { icon: 'flag', color: 'text-secondary', labelKey: 'inbox.source.goal' }
    case 'trigger':
      return { icon: 'bolt', color: 'text-error', labelKey: 'inbox.source.trigger' }
    case 'batch':
      // Parallel batch run's aggregate completion record (T3) — same visual
      // language as the batch runner panel (call_split icon, tertiary color).
      return { icon: 'call_split', color: 'text-tertiary', labelKey: 'inbox.source.batch' }
    case 'session_approval':
      // T5: a session permission prompt is waiting on the user (lock_open =
      // an action is gated); secondary color reads "needs your action".
      return { icon: 'lock_open', color: 'text-secondary', labelKey: 'inbox.source.session_approval' }
    case 'session_failed':
      // T5: the session's last turn failed — same error color as `trigger`.
      return { icon: 'error', color: 'text-error', labelKey: 'inbox.source.session_failed' }
    case 'skill_candidate':
      // T5: a detected skill pattern awaits review (auto_awesome = the same
      // sparkles language the skill catalog uses).
      return { icon: 'auto_awesome', color: 'text-tertiary', labelKey: 'inbox.source.skill_candidate' }
    case 'dream_report':
      // Dream pass (梦境提炼) daily summary card — bedtime icon + tertiary,
      // the same language the Memory page's distillation section uses.
      return { icon: 'bedtime', color: 'text-tertiary', labelKey: 'inbox.source.dream_report' }
    default:
      return { icon: 'notifications', color: 'text-on-surface-variant', labelKey: 'inbox.source.trigger' }
  }
}

export const STATUS_OPTIONS: readonly (InboxItemStatus | 'all')[] = ['all', 'pending', 'read', 'archived']
export const SOURCE_OPTIONS: readonly (InboxSource | 'all')[] = ['all', 'routine', 'scheduled_task', 'goal', 'trigger', 'batch', 'session_approval', 'session_failed', 'skill_candidate', 'dream_report']

// ─── B4 #28: URL-persisted view state ──────────────────────────────────────
// Filters, sort and grouping live in the search params (?status=&source=
// &sort=&group=source) so a triage session survives reloads and can be
// bookmarked — same convention as the ?project= deep link. Unknown values
// fall back to the defaults instead of silently narrowing the list.
const STATUS_VALUES: readonly InboxItemStatus[] = ['pending', 'read', 'archived']

function parseStatusParam(v: string | null): StatusFilter {
  return STATUS_VALUES.includes(v as InboxItemStatus) ? (v as InboxItemStatus) : undefined
}

function parseSourceParam(v: string | null): SourceFilter {
  return v !== null && v !== 'all' && SOURCE_OPTIONS.includes(v as InboxSource) ? (v as InboxSource) : undefined
}

function InboxCard({ item, selected, focused, highlighted, onToggleSelected, onMarkRead, onArchive, onContinue, onRerun, onOpenSource, onReview, onViewReport }: {
  item: InboxItem
  selected: boolean
  focused?: boolean
  /** IA T2: one-shot ring when the user arrived from HistoryView. */
  highlighted?: boolean
  onToggleSelected: (id: number) => void
  onMarkRead: (id: number) => void
  onArchive: (id: number) => void
  onContinue: (item: InboxItem) => void
  onRerun: (item: InboxItem) => void
  onOpenSource: (item: InboxItem) => void
  /** IA T6/X1: jump to the Extensions → Pending review queue for this candidate. */
  onReview: (item: InboxItem) => void
  /** Dream report: open the Memory page's distillation section (report +
      proposal review live there — there is no per-item detail view). */
  onViewReport: (item: InboxItem) => void
}) {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const [showError, setShowError] = useState(false)
  const meta = sourceMeta(item.source)
  const rerunnable = canRerun(item)
  const openSource = canOpenSource(item)
  const isPending = item.status === 'pending'
  // IA T6: approval/failed items reframe "resume" as "view" — the user is
  // going to the session to answer a permission prompt or read the failure.
  const isSessionSource = SESSION_SOURCES.includes(item.source)
  // IA X1: skill candidates are assets, not run results — the card links to
  // the rich review queue (Extensions → Pending) instead of approving inline
  // (评审裁决 #2: 收件箱只放发现条目，不做卡内审批).
  const reviewable = item.source === 'skill_candidate' && item.sourceId != null
  // Dream pass: the report + the proposal review both live on the Memory
  // page's distillation section, so the card's action is a single jump.
  const isDreamReport = item.source === 'dream_report'

  return (
    <div role="listitem" data-focused={focused ? 'true' : undefined} data-highlight={highlighted ? 'true' : undefined} className={cn('glass-panel border rounded-xl p-md shadow-sm hover:shadow-md transition-all group bg-surface-container-lowest/80', isPending ? 'border-primary/20' : 'border-outline-variant/10', focused ? 'ring-2 ring-primary' : highlighted ? 'ring-2 ring-tertiary' : selected ? 'ring-2 ring-primary/40' : '')}>
      <div className="flex items-start gap-sm">
        <label className="flex items-center pt-xs cursor-pointer shrink-0" aria-label={t('inbox.select.aria', { id: item.id })}>
          <input
            type="checkbox"
            checked={selected}
            onChange={() => onToggleSelected(item.id)}
            className="w-4 h-4 accent-primary cursor-pointer"
          />
        </label>
        <div className="flex items-start gap-md flex-1 min-w-0">
          <div className={cn("w-10 h-10 rounded-xl bg-surface-container-low flex items-center justify-center", meta.color, "shrink-0")}>
            <span className="material-symbols-outlined icon-lg" aria-hidden="true">{meta.icon}</span>
          </div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-sm mb-xs flex-wrap">
              <span className={cn("font-label-sm text-[11px] font-bold uppercase tracking-wider", meta.color)}>{t(meta.labelKey)}</span>
              {isPending && <span className="w-2 h-2 rounded-full bg-primary shrink-0" title={t('inbox.pending.title')} />}
              {item.status === 'archived' && <span className="font-label-sm text-[11px] text-on-surface-variant">{t('inbox.status.archived')}</span>}
            </div>
            <p className={cn("text-body-sm font-medium mb-xs break-words", isPending ? "text-on-surface" : "text-on-surface-variant")}>{item.title}</p>
            {item.summary && (
              <p className="text-body-sm text-on-surface-variant mb-xs break-words whitespace-pre-wrap">{item.summary}</p>
            )}
            {item.error && (
              <div className="mb-xs">
                <Button
                  variant="ghost"
                  aria-expanded={showError}
                  aria-controls={`inbox-error-${item.id}`}
                  onClick={() => setShowError(v => !v)}
                  className="px-xs py-0.5 rounded-lg text-label-sm text-error hover:bg-error/10 cursor-pointer inline-flex items-center gap-xs"
                >
                  <span className="material-symbols-outlined text-[14px]" aria-hidden="true">warning</span>
                  {t('inbox.error.label')}
                  <span className="material-symbols-outlined text-[14px]" aria-hidden="true">{showError ? 'expand_less' : 'expand_more'}</span>
                </Button>
                {showError && (
                  <pre id={`inbox-error-${item.id}`} className="mt-xs px-sm py-xs rounded-lg bg-surface-container-low border border-outline-variant/30 text-label-sm text-error whitespace-pre-wrap break-words max-h-48 overflow-y-auto">{item.error}</pre>
                )}
              </div>
            )}
            <div className="flex items-center gap-md flex-wrap">
              <span className="font-label-sm text-label-sm text-on-surface-variant flex items-center gap-xs">
                <span className="material-symbols-outlined text-[14px]" aria-hidden="true">schedule</span>
                {/* B4 #28: format in the app's locale, not the OS default. */}
                {new Date(item.createdAtMs).toLocaleString(intl.locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })}
              </span>
            </div>
          </div>
        </div>
        <div className="flex items-center gap-sm shrink-0">
          {/* IA T2: routine/scheduled_task results link back to the
              automation that produced them (opens RoutineDetailDrawer on
              /tasks). 「继续会话」 stays the primary action. */}
          {openSource && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={t('inbox.openSource.aria')}
              title={t('inbox.openSource.aria')}
              className="cursor-pointer inline-flex items-center gap-xs text-on-surface-variant hover:text-primary"
              onClick={() => onOpenSource(item)}
            >
              <span className="material-symbols-outlined text-[16px]">event_repeat</span>
              {t('inbox.openSource.label')}
            </Button>
          )}
          {/* IA X1: skill candidates link to the Extensions → Pending review
              queue, carrying the candidate id so the queue can focus it. */}
          {reviewable && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={t('inbox.review.aria')}
              title={t('inbox.review.aria')}
              className="cursor-pointer inline-flex items-center gap-xs text-tertiary hover:text-primary"
              onClick={() => onReview(item)}
            >
              <span className="material-symbols-outlined text-[16px]">rate_review</span>
              {t('inbox.review.label')}
            </Button>
          )}
          {/* Dream report: 查看报告 → the Memory page's distillation section
              (mirrors the reviewable/openSource jump pattern for sources with
              no runnable session behind them). */}
          {isDreamReport && (
            <Button
              size="sm"
              variant="ghost"
              aria-label={t('inbox.viewReport.aria')}
              title={t('inbox.viewReport.aria')}
              className="cursor-pointer inline-flex items-center gap-xs text-tertiary hover:text-primary"
              onClick={() => onViewReport(item)}
            >
              <span className="material-symbols-outlined text-[16px]">bedtime</span>
              {t('inbox.viewReport.label')}
            </Button>
          )}
          {item.sessionId && (
            /* Primary action (audit §3.4): the Codex review-queue loop is
               "result → resume the original thread". IA T6: for approval /
               failed sources the same jump reads "View session" — the target
               is a permission prompt or a failure to inspect. */
            <Button
              size="sm"
              aria-label={t(isSessionSource ? 'inbox.action.viewSession.aria' : 'inbox.continue.aria')}
              className="cursor-pointer inline-flex items-center gap-xs"
              onClick={() => onContinue(item)}
            >
              <span className="material-symbols-outlined text-[16px]">{isSessionSource ? 'visibility' : 'forum'}</span>
              {t(isSessionSource ? 'inbox.action.viewSession' : 'inbox.action.resume')}
            </Button>
          )}
          <Button
            aria-label={t('inbox.rerun.aria')}
            variant="ghost"
            disabled={!rerunnable}
            className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
            onClick={() => onRerun(item)}
            title={rerunnable ? t('inbox.rerun.title') : t('inbox.rerun.disabled.title')}
          >
            <span className="material-symbols-outlined text-[18px]">replay</span>
          </Button>
          {isPending && (
            <Button
              aria-label={t('inbox.markRead.aria', { id: item.id })}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onMarkRead(item.id)}
              title={t('inbox.markRead.title')}
            >
              <span className="material-symbols-outlined text-[18px]">check</span>
            </Button>
          )}
          {item.status !== 'archived' && (
            <Button
              aria-label={t('inbox.archive.aria', { id: item.id })}
              variant="ghost"
              className="p-2 rounded-lg hover:bg-surface-container-low text-on-surface-variant cursor-pointer"
              onClick={() => onArchive(item.id)}
              title={t('inbox.archive.title')}
            >
              <span className="material-symbols-outlined text-[18px]">archive</span>
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}

export default function Triage() {
  const intl = useIntl()
  const t = (id: string, values?: Record<string, PrimitiveType>) => intl.formatMessage({ id }, values)
  const navigate = useNavigate()
  const location = useLocation()
  // P-U3: the session list doubles as the project join — an inbox item's
  // project is the working_dir of the session it came from (items carry no
  // project of their own by design; the inbox table was left untouched).
  const { switchSession, sessions = [] } = useSessions()
  // P-U3: /triage?project=<encoded path> — hide items whose session's
  // working_dir doesn't map to the project (items with no session drop out
  // too). The chip removes the param.
  const { projectKey, projectLabel, clearProject } = useProjectDeepLink()

  // B4 #28: view state (status / source / sort / grouping) is URL-owned.
  // The chips write search params (replace navigation — no history spam,
  // same convention as the ?project= chip); the values below are derived.
  const [searchParams, setSearchParams] = useSearchParams()
  const statusFilter = parseStatusParam(searchParams.get('status'))
  const sourceFilter = parseSourceParam(searchParams.get('source'))
  const sortOrder: SortOrder = searchParams.get('sort') === 'oldest' ? 'oldest' : 'newest'
  // 2026-09 P0-4: Triage list can render flat (default) or grouped by
  // source (each source becomes a collapsible folder). The grouping mode
  // sits next to the source filter dropdown so the relationship reads.
  const groupBySource = searchParams.get('group') === 'source'
  const updateParams = useCallback((patch: Record<string, string | null>) => {
    const next = new URLSearchParams(searchParams)
    for (const [key, value] of Object.entries(patch)) {
      if (value == null) next.delete(key)
      else next.set(key, value)
    }
    setSearchParams(next, { replace: true })
  }, [searchParams, setSearchParams])
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const [bulkRunning, setBulkRunning] = useState(false)
  // B4 P1-28: the keyboard cursor is a GLOBAL flat index over the rendered
  // order (grouped mode included) — see `flatItems` below. A bucket-local
  // index used to let several cards ring at once while Enter/a acted on a
  // different card than the one that lit up.
  const [focusedIndex, setFocusedIndex] = useState<number | null>(null)
  const listRef = useRef<HTMLDivElement>(null)

  // IA T2: HistoryView hands over `highlightInboxId` via router state. The
  // highlight is one-shot — snapshotted once (so the ring survives the
  // state-clearing replace below but dies with the page) and never turned
  // into a filter.
  const [highlightId] = useState<number | null>(
    () => (location.state as { highlightInboxId?: number } | null)?.highlightInboxId ?? null,
  )
  useEffect(() => {
    if (highlightId == null) return
    // B4 #28: keep the search string — view state now lives in the params,
    // only the hand-over state is being drained here.
    navigate({ pathname: location.pathname, search: location.search }, { replace: true })
    // Run once per mount — the point is to drain the router state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const { stats } = useInboxStats()
  // B4 #28: `error` is consumed below — an IPC failure used to render as an
  // empty inbox, indistinguishable from "all clear".
  const { items, loading, error, markRead, archive, rerun, getSessionId, setFilter, refresh } = useInboxItems()

  // Server-side filtering: status + source chips map 1:1 onto the
  // `list_inbox_items` filter args; sort stays client-side. Both hooks
  // subscribe to `inbox-updated` themselves, so the header chips and the
  // list stay in sync after every status write / new run item.
  useEffect(() => {
    setFilter({ status: statusFilter, source: sourceFilter })
  }, [statusFilter, sourceFilter, setFilter])

  // Scroll the highlighted card into view once the list has rendered.
  useEffect(() => {
    if (highlightId == null || loading) return
    listRef.current?.querySelector('[data-highlight="true"]')?.scrollIntoView({ block: 'center' })
  }, [highlightId, loading])

  // P-U3: session_id → working_dir map for the project join (normalized to
  // the same trailing-slash-free key the rail's project tree uses).
  const dirBySessionId = useMemo(() => {
    const m = new Map<string, string | null>()
    for (const s of sessions) m.set(s.id, projectKeyOf(s))
    return m
  }, [sessions])

  const visibleItems = useMemo(() => {
    // P-U3: project scope first — only items whose linked session lives in
    // the deep-linked project survive it.
    const scoped = projectKey
      ? items.filter(i => i.sessionId != null && dirBySessionId.get(i.sessionId) === projectKey)
      : items
    // IA T6: pending (unread) items float to the top — the inbox answers
    // "what needs me" first. Within each band the time sort stays stable,
    // so the toggle below only reorders inside a band.
    // 卡 3a: `upsert_pending` refreshes an existing row's `updatedAtMs` in
    // place instead of appending, so the band sort keys on it (falling back
    // to `createdAtMs` for rows without one) — a same-session failure that
    // re-fails floats back to the top of its band instead of staying buried
    // under newer entries.
    const sorted = [...scoped].sort((a, b) => {
      const aPending = a.status === 'pending' ? 0 : 1
      const bPending = b.status === 'pending' ? 0 : 1
      if (aPending !== bPending) return aPending - bPending
      const diff = (a.updatedAtMs ?? a.createdAtMs) - (b.updatedAtMs ?? b.createdAtMs)
      return sortOrder === 'newest' ? -diff : diff
    })
    return sorted
  }, [items, sortOrder, projectKey, dirBySessionId])

  // B4 P1-28: the one ordering the keyboard cursor is allowed to talk about.
  // Grouped mode renders buckets in SOURCE_OPTIONS order, so the flat list
  // is exactly that concatenation — a single global index drives both the
  // visible ring and what Enter/a act on, in either render mode.
  const flatItems = useMemo(() => {
    if (!groupBySource) return visibleItems
    const out: InboxItem[] = []
    for (const src of SOURCE_OPTIONS) {
      if (src === 'all') continue
      for (const it of visibleItems) if (it.source === src) out.push(it)
    }
    return out
  }, [groupBySource, visibleItems])

  // Card id → position in `flatItems`; render looks the focus up from here
  // so a card rings iff the keyboard cursor points at it.
  const flatIndexById = useMemo(
    () => new Map(flatItems.map((it, idx) => [it.id, idx])),
    [flatItems],
  )

  // Drop selections that no longer match the visible list.
  const effectiveSelected = useMemo(() => {
    const visibleIds = new Set(visibleItems.map(i => i.id))
    const next = new Set<number>()
    for (const id of selectedIds) if (visibleIds.has(id)) next.add(id)
    return next
  }, [selectedIds, visibleItems])

  const allSelected = visibleItems.length > 0 && effectiveSelected.size === visibleItems.length

  const toggleSelected = useCallback((id: number) => {
    setSelectedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const toggleSelectAll = useCallback(() => {
    if (allSelected) setSelectedIds(new Set())
    else setSelectedIds(new Set(visibleItems.map(i => i.id)))
  }, [allSelected, visibleItems])

  const clearSelection = useCallback(() => setSelectedIds(new Set()), [])

  // B4 P1-30: restore previously-captured statuses (the Undo path behind
  // archive). Best-effort per item; a failed restore is reported instead of
  // swallowed.
  const undoStatuses = useCallback(async (entries: Array<[number, InboxItemStatus]>) => {
    const results = await Promise.allSettled(
      entries.map(([id, status]) => api.updateInboxItemStatus(id, status)),
    )
    await refresh()
    const failed = results.filter(r => r.status === 'rejected').length
    if (failed > 0) {
      toast.error(intl.formatMessage({ id: 'inbox.undo.failed' }, { count: failed }))
    }
  }, [refresh, intl])

  const bulkSetStatus = useCallback(async (status: InboxItemStatus, toastKey: string, toastKeyPlural: string) => {
    const ids = Array.from(effectiveSelected)
    if (ids.length === 0) return
    // Snapshot the pre-operation statuses so Undo restores what was there
    // before (a `read` item bulk-archived comes back as `read`, not pending).
    const prevById = new Map(items.map(i => [i.id, i.status]))
    setBulkRunning(true)
    const results = await Promise.allSettled(ids.map(id => api.updateInboxItemStatus(id, status)))
    await refresh()
    setBulkRunning(false)
    const okIds: number[] = []
    const failedIds: number[] = []
    results.forEach((r, idx) => { (r.status === 'fulfilled' ? okIds : failedIds).push(ids[idx]) })
    if (okIds.length === 0) {
      // B4 P1-30: a total failure used to pass in silence with the selection
      // cleared — the worst possible read ("everything worked, inbox empty").
      toast.error(intl.formatMessage({ id: 'inbox.bulk.toast.failedAll' }, { count: ids.length }))
      return
    }
    const successMsg = intl.formatMessage(
      { id: okIds.length === 1 ? toastKey : toastKeyPlural },
      { count: okIds.length },
    )
    if (failedIds.length > 0) {
      // Partial failure: report both sides truthfully, keep the failed items
      // selected so a retry is one click away.
      toast.error(intl.formatMessage(
        { id: 'inbox.bulk.toast.partial' },
        { written: okIds.length, total: ids.length, failed: failedIds.length },
      ))
      setSelectedIds(new Set(failedIds))
      return
    }
    if (status === 'archived') {
      // B4 P1-30: bulk archive is recoverable — the toast carries an Undo
      // that writes every item's pre-operation status back.
      const undoEntries: Array<[number, InboxItemStatus]> =
        okIds.map(id => [id, prevById.get(id) ?? 'pending'])
      toast.success(successMsg, {
        action: {
          label: intl.formatMessage({ id: 'inbox.undo' }),
          onClick: () => { void undoStatuses(undoEntries) },
        },
      })
    } else {
      toast.success(successMsg)
    }
    clearSelection()
  }, [effectiveSelected, refresh, clearSelection, intl, items, undoStatuses])

  const bulkMarkRead = useCallback(
    () => bulkSetStatus('read', 'inbox.bulk.toast.markRead', 'inbox.bulk.toast.markRead.plural'),
    [bulkSetStatus],
  )
  const bulkArchive = useCallback(
    () => bulkSetStatus('archived', 'inbox.bulk.toast.archived', 'inbox.bulk.toast.archived.plural'),
    [bulkSetStatus],
  )

  const handleContinue = useCallback(async (item: InboxItem) => {
    const sessionId = await getSessionId(item.id)
    if (!sessionId) return
    await switchSession(sessionId)
    navigate('/chat')
  }, [getSessionId, switchSession, navigate])

  const handleRerun = useCallback(async (item: InboxItem) => {
    await rerun(item.id)
  }, [rerun])

  // IA T2: back-link to the automation that produced the item — /tasks
  // consumes `openRoutineId` from router state to open RoutineDetailDrawer.
  const handleOpenSource = useCallback((item: InboxItem) => {
    navigate('/tasks', { state: { openRoutineId: item.sourceId } })
  }, [navigate])

  // IA T6/X1: hand the candidate over to the Extensions → Pending review
  // queue (the single skill-review surface, 评审裁决 #2). The queue consumes
  // `skillCandidateId` for a one-shot focus, mirroring the T2 pattern.
  const handleReview = useCallback((item: InboxItem) => {
    navigate('/extensions/pending', { state: { skillCandidateId: item.sourceId } })
  }, [navigate])

  // Dream report: the distillation section (run / proposals / report) lives
  // on the Memory page — same "jump to the owning surface" pattern as T2/X1.
  const handleViewReport = useCallback((_item: InboxItem) => {
    navigate('/memory')
  }, [navigate])

  // Keyboard navigation over the inbox list (list must be focused first).
  // j/ArrowDown = next, k/ArrowUp = previous, Enter = mark read, a = archive.
  // The cursor is a global flat index over `flatItems` (B4 P1-28) so the
  // ringed card is always the one an action would hit. Ignored when the
  // keystroke originates from a form field so the filter chips keep working
  // normally.
  useEffect(() => {
    if (focusedIndex == null) return
    if (flatItems.length === 0) { setFocusedIndex(null); return }
    if (focusedIndex >= flatItems.length) setFocusedIndex(flatItems.length - 1)
  }, [flatItems, focusedIndex])

  useEffect(() => {
    if (focusedIndex == null) return
    const el = listRef.current?.querySelector('[data-focused="true"]') as HTMLElement | null
    el?.scrollIntoView({ block: 'nearest' })
  }, [focusedIndex])

  const handleListKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    const el = e.target as HTMLElement
    const tag = el.tagName
    if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable) return
    const max = flatItems.length
    if (max === 0) return
    const cur = focusedIndex ?? -1
    if (e.key === 'j' || e.key === 'ArrowDown') {
      e.preventDefault()
      setFocusedIndex(Math.min(cur + 1, max - 1))
    } else if (e.key === 'k' || e.key === 'ArrowUp') {
      e.preventDefault()
      setFocusedIndex(Math.max(cur - 1, 0))
    } else if (e.key === 'Enter' || e.key === 'a') {
      // B4 P1-29: a keystroke aimed at a focused button belongs to that
      // button — Enter on「查看会话」used to be swallowed and mark the item
      // read instead. Same passthrough rule as useDiffKeyboard (T5).
      if (tag === 'BUTTON' || el.closest('[role="button"]')) return
      if (cur < 0) return
      const it = flatItems[cur]
      if (!it) return
      if (e.key === 'Enter' && it.status === 'pending') { e.preventDefault(); markRead(it.id) }
      else if (e.key === 'a' && it.status !== 'archived') { e.preventDefault(); archive(it.id) }
    }
  }

  return (
    <div className="flex-1 overflow-y-auto w-full pb-16">
      <div className="max-w-[1200px] mx-auto px-lg py-xl">
        {/* Header — the page title is rendered globally in the app Header;
            here we keep the one-line subtitle so first-time users get the
            "计划任务、目标与触发器..." context without a duplicate H1. */}
        <div className="flex flex-col md:flex-row md:items-end justify-between mb-xl gap-md">
          <p className="text-on-surface-variant">{t('inbox.subtitle')}</p>
          <div className="flex items-center gap-md">
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-primary" aria-hidden="true">mark_email_unread</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'inbox.stats.pending' }, { count: stats.pending })}</span>
            </div>
            <div className="flex items-center gap-sm px-md py-sm rounded-xl bg-surface-container-lowest border border-outline-variant/30">
              <span className="material-symbols-outlined text-[18px] text-on-surface-variant" aria-hidden="true">today</span>
              <span className="font-label-md text-on-surface">{intl.formatMessage({ id: 'inbox.stats.today' }, { count: stats.today })}</span>
            </div>
          </div>
        </div>

        {/* P-U3: project deep-link chip — × strips ?project= and the full
            inbox comes back. */}
        {projectKey && projectLabel && (
          <ProjectFilterChip label={projectLabel} onRemove={clearProject} />
        )}

        {/* Filter bar: status */}
        <div className="flex items-center gap-sm mb-sm flex-wrap">
          <span className="font-label-sm text-on-surface-variant uppercase tracking-wider mr-xs">{t('inbox.status.label')}</span>
          {STATUS_OPTIONS.map(opt => {
            const active = statusFilter === (opt === 'all' ? undefined : opt)
            return (
              <Button
                key={opt}
                variant="ghost"
                onClick={() => updateParams({ status: opt === 'all' ? null : opt })}
                aria-pressed={active}
                className={cn("px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer", active ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10')}
              >
                {t(opt === 'all' ? 'inbox.filter.all' : `inbox.status.${opt}`)}
              </Button>
            )
          })}
        </div>

        {/* Q4 2026-09: source chips (6) overflow on narrow windows — collapse
            them into a dropdown that mirrors the wide layout's semantics. */}
        <div className="flex items-center gap-sm mb-lg flex-wrap">
          <SourceFilterDropdown
            value={sourceFilter}
            onChange={src => updateParams({ source: src ?? null })}
            sourceMeta={sourceMeta}
            allLabel={t('inbox.filter.all')}
            label={t('inbox.source.label')}
          />
          <Button
            type="button"
            variant="ghost"
            onClick={() => updateParams({ group: groupBySource ? null : 'source' })}
            aria-pressed={groupBySource}
            aria-label={t('inbox.groupBySource')}
            title={t('inbox.groupBySource')}
            className={cn(
              'px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer',
              groupBySource
                ? 'bg-primary/10 text-primary font-bold'
                : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10',
            )}
          >
            <span className="material-symbols-outlined text-[14px] mr-xs align-middle" aria-hidden="true">folder_open</span>
            {t('inbox.groupBySource')}
          </Button>
          <Button
            variant="ghost"
            onClick={() => updateParams({ sort: sortOrder === 'newest' ? 'oldest' : 'newest' })}
            aria-label={t('inbox.sort.aria')}
            className="ml-auto px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10"
          >
            <span className="material-symbols-outlined text-[14px] mr-xs align-middle" aria-hidden="true">
              {sortOrder === 'newest' ? 'arrow_downward' : 'arrow_upward'}
            </span>
            {t(sortOrder === 'newest' ? 'inbox.sort.newest' : 'inbox.sort.oldest')}
          </Button>
        </div>

        {/* Bulk-action bar (visible when items are selected) */}
        {effectiveSelected.size > 0 ? (
          <div
            role="region"
            aria-label={t('inbox.bulk.title')}
            className="sticky top-0 z-raised mb-md flex items-center gap-md px-md py-sm rounded-xl bg-primary/10 border border-primary/30 backdrop-blur-md"
          >
            <span className="font-label-md text-primary font-bold">
              {intl.formatMessage({ id: 'inbox.bulk.selected' }, { count: effectiveSelected.size })}
            </span>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkMarkRead}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined icon-sm mr-xs align-middle" aria-hidden="true">done_all</span>
              {t('inbox.bulk.markRead')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={bulkArchive}
              className="px-sm py-xs rounded-lg text-label-md text-on-surface hover:bg-primary/20 cursor-pointer disabled:opacity-50 disabled:cursor-not-allowed"
            >
              <span className="material-symbols-outlined icon-sm mr-xs align-middle" aria-hidden="true">archive</span>
              {t('inbox.bulk.archive')}
            </Button>
            <Button
              variant="ghost"
              disabled={bulkRunning}
              onClick={clearSelection}
              className="ml-auto px-sm py-xs rounded-lg text-label-md text-on-surface-variant hover:text-primary cursor-pointer"
            >
              {t('inbox.bulk.clear')}
            </Button>
          </div>
        ) : null}

        {/* List */}
        {loading ? (
          <div className="space-y-md">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        ) : error && items.length === 0 ? (
          // B4 #28: an IPC failure must not read as an empty inbox. With no
          // stale rows to show, the failure gets the full error state.
          <ErrorState
            icon="inbox"
            title={t('inbox.errorState.title')}
            description={t('inbox.errorState.description')}
            action={{ label: t('inbox.errorState.retry'), onClick: () => void refresh() }}
          />
        ) : items.length === 0 ? (
          <EmptyState
            icon="inbox"
            title={t('inbox.empty.title')}
            description={t('inbox.empty.description')}
            action={{ label: t('inbox.empty.cta'), onClick: () => void refresh() }}
          />
        ) : (
          <>
            {/* Stale-rows banner: the last refresh failed but older items are
                still on screen — say so instead of letting them pass as fresh
                (T1: failure and empty must never look alike). */}
            {error && (
              <div
                role="alert"
                className="mb-md flex items-center gap-sm px-md py-sm rounded-xl bg-error/10 border border-error/30 text-error"
              >
                <span className="material-symbols-outlined text-[18px]" aria-hidden="true">cloud_off</span>
                <span className="font-label-md flex-1 min-w-0">{t('inbox.errorState.title')}</span>
                <Button
                  variant="ghost"
                  onClick={() => void refresh()}
                  className="px-sm py-xs rounded-lg text-label-md text-error hover:bg-error/10 cursor-pointer"
                >
                  {t('inbox.errorState.retry')}
                </Button>
              </div>
            )}
            {/* Select-all row */}
            <div className="flex items-center gap-sm mb-sm">
              <label className="flex items-center gap-xs cursor-pointer">
                <input
                  type="checkbox"
                  checked={allSelected}
                  onChange={toggleSelectAll}
                  aria-label={t(allSelected ? 'inbox.select.deselectAll' : 'inbox.select.selectAll')}
                  className="w-4 h-4 accent-primary cursor-pointer"
                />
                <span className="font-label-sm text-on-surface-variant">
                  {t(allSelected ? 'inbox.select.deselectAll' : 'inbox.select.selectAll')}
                </span>
              </label>
              <span className="font-label-sm text-on-surface-variant ml-auto">
                {intl.formatMessage({ id: 'inbox.select.shown' }, { visible: visibleItems.length, total: items.length })}
              </span>
            </div>
            <div
              ref={listRef}
              role="list"
              tabIndex={0}
              aria-label={t('inbox.list.aria')}
              onKeyDown={handleListKey}
              className="space-y-md outline-none"
            >
              {groupBySource
                ? SOURCE_OPTIONS.filter(s => s !== 'all').map(src => {
                    const bucket = visibleItems.filter(i => i.source === src)
                    if (bucket.length === 0) return null
                    const meta = sourceMeta(src)
                    return (
                      <div key={src} className="space-y-xs">
                        <div className="flex items-center gap-xs px-1 pt-2 pb-1 text-[11px] font-bold uppercase tracking-wider text-on-surface-variant/80">
                          <span className="material-symbols-outlined text-[14px]" aria-hidden="true">{meta.icon}</span>
                          <span className={meta.color}>{t(meta.labelKey)}</span>
                          <span className="text-on-surface-variant/60">· {bucket.length}</span>
                        </div>
                        {/* B4 P1-28: focus compares against the GLOBAL flat
                            index (flatIndexById), never the bucket-local j —
                            exactly one card rings, and Enter/a hit that one. */}
                        {bucket.map(item => (
                          <InboxCard
                            key={item.id}
                            item={item}
                            selected={effectiveSelected.has(item.id)}
                            focused={focusedIndex === flatIndexById.get(item.id)}
                            highlighted={highlightId === item.id}
                            onToggleSelected={toggleSelected}
                            onMarkRead={markRead}
                            onArchive={archive}
                            onContinue={item => void handleContinue(item)}
                            onRerun={item => void handleRerun(item)}
                            onOpenSource={handleOpenSource}
                            onReview={handleReview}
                            onViewReport={handleViewReport}
                          />
                        ))}
                      </div>
                    )
                  })
                : visibleItems.map(item => (
                  <InboxCard
                    key={item.id}
                    item={item}
                    selected={effectiveSelected.has(item.id)}
                    focused={focusedIndex === flatIndexById.get(item.id)}
                    highlighted={highlightId === item.id}
                    onToggleSelected={toggleSelected}
                    onMarkRead={markRead}
                    onArchive={archive}
                    onContinue={item => void handleContinue(item)}
                    onRerun={item => void handleRerun(item)}
                    onOpenSource={handleOpenSource}
                    onReview={handleReview}
                    onViewReport={handleViewReport}
                  />
                ))}
            </div>
          </>
        )}
      </div>
    </div>
  )
}

type SourceMeta = { icon: string; color: string; labelKey: string }

/** Source filter — collapsed dropdown (Q4: avoids horizontal overflow on
 *  narrow windows). The trigger shows the active source icon + label; the
 *  menu lists every source with its own icon so colour and label disambiguate
 *  the picker. */
function SourceFilterDropdown({
  value,
  onChange,
  sourceMeta,
  allLabel,
  label,
}: {
  value: InboxSource | undefined
  onChange: (next: InboxSource | undefined) => void
  sourceMeta: (s: InboxSource) => SourceMeta
  allLabel: string
  label: string
}) {
  const intl = useIntl()
  const t = (id: string) => intl.formatMessage({ id })
  const [open, setOpen] = useState(false)
  const meta = value ? sourceMeta(value) : null
  const triggerLabel = value ? t(meta!.labelKey) : allLabel
  const triggerIcon = value ? meta!.icon : 'layers'
  const items: DropdownMenuItem[] = SOURCE_OPTIONS.map(opt => {
    if (opt === 'all') {
      return {
        id: 'all',
        label: allLabel,
        icon: 'layers',
        onSelect: () => { onChange(undefined); setOpen(false) },
      }
    }
    const m = sourceMeta(opt)
    return {
      id: opt,
      label: t(m.labelKey),
      icon: m.icon,
      onSelect: () => { onChange(opt); setOpen(false) },
    }
  })
  return (
    <div className="relative">
      <Button
        variant="ghost"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={label}
        title={label}
        onClick={() => setOpen(v => !v)}
        className={cn(
          'inline-flex items-center gap-xs px-sm py-xs rounded-full text-label-sm transition-colors cursor-pointer',
          value ? 'bg-primary/10 text-primary font-bold' : 'bg-surface-container-low text-on-surface-variant hover:text-primary hover:bg-primary/10'
        )}
      >
        <span className="material-symbols-outlined text-[14px] align-middle" aria-hidden="true">{triggerIcon}</span>
        <span className="align-middle">{triggerLabel}</span>
        <span className="material-symbols-outlined text-[14px] align-middle" aria-hidden="true">expand_more</span>
      </Button>
      {open && (
        <DropdownMenu
          open
          onClose={() => setOpen(false)}
          items={items}
          align="start"
          className="w-48 min-w-0"
          ariaLabel={label}
        />
      )}
    </div>
  )
}

// Tests for the P0-3 inbox-backed Triage page: filters, per-item actions
// (mark read / archive / continue-in-session / rerun), error expander,
// bulk operations, and keyboard navigation.
//
// The data layer (`@/hooks/inbox`) is mocked like the old triage tests
// mocked `useTriageItems`; the bulk bar's direct `api.updateInboxItemStatus`
// calls are asserted through the `@/lib/tauri-api` mock.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { IntlProvider } from 'react-intl'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { listen } from '@tauri-apps/api/event'
import { toast } from 'sonner'
import Triage, { weekSavedOf } from '@/pages/Triage'
import * as api from '@/lib/tauri-api'
import type { InboxItem, InboxListFilter, ScheduledRoutine } from '@/types'
import { EVENT_NAMES } from '@/types'

// Test locale messages (minimal set for test assertions)
const testMessages: Record<string, string> = {
  'inbox.title': 'Inbox',
  'inbox.subtitle': 'Automation results',
  'inbox.stats.pending': '{count} pending',
  'inbox.stats.today': '{count} today',
  'inbox.filter.all': 'All',
  'inbox.status.label': 'Status',
  'inbox.status.pending': 'Pending',
  'inbox.status.read': 'Read',
  'inbox.status.archived': 'Archived',
  'inbox.source.label': 'Source',
  'inbox.source.routine': 'Routine',
  'inbox.source.scheduled_task': 'Scheduled task',
  'inbox.source.goal': 'Goal',
  'inbox.source.trigger': 'Trigger',
  'inbox.source.batch': 'Batch',
  'inbox.source.session_approval': 'Approval',
  'inbox.source.session_failed': 'Session failed',
  'inbox.source.skill_candidate': 'Skill candidate',
  'inbox.action.viewSession': 'View session',
  'inbox.action.viewSession.aria': 'Open the session this item came from',
  'inbox.action.handle': 'Handle',
  'inbox.action.handle.aria': 'Open the session to handle the waiting approval request',
  'inbox.toast.handleGuidance': 'Session opened — approve or deny the permission request there.',
  'inbox.approval.approve': 'Approve',
  'inbox.approval.reject': 'Deny',
  'inbox.approval.approved': 'Approved — the session will continue.',
  'inbox.approval.rejected': 'Denied — the session was told no.',
  'inbox.approval.expired': 'This approval request is no longer active — handle it in the session.',
  'inbox.approval.risk.aria': 'Risk level: {level}',
  'inbox.approval.risk.critical': 'Critical',
  'inbox.approval.risk.high': 'High',
  'inbox.approval.risk.medium': 'Medium',
  'inbox.approval.risk.low': 'Low',
  'inbox.continue.prefill': 'Continue working on: {summary}',
  'inbox.review.label': 'Review',
  'inbox.review.aria': 'Review this skill candidate in Extensions → Pending',
  'inbox.sort.aria': 'Toggle sort order',
  'inbox.sort.newest': 'Newest first',
  'inbox.sort.oldest': 'Oldest first',
  'inbox.pending.title': 'Pending',
  'inbox.error.label': 'Error',
  'inbox.markRead.aria': 'Mark item {id} as read',
  'inbox.markRead.title': 'Mark read',
  'inbox.archive.aria': 'Archive item {id}',
  'inbox.archive.title': 'Archive',
  'inbox.continue.aria': 'Continue this item session',
  'inbox.continue.title': 'Continue in session',
  'inbox.rerun.aria': 'Rerun the routine behind this item',
  'inbox.rerun.title': 'Rerun',
  'inbox.rerun.disabled.title': 'Rerun is available for routine and scheduled-task items',
  'inbox.openSource.label': 'View source automation',
  'inbox.openSource.aria': 'Open the automation that produced this item',
  'inbox.groupBySource': 'Group by source',
  'inbox.select.aria': 'Select item {id}',
  'inbox.select.selectAll': 'Select all visible items',
  'inbox.select.deselectAll': 'Deselect all',
  'inbox.select.shown': '{visible} of {total}',
  'inbox.bulk.title': 'Bulk actions',
  'inbox.bulk.selected': '{count} selected',
  'inbox.bulk.markRead': 'Mark read',
  'inbox.bulk.archive': 'Archive',
  'inbox.bulk.clear': 'Clear',
  'inbox.bulk.toast.markRead': 'Marked {count} item as read',
  'inbox.bulk.toast.markRead.plural': 'Marked {count} items as read',
  'inbox.bulk.toast.archived': 'Archived {count} item',
  'inbox.bulk.toast.archived.plural': 'Archived {count} items',
  'inbox.bulk.toast.failedAll': 'Nothing was updated — all {count} operations failed',
  'inbox.bulk.toast.partial': 'Updated {written} of {total} items — {failed} failed',
  'inbox.undo': 'Undo',
  'inbox.undo.failed': 'Undo failed — could not restore {count} items',
  'inbox.errorState.title': "Couldn't load the inbox",
  'inbox.errorState.description': 'The automation inbox could not be reached.',
  'inbox.errorState.retry': 'Retry',
  'inbox.list.aria': 'Inbox items. Use j or k to move focus, Enter to mark read, and a to archive.',
  'inbox.empty.title': 'All clear.',
  'inbox.empty.description': 'The automation inbox collects results from routines and triggers.',
  'inbox.empty.cta': 'Refresh',
  'inbox.empty.project.title': 'Nothing here for this project',
  'inbox.empty.project.description': 'Every inbox item in this view belongs to another project (or has no session). Clear the filter to see the full inbox.',
  'inbox.empty.project.cta': 'Clear project filter',
  'project.filter.chip.aria': 'Filtered by project: {name}',
  'project.filter.remove.aria': 'Remove project filter',
  'tasks.status.failed.label': 'Failed',
  'inbox.needsAction': 'Needs action',
  'inbox.runStatus.succeeded': 'Succeeded',
  // M7 (design-parity R2): merged pause badge + 查看日志 direct entry.
  'tasks.routineControls.autoPausedLabel': 'Auto-paused',
  'tasks.routineControls.autoPausedReasonSeparator': ' · ',
  'tasks.routineControls.autoPausedReason.budget': 'Monthly budget reached',
  'tasks.routineControls.autoPausedReason.consecutive_failures': 'Consecutive failures',
  'inbox.viewLogs.label': 'View logs',
  'inbox.viewLogs.aria': 'Open the error log of the routine that produced this item',
  // G6 (design-parity R2): 本周节省 stat + mandatory estimation footnote
  // (补2: the footnote's N is parameterized — it prints the CONFIGURED value).
  'inbox.stats.saved': '≈{minutes} min saved · {count} items',
  'inbox.stats.saved.footnote': 'Estimated at ~{minutes} min of manual work per item',
  'inbox.stats.saved.edit.aria': 'Edit minutes of manual work per item',
  'inbox.stats.saved.edit.title': 'Click to edit the estimate',
  'inbox.stats.saved.edit.invalid': 'Enter a whole number of minutes between {min} and {max}',
  'inbox.stats.saved.edit.saved': 'Estimate updated to {minutes} min per item',
  'inbox.stats.saved.edit.failed': "Couldn't save the estimate — reverted",
}

// Hook spies — useInboxItems returns
// { items, loading, error, filter, setFilter, refresh, markRead, archive, rerun, getContinueTarget }
// useInboxStats returns { stats, loading, error, refresh }
const itemsSpy = vi.hoisted(() => vi.fn())
const statsSpy = vi.hoisted(() => vi.fn())
const switchSessionSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/inbox', () => ({
  useInboxItems: () => itemsSpy(),
  useInboxStats: () => statsSpy(),
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ switchSession: switchSessionSpy }),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await import('@/lib/tauri-api')
  return {
    ...actual,
    updateInboxItemStatus: vi.fn().mockResolvedValue(undefined),
    respondPermission: vi.fn().mockResolvedValue(undefined),
    // M7: the merged pause badge joins the producing routine's pause_reason
    // through list_scheduled_tasks (empty list = nothing auto-paused).
    listScheduledTasks: vi.fn().mockResolvedValue([] as ScheduledRoutine[]),
    // 补2: the 本周节省 footnote's per-item minutes come from get_config;
    // default fixture = unset (the UI falls back to the standing 5).
    getConfig: vi.fn().mockResolvedValue({ manual_minutes_per_item: null }),
    // 补2: the footnote's inline edit persists via configure.
    configure: vi.fn().mockResolvedValue(undefined),
  }
})

function makeItem(o: Partial<InboxItem> & { id: number }): InboxItem {
  const base: InboxItem = {
    source: 'routine',
    sourceId: 'sched-001',
    sessionId: null,
    title: o.title ?? `Item ${o.id}`,
    summary: 'run summary',
    error: null,
    status: 'pending',
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    requestId: null,
    risk: null,
    runId: null,
    ...o,
  }
  // Store invariant: a freshly appended row has updatedAtMs == createdAtMs
  // (only an in-place upsert refresh diverges them). Fixtures that set only
  // createdAtMs therefore inherit it as updatedAtMs too.
  return { ...base, updatedAtMs: o.updatedAtMs ?? o.createdAtMs ?? base.updatedAtMs }
}

const baseStats = { pending: 0, today: 0 }

function setItems(items: InboxItem[], stats: { pending: number; today: number } = baseStats) {
  const actions = {
    markRead: vi.fn(async (_id: number) => true),
    archive: vi.fn(async (_id: number) => true),
    rerun: vi.fn(async (_id: number) => 'run-1'),
    getContinueTarget: vi.fn(async (_id: number) => ({
      sessionId: 'sess-006',
      summary: 'run summary',
      error: undefined,
    })),
    setFilter: vi.fn(),
    refresh: vi.fn(),
  }
  itemsSpy.mockReturnValue({
    items, loading: false, error: null, filter: undefined,
    ...actions,
  })
  statsSpy.mockReturnValue({ stats, loading: false, error: null, refresh: vi.fn() })
  return actions
}

function LocationCapture() {
  const location = useLocation()
  return (
    <div
      data-testid="location"
      data-state={JSON.stringify(location.state ?? null)}
      data-search={location.search}
    >{location.pathname}</div>
  )
}

function renderPage(initialEntry: string | { pathname: string; state?: unknown } = '/triage') {
  return render(
    <MemoryRouter initialEntries={[initialEntry]}>
      <IntlProvider locale="en" messages={testMessages} defaultLocale="en">
        <div>
          <Triage />
          <LocationCapture />
        </div>
      </IntlProvider>
    </MemoryRouter>,
  )
}

/** Approval-closure tests: one pending session_approval card, overrides on top. */
function renderPageWithItem(overrides: Partial<InboxItem> & { id?: number } = {}) {
  setItems([
    makeItem({
      id: 40,
      source: 'session_approval',
      sessionId: 'sess-77',
      title: 'bash',
      requestId: 'perm-40',
      ...overrides,
    }),
  ])
  return renderPage()
}

beforeEach(() => {
  itemsSpy.mockReset()
  statsSpy.mockReset()
  switchSessionSpy.mockReset()
  switchSessionSpy.mockResolvedValue(undefined)
  vi.mocked(api.updateInboxItemStatus).mockClear()
  vi.mocked(api.respondPermission).mockReset()
  vi.mocked(api.respondPermission).mockResolvedValue(undefined)
  // M7: default fixture set = nothing auto-paused; merge tests override.
  vi.mocked(api.listScheduledTasks).mockReset()
  vi.mocked(api.listScheduledTasks).mockResolvedValue([])
  // 补2: default = the config dial unset (UI falls back to the standing 5);
  // the parameterization tests override with an explicit value.
  vi.mocked(api.getConfig).mockReset()
  vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: null })
  vi.mocked(api.configure).mockReset()
  vi.mocked(api.configure).mockResolvedValue(undefined)
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.error).mockClear()
  setItems([])
})

describe('Triage page (inbox)', () => {
  it('renders the guided empty state when there are no items', () => {
    setItems([])
    renderPage()
    expect(screen.getByText('All clear.')).toBeInTheDocument()
    expect(screen.getByText(/automation inbox collects results/i)).toBeInTheDocument()
  })

  // A1 polish: emptiness is judged on the VISIBLE list. A project deep-link
  // that filters every item out must land on the scoped empty state — not a
  // select-all row reading 「shown 0 of 1」 — with the 清除筛选 escape hatch.
  it('shows the scoped empty state (no select-all row) when the project filter excludes every item', () => {
    setItems([makeItem({ id: 1, sessionId: 'sess-elsewhere' })])
    renderPage('/triage?project=%2Fw%2Falpha')
    expect(screen.getByText('Nothing here for this project')).toBeInTheDocument()
    expect(screen.queryByLabelText('Select all visible items')).not.toBeInTheDocument()
    expect(screen.queryByText('0 of 1')).not.toBeInTheDocument()

    // 清除筛选 strips ?project= — the unscoped inbox and its select-all row return.
    fireEvent.click(screen.getByRole('button', { name: 'Clear project filter' }))
    expect(screen.getByText('Item 1')).toBeInTheDocument()
    expect(screen.getByLabelText('Select all visible items')).toBeInTheDocument()
    expect(screen.queryByText('Nothing here for this project')).not.toBeInTheDocument()
  })

  it('keeps the plain empty state when no project filter is active and the inbox is empty', () => {
    setItems([])
    renderPage('/triage?project=%2Fw%2Falpha')
    // Zero items everywhere WITH the param → still the scoped variant.
    expect(screen.getByText('Nothing here for this project')).toBeInTheDocument()
  })

  it('renders one card per inbox item with title and summary', () => {
    setItems([
      makeItem({ id: 1, title: 'Digest finished', summary: '3 anomalies flagged' }),
      makeItem({ id: 2, title: 'Audit failed', summary: '2 vulnerabilities' }),
    ])
    renderPage()
    expect(screen.getByText('Digest finished')).toBeInTheDocument()
    expect(screen.getByText('3 anomalies flagged')).toBeInTheDocument()
    expect(screen.getByText('Audit failed')).toBeInTheDocument()
  })

  it('shows pending and today counts from inbox stats', () => {
    setItems([], { pending: 4, today: 2 })
    renderPage()
    expect(screen.getByText('4 pending')).toBeInTheDocument()
    expect(screen.getByText('2 today')).toBeInTheDocument()
  })

  it('status filter chips push the status onto the hook filter', () => {
    const { setFilter } = setItems([makeItem({ id: 1 })])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Pending' }))
    expect(setFilter).toHaveBeenCalledWith({ status: 'pending', source: undefined })
  })

  it('source filter chips push the source onto the hook filter', () => {
    const { setFilter } = setItems([makeItem({ id: 1 })])
    renderPage()
    // Q4 2026-09: source filter is a dropdown — open it first, then click the option.
    fireEvent.click(screen.getByRole('button', { name: 'Source' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Goal' }))
    expect(setFilter).toHaveBeenCalledWith({ status: undefined, source: 'goal' })
  })

  // T3 contract: batch runs append aggregate inbox items (source=`batch`,
  // batch_commands.rs). They must render with their own label/icon — not the
  // trigger fallback — and be filterable by the batch source chip.
  it('batch items render the Batch label and icon, not the trigger fallback', () => {
    setItems([makeItem({ id: 7, source: 'batch', title: 'Batch #b1 finished' })])
    renderPage()
    const list = screen.getByRole('list')
    expect(within(list).getByText('Batch')).toBeInTheDocument()
    expect(within(list).queryByText('Trigger')).not.toBeInTheDocument()
    // call_split icon (parallel-plan semantics), same as the batch runner panel.
    expect(within(list).getByText('call_split')).toBeInTheDocument()
    // Batch items have no rerunnable routine behind them (backend rejects).
    expect(screen.getByRole('button', { name: 'Rerun the routine behind this item' })).toBeDisabled()
  })

  it('batch source chip pushes source=batch onto the hook filter', () => {
    const { setFilter } = setItems([makeItem({ id: 7, source: 'batch' })])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Source' }))
    // Menu items render label inside a <span>; query by partial match to
    // tolerate i18n locale changes ("Best-of-N" / "多方案").
    const items = screen.getAllByRole('menuitem')
    const target = items.find(el => /best-of|多方案|batch/i.test(el.textContent || ''))
    expect(target).toBeTruthy()
    fireEvent.click(target!)
    expect(setFilter).toHaveBeenCalledWith({ status: undefined, source: 'batch' })
  })

  it('mark read button on a pending item calls hook markRead with the id', async () => {
    const { markRead } = setItems([makeItem({ id: 3, status: 'pending' })])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Mark item 3 as read' }))
    await waitFor(() => expect(markRead).toHaveBeenCalledWith(3))
  })

  it('read items hide mark-read and archived items hide archive', () => {
    setItems([
      makeItem({ id: 1, status: 'read' }),
      makeItem({ id: 2, status: 'archived' }),
    ])
    renderPage()
    expect(screen.queryByRole('button', { name: 'Mark item 1 as read' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Archive item 2' })).not.toBeInTheDocument()
    // The archived card still shows its "Archived" chip (scoped to the list
    // so the status-filter chip of the same name doesn't collide).
    expect(within(screen.getByRole('list')).getByText('Archived')).toBeInTheDocument()
  })

  it('rerun is enabled for a non-archived routine item and calls hook rerun', async () => {
    const { rerun } = setItems([makeItem({ id: 5, source: 'routine', status: 'read' })])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Rerun the routine behind this item' })
    expect(btn).not.toBeDisabled()
    fireEvent.click(btn)
    await waitFor(() => expect(rerun).toHaveBeenCalledWith(5))
  })

  it('rerun is disabled for goal and trigger items', () => {
    setItems([
      makeItem({ id: 1, source: 'goal' }),
      makeItem({ id: 2, source: 'trigger' }),
    ])
    renderPage()
    const buttons = screen.getAllByRole('button', { name: 'Rerun the routine behind this item' })
    expect(buttons).toHaveLength(2)
    expect(buttons[0]).toBeDisabled()
    expect(buttons[1]).toBeDisabled()
    expect(buttons[0]).toHaveAttribute('title', 'Rerun is available for routine and scheduled-task items')
  })

  it('rerun is disabled for archived routine items', () => {
    setItems([makeItem({ id: 9, source: 'routine', status: 'archived' })])
    renderPage()
    expect(screen.getByRole('button', { name: 'Rerun the routine behind this item' })).toBeDisabled()
  })

  it('continue button appears only when the item has a session and switches to it', async () => {
    const { getContinueTarget } = setItems([
      makeItem({ id: 1, sessionId: 'sess-006' }),
      makeItem({ id: 2, sessionId: null }),
    ])
    renderPage()
    const continueButtons = screen.getAllByRole('button', { name: 'Continue this item session' })
    expect(continueButtons).toHaveLength(1)
    fireEvent.click(continueButtons[0])
    await waitFor(() => expect(getContinueTarget).toHaveBeenCalledWith(1))
    await waitFor(() => expect(switchSessionSpy).toHaveBeenCalledWith('sess-006'))
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/chat'))
  })

  it('error items expose an expandable error detail', () => {
    setItems([makeItem({ id: 1, error: 'cargo audit exited with code 2' })])
    renderPage()
    const toggle = screen.getByRole('button', { name: /Error/ })
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.queryByText('cargo audit exited with code 2')).not.toBeInTheDocument()
    fireEvent.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByText('cargo audit exited with code 2')).toBeInTheDocument()
  })

  it('selecting items shows the bulk action bar with the count', () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    renderPage()
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('checkbox')[1])
    expect(screen.getByRole('region', { name: 'Bulk actions' })).toBeInTheDocument()
    expect(screen.getByText('1 selected')).toBeInTheDocument()
  })

  it('bulk mark read calls update_inbox_item_status(id, "read") for each selected item', async () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 }), makeItem({ id: 3 })])
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Mark read/ }))
    await waitFor(() => expect(api.updateInboxItemStatus).toHaveBeenCalledTimes(3))
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(1, 'read')
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(3, 'read')
  })

  it('bulk archive calls update_inbox_item_status(id, "archived") for each selected item', async () => {
    setItems([makeItem({ id: 4 }), makeItem({ id: 5 })])
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(api.updateInboxItemStatus).toHaveBeenCalledTimes(2))
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(4, 'archived')
  })

  it('Clear button in bulk bar empties the selection', () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    fireEvent.click(screen.getByRole('button', { name: /^Clear$/ }))
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
  })

  it('sort toggle flips between newest-first and oldest-first', () => {
    setItems([
      makeItem({ id: 1, title: 'Older', createdAtMs: 1_000 }),
      makeItem({ id: 2, title: 'Newer', createdAtMs: 5_000 }),
    ])
    const { container } = renderPage()
    const cards = container.querySelectorAll('[role="listitem"]')
    expect(cards[0]).toHaveTextContent('Newer')
    expect(cards[1]).toHaveTextContent('Older')
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    const cardsAfter = container.querySelectorAll('[role="listitem"]')
    expect(cardsAfter[0]).toHaveTextContent('Older')
    expect(cardsAfter[1]).toHaveTextContent('Newer')
  })

  it('j moves focus and Enter marks the focused pending item read', () => {
    const { markRead } = setItems([
      makeItem({ id: 1, status: 'pending' }),
      makeItem({ id: 2, status: 'pending' }),
    ])
    renderPage()
    const list = screen.getByRole('list', { name: /Inbox items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    fireEvent.keyDown(list, { key: 'Enter' })
    expect(markRead).toHaveBeenCalledWith(1)
  })

  it('a archives the focused item', () => {
    const { archive } = setItems([
      makeItem({ id: 1, status: 'pending' }),
      makeItem({ id: 2, status: 'pending' }),
    ])
    renderPage()
    const list = screen.getByRole('list', { name: /Inbox items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    fireEvent.keyDown(list, { key: 'a' })
    expect(archive).toHaveBeenCalledWith(1)
  })

  it('keyboard nav is ignored when the keystroke comes from a form control', () => {
    const { markRead } = setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    renderPage()
    const list = screen.getByRole('list', { name: /Inbox items/ })
    const checkbox = within(list).getAllByRole('checkbox')[0]
    list.focus()
    fireEvent.keyDown(checkbox, { key: 'j' })
    fireEvent.keyDown(list, { key: 'Enter' })
    expect(markRead).not.toHaveBeenCalled()
  })

  it('shows skeletons while loading', () => {
    itemsSpy.mockReturnValue({
      items: [], loading: true, error: null, filter: undefined,
      setFilter: vi.fn(), refresh: vi.fn(), markRead: vi.fn(), archive: vi.fn(),
      rerun: vi.fn(), getContinueTarget: vi.fn(),
    })
    statsSpy.mockReturnValue({ stats: baseStats, loading: false, error: null, refresh: vi.fn() })
    const { container } = renderPage()
    expect(container.querySelector('[data-testid="card-skeleton"], .animate-pulse')).toBeTruthy()
  })

  it('empty-state CTA calls the hook refresh', () => {
    const { refresh } = setItems([])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }))
    expect(refresh).toHaveBeenCalled()
  })

  it('filter type stays assignable to the wire shape', () => {
    const f: InboxListFilter = { status: 'read', source: 'routine' }
    expect(f.status).toBe('read')
  })
})

// IA T2 (互链闭环): routine/scheduled_task cards link back to the automation
// that produced them, and a HistoryView hand-over highlights the target card.
describe('Triage — cross links (IA T2)', () => {
  it('shows「View source automation」on a routine item and navigates to /tasks with openRoutineId', () => {
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001' })])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Open the automation that produced this item' })
    expect(btn).toHaveTextContent('View source automation')
    fireEvent.click(btn)
    const probe = screen.getByTestId('location')
    expect(probe).toHaveTextContent('/tasks')
    expect(JSON.parse(probe.getAttribute('data-state')!)).toEqual({ openRoutineId: 'sched-001' })
  })

  it('shows the source link for scheduled_task items too', () => {
    setItems([makeItem({ id: 2, source: 'scheduled_task', sourceId: 'st-9' })])
    renderPage()
    expect(screen.getByRole('button', { name: 'Open the automation that produced this item' })).toBeInTheDocument()
  })

  it('hides the source link for goal/trigger items and items without sourceId', () => {
    setItems([
      makeItem({ id: 1, source: 'goal', sourceId: null }),
      makeItem({ id: 2, source: 'trigger', sourceId: 'tr-1' }),
    ])
    renderPage()
    expect(screen.queryByRole('button', { name: 'Open the automation that produced this item' })).not.toBeInTheDocument()
  })

  it('rings the card handed over via highlightInboxId state and drains the router state', () => {
    setItems([
      makeItem({ id: 7, source: 'routine', sourceId: 'sched-001' }),
      makeItem({ id: 8, source: 'routine', sourceId: 'sched-002' }),
    ])
    renderPage({ pathname: '/triage', state: { highlightInboxId: 7 } })
    const highlighted = screen.getByText('Item 7').closest('[role="listitem"]')
    expect(highlighted).not.toBeNull()
    expect(highlighted!).toHaveAttribute('data-highlight', 'true')
    expect(highlighted!.className).toContain('ring-2')
    // The other card is not highlighted.
    expect(screen.getByText('Item 8').closest('[data-highlight]')).toBeNull()
    // One-shot: the router state was consumed (highlight lives in local state).
    expect(JSON.parse(screen.getByTestId('location').getAttribute('data-state')!)).toBeNull()
  })

  it('does not ring any card without highlight state', () => {
    setItems([makeItem({ id: 1 })])
    renderPage()
    expect(document.querySelector('[data-highlight="true"]')).toBeNull()
  })
})

// IA T6 (收件箱升级): session sources target the session, skill candidates
// link to the Extensions → Pending review queue (互通), pending items pin to
// the top of the default view, and the three new sources are filterable.
// 2026-10 design parity (审查 R1 §05): approval cards read「Handle」(去处理)
// + the in-session guidance toast — the payload carries no permission
// request id, so inline approve/deny would be a fake button; failure cards
// keep the read-oriented「View session」.
describe('Triage — session sources, skill candidates and pending pinning (IA T6/X1)', () => {
  it('shows「Handle」as the primary action for a session_approval item, toasts the guidance and switches to it', async () => {
    const { toast } = await import('sonner')
    const { getContinueTarget } = setItems([
      makeItem({ id: 11, source: 'session_approval', sessionId: 'sess-77', title: 'Permission requested' }),
    ])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Open the session to handle the waiting approval request' })
    expect(btn).toHaveTextContent('Handle')
    // The automation-facing「Resume session」wording stays off approval cards.
    expect(screen.queryByRole('button', { name: 'Continue this item session' })).not.toBeInTheDocument()
    fireEvent.click(btn)
    await waitFor(() => expect(getContinueTarget).toHaveBeenCalledWith(11))
    await waitFor(() => expect(switchSessionSpy).toHaveBeenCalledWith('sess-006'))
    await waitFor(() => expect(toast.info).toHaveBeenCalledWith('Session opened — approve or deny the permission request there.'))
    await waitFor(() => expect(screen.getByTestId('location')).toHaveTextContent('/chat'))
  })

  it('shows「View session」for a session_failed item too', () => {
    setItems([makeItem({ id: 12, source: 'session_failed', sessionId: 'sess-88', title: 'Turn failed' })])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Open the session this item came from' })
    expect(btn).toHaveTextContent('View session')
  })

  it('keeps「Resume session」as the primary action for automation sources', () => {
    setItems([makeItem({ id: 13, source: 'routine', sessionId: 'sess-99' })])
    renderPage()
    expect(screen.getByRole('button', { name: 'Continue this item session' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open the session this item came from' })).not.toBeInTheDocument()
  })

  it('shows「Review」on a skill_candidate card and jumps to /extensions/pending with the candidate id', () => {
    setItems([makeItem({ id: 21, source: 'skill_candidate', sourceId: 'cand-42', title: 'Recurring deploy pattern' })])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Review this skill candidate in Extensions → Pending' })
    expect(btn).toHaveTextContent('Review')
    fireEvent.click(btn)
    const probe = screen.getByTestId('location')
    expect(probe).toHaveTextContent('/extensions/pending')
    expect(JSON.parse(probe.getAttribute('data-state')!)).toEqual({ skillCandidateId: 'cand-42' })
  })

  it('hides「Review」on items that are not skill candidates', () => {
    setItems([makeItem({ id: 22, source: 'routine', sourceId: 'sched-001' })])
    renderPage()
    expect(screen.queryByRole('button', { name: /Review this skill candidate/ })).not.toBeInTheDocument()
  })

  it('pins pending items above read items in the default (newest) sort', () => {
    setItems([
      makeItem({ id: 1, status: 'read', title: 'Newer read', createdAtMs: 9_000 }),
      makeItem({ id: 2, status: 'pending', title: 'Older pending', createdAtMs: 1_000 }),
    ])
    const { container } = renderPage()
    const cards = container.querySelectorAll('[role="listitem"]')
    expect(cards[0]).toHaveTextContent('Older pending')
    expect(cards[1]).toHaveTextContent('Newer read')
    // Pinning survives the sort toggle; time order stays stable inside a band.
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    const cardsAfter = container.querySelectorAll('[role="listitem"]')
    expect(cardsAfter[0]).toHaveTextContent('Older pending')
    expect(cardsAfter[1]).toHaveTextContent('Newer read')
  })

  // 卡 3a: upsert_pending refreshes an existing row's updatedAtMs in place.
  // A same-session failure that re-fails must float to the top of its band
  // even though its createdAtMs is older than other entries.
  it('floats an upsert-refreshed item to the top of its status band', () => {
    setItems([
      // Session A failed once at t=9s and was never refreshed.
      makeItem({
        id: 1,
        source: 'session_failed',
        sourceId: 'sess-a',
        title: 'Stale failure',
        createdAtMs: 9_000,
        updatedAtMs: 9_000,
      }),
      // Session B failed at t=1s, then failed AGAIN at t=12s — the second
      // event updated the same row in place (createdAtMs stays 1s).
      makeItem({
        id: 2,
        source: 'session_failed',
        sourceId: 'sess-b',
        title: 'Re-failed session',
        createdAtMs: 1_000,
        updatedAtMs: 12_000,
      }),
    ])
    const { container } = renderPage()
    const cards = container.querySelectorAll('[role="listitem"]')
    expect(cards[0]).toHaveTextContent('Re-failed session')
    expect(cards[1]).toHaveTextContent('Stale failure')
    // The toggle still only reorders inside the band — the refreshed entry
    // lands last under 'oldest', never losing its band membership.
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    const cardsAfter = container.querySelectorAll('[role="listitem"]')
    expect(cardsAfter[0]).toHaveTextContent('Stale failure')
    expect(cardsAfter[1]).toHaveTextContent('Re-failed session')
  })

  it('filters the three new sources through the source dropdown', () => {
    const { setFilter } = setItems([makeItem({ id: 1 })])
    renderPage()
    const cases: Array<[RegExp, string]> = [
      [/approval/i, 'session_approval'],
      [/session failed/i, 'session_failed'],
      [/skill candidate/i, 'skill_candidate'],
    ]
    for (const [label, source] of cases) {
      fireEvent.click(screen.getByRole('button', { name: 'Source' }))
      const items = screen.getAllByRole('menuitem')
      const target = items.find(el => label.test(el.textContent || ''))
      expect(target, `menu item for ${source}`).toBeTruthy()
      fireEvent.click(target!)
      expect(setFilter).toHaveBeenCalledWith({ status: undefined, source })
    }
  })
})

// ─── B4 hardening (P1-28 / P1-29 / P1-30, §7-26 + §7-28) ───────────────────

describe('Triage — B4 keyboard/focus hardening', () => {
  it('grouped mode rings exactly one card and a archives THAT card (P1-28)', () => {
    // Flat order under grouping: routine bucket [1, 2] then trigger bucket [3].
    // The old bucket-local index (j) rang cards 1 AND 2 (j=1 in each bucket)
    // while the cursor acted on a third item entirely.
    const { archive } = setItems([
      makeItem({ id: 1, source: 'routine' }),
      makeItem({ id: 2, source: 'routine' }),
      makeItem({ id: 3, source: 'trigger' }),
    ])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Group by source' }))
    const list = screen.getByRole('list', { name: /Inbox items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    fireEvent.keyDown(list, { key: 'j' })
    // Exactly one ring — the second routine card (flat index 1).
    const ringed = document.querySelectorAll('[data-focused="true"]')
    expect(ringed).toHaveLength(1)
    expect(ringed[0]).toHaveTextContent('Item 2')
    fireEvent.keyDown(list, { key: 'a' })
    expect(archive).toHaveBeenCalledWith(2)
  })

  it('Enter on a focused card button is not hijacked into mark-read (P1-29)', () => {
    const { markRead } = setItems([makeItem({ id: 1 })])
    renderPage()
    const list = screen.getByRole('list', { name: /Inbox items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    const archiveBtn = within(list).getByRole('button', { name: 'Archive item 1' })
    fireEvent.keyDown(archiveBtn, { key: 'Enter' })
    expect(markRead).not.toHaveBeenCalled()
  })

  it('j/k still navigate while focus rests on a card button', () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    renderPage()
    const list = screen.getByRole('list', { name: /Inbox items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    const btn = within(list).getAllByRole('button', { name: 'Rerun the routine behind this item' })[0]
    btn.focus()
    fireEvent.keyDown(btn, { key: 'j' })
    const ringed = document.querySelectorAll('[data-focused="true"]')
    expect(ringed).toHaveLength(1)
    expect(ringed[0]).toHaveTextContent('Item 2')
  })
})

describe('Triage — B4 bulk failures and undo (P1-30)', () => {
  it('keeps the selection and toasts when every bulk write fails', async () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    vi.mocked(api.updateInboxItemStatus).mockRejectedValue(new Error('db locked'))
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Archive' }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith('Nothing was updated — all 2 operations failed'),
    )
    // The selection survives so a retry is one click away.
    expect(screen.getByText('2 selected')).toBeInTheDocument()
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('reports partial bulk failure truthfully and keeps failed items selected', async () => {
    setItems([makeItem({ id: 1 }), makeItem({ id: 2 })])
    vi.mocked(api.updateInboxItemStatus).mockImplementation((id: number) =>
      id === 1 ? Promise.resolve(undefined) : Promise.reject(new Error('boom')),
    )
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Archive' }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith('Updated 1 of 2 items — 1 failed'),
    )
    expect(toast.success).not.toHaveBeenCalled()
    // Only the failed item stays selected.
    expect(screen.getByText('1 selected')).toBeInTheDocument()
  })

  it('successful bulk archive offers an Undo that restores prior statuses', async () => {
    setItems([
      makeItem({ id: 1, status: 'read' }),
      makeItem({ id: 2, status: 'pending' }),
    ])
    vi.mocked(api.updateInboxItemStatus).mockResolvedValue(undefined)
    renderPage()
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: 'Archive' }))
    await waitFor(() => expect(api.updateInboxItemStatus).toHaveBeenCalledWith(1, 'archived'))
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    const archiveToast = vi.mocked(toast.success).mock.calls.find(([msg]) => String(msg).startsWith('Archived'))
    expect(archiveToast).toBeTruthy()
    const action = (archiveToast![1] as { action: { label: string; onClick: () => void } }).action
    expect(action.label).toBe('Undo')
    vi.mocked(api.updateInboxItemStatus).mockClear()
    action.onClick()
    // Undo restores each item's PRE-operation status, not a blanket 'pending'.
    await waitFor(() => expect(api.updateInboxItemStatus).toHaveBeenCalledWith(1, 'read'))
    expect(api.updateInboxItemStatus).toHaveBeenCalledWith(2, 'pending')
  })
})

describe('Triage — B4 URL view state and error state (§7-28)', () => {
  it('initializes status/sort/group from search params and writes changes back', () => {
    const { setFilter } = setItems([
      makeItem({ id: 1, status: 'read', title: 'Read later', createdAtMs: 5_000 }),
      makeItem({ id: 2, status: 'read', title: 'Read earlier', createdAtMs: 1_000 }),
    ])
    renderPage('/triage?status=read&sort=oldest')
    expect(setFilter).toHaveBeenCalledWith({ status: 'read', source: undefined })
    // oldest-first honored from the param.
    const cards = document.querySelectorAll('[role="listitem"]')
    expect(cards[0]).toHaveTextContent('Read earlier')
    expect(cards[1]).toHaveTextContent('Read later')
    // Interactions write the params back (replace navigation).
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    expect(screen.getByTestId('location').getAttribute('data-search')).toContain('sort=newest')
    fireEvent.click(screen.getByRole('button', { name: 'Group by source' }))
    expect(screen.getByTestId('location').getAttribute('data-search')).toContain('group=source')
    fireEvent.click(screen.getByRole('button', { name: 'Pending' }))
    expect(screen.getByTestId('location').getAttribute('data-search')).toContain('status=pending')
  })

  it('ignores unknown param values instead of silently narrowing the list', () => {
    const { setFilter } = setItems([makeItem({ id: 1 })])
    renderPage('/triage?status=bogus&source=warp&sort=sideways')
    expect(setFilter).toHaveBeenCalledWith({ status: undefined, source: undefined })
    expect(screen.getByText('Item 1')).toBeInTheDocument()
  })

  it('renders the full error state when the inbox IPC fails with no rows', () => {
    const refresh = vi.fn()
    itemsSpy.mockReturnValue({
      items: [], loading: false, error: 'db locked', filter: undefined,
      setFilter: vi.fn(), refresh, markRead: vi.fn(), archive: vi.fn(),
      rerun: vi.fn(), getContinueTarget: vi.fn(),
    })
    statsSpy.mockReturnValue({ stats: baseStats, loading: false, error: null, refresh: vi.fn() })
    renderPage()
    expect(screen.getByText("Couldn't load the inbox")).toBeInTheDocument()
    expect(screen.queryByText('All clear.')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(refresh).toHaveBeenCalled()
  })

  it('shows a stale-rows banner (not a silent list) when refresh fails with rows present', () => {
    itemsSpy.mockReturnValue({
      items: [makeItem({ id: 1 })], loading: false, error: 'db locked', filter: undefined,
      setFilter: vi.fn(), refresh: vi.fn(), markRead: vi.fn(), archive: vi.fn(),
      rerun: vi.fn(), getContinueTarget: vi.fn(),
    })
    statsSpy.mockReturnValue({ stats: baseStats, loading: false, error: null, refresh: vi.fn() })
    renderPage()
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't load the inbox")
    expect(screen.getByText('Item 1')).toBeInTheDocument()
  })
})

// ── W3-2: needs-action run status on triage cards ──────────────────────────
describe('W3-2 run outcome on triage cards', () => {
  it('leads a failed run card with Failed · Needs action and an error border', () => {
    setItems([makeItem({ id: 1, source: 'routine', error: 'provider unreachable' })])
    const { container } = renderPage()
    const chip = screen.getByTestId('inbox-run-status')
    expect(chip).toHaveTextContent('Failed')
    expect(chip).toHaveTextContent('Needs action')
    // needs-action semantics beyond colour: priority icon + words on a
    // tinted chip, plus the card's error-tinted border.
    expect(chip.querySelector('.material-symbols-outlined')).toHaveTextContent('priority_high')
    expect(container.querySelector('[role="listitem"]')?.className).toContain('border-error/40')
  })

  it('leads a succeeded run card with the succeeded status word', () => {
    setItems([makeItem({ id: 2, source: 'routine', error: null })])
    renderPage()
    const chip = screen.getByTestId('inbox-run-status')
    expect(chip).toHaveTextContent('Succeeded')
    expect(chip).not.toHaveTextContent('Needs action')
  })

  it('derives the outcome for trigger results too', () => {
    setItems([
      makeItem({ id: 3, source: 'trigger', error: 'boom' }),
      makeItem({ id: 4, source: 'trigger', error: null }),
    ])
    renderPage()
    expect(screen.getAllByTestId('inbox-run-status')).toHaveLength(2)
  })

  it('carries no run-status chip on non-run sources', () => {
    setItems([
      makeItem({ id: 5, source: 'session_failed', error: 'turn failed' }),
      makeItem({ id: 6, source: 'skill_candidate', error: null }),
      makeItem({ id: 7, source: 'dream_report', error: null }),
    ])
    renderPage()
    expect(screen.queryByTestId('inbox-run-status')).not.toBeInTheDocument()
  })
})

// ── Design 05 收件箱审批闭环: pending approval rows carry the live
// requestId, so the card answers it inline through respondPermission; risk
// rides along for the Header-consistent four-tier badge. ───────────────────
describe('Triage — inline approval closure (requestId cards)', () => {
  beforeEach(() => {
    vi.mocked(api.respondPermission).mockReset()
    vi.mocked(api.respondPermission).mockResolvedValue(undefined)
  })

  it('renders inline Approve/Deny (not 「Handle」) on a pending card with a requestId', () => {
    setItems([
      makeItem({
        id: 30,
        source: 'session_approval',
        sessionId: 'sess-77',
        title: 'bash',
        requestId: 'perm-30',
        risk: 'high',
      }),
    ])
    renderPage()
    expect(screen.getByRole('button', { name: 'Approve' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Deny' })).toBeInTheDocument()
    // The blind jump is gone while the live decision is available…
    expect(screen.queryByRole('button', { name: 'Open the session to handle the waiting approval request' })).not.toBeInTheDocument()
    // …and so is the automation-facing resume wording.
    expect(screen.queryByRole('button', { name: 'Continue this item session' })).not.toBeInTheDocument()
  })

  it('approve calls respondPermission(requestId, true), toasts success and refreshes the list', async () => {
    const { refresh } = setItems([
      makeItem({
        id: 31,
        source: 'session_approval',
        sessionId: 'sess-77',
        title: 'bash',
        requestId: 'perm-31',
        risk: 'low',
      }),
    ])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    await waitFor(() => expect(api.respondPermission).toHaveBeenCalledWith('perm-31', true))
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Approved — the session will continue.'))
    expect(toast.error).not.toHaveBeenCalled()
    await waitFor(() => expect(refresh).toHaveBeenCalled())
  })

  it('deny calls respondPermission(requestId, false) with the rejected toast', async () => {
    setItems([
      makeItem({
        id: 32,
        source: 'session_approval',
        sessionId: 'sess-77',
        title: 'bash',
        requestId: 'perm-32',
        risk: 'medium',
      }),
    ])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Deny' }))
    await waitFor(() => expect(api.respondPermission).toHaveBeenCalledWith('perm-32', false))
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Denied — the session was told no.'))
  })

  it('falls back to 「Handle」 with the honest expired toast when the request is gone', async () => {
    vi.mocked(api.respondPermission).mockRejectedValue(new Error('Permission request not found: perm-33'))
    setItems([
      makeItem({
        id: 33,
        source: 'session_approval',
        sessionId: 'sess-77',
        title: 'bash',
        requestId: 'perm-33',
        risk: 'high',
      }),
    ])
    renderPage()
    fireEvent.click(screen.getByRole('button', { name: 'Approve' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith('This approval request is no longer active — handle it in the session.'))
    // No fake success: the approval toast must stay silent.
    expect(toast.success).not.toHaveBeenCalled()
    // The card reverts to the honest 「去处理」 jump (request answered in the
    // session / timed out — inline is no longer claimable).
    await waitFor(() =>
      expect(screen.getByRole('button', { name: 'Open the session to handle the waiting approval request' })).toBeInTheDocument(),
    )
    expect(screen.queryByRole('button', { name: 'Approve' })).not.toBeInTheDocument()
  })

  it('keeps the 「Handle」 jump on legacy rows without a requestId', () => {
    setItems([
      makeItem({ id: 34, source: 'session_approval', sessionId: 'sess-77', title: 'bash', requestId: null }),
    ])
    renderPage()
    expect(screen.getByRole('button', { name: 'Open the session to handle the waiting approval request' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Approve' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Deny' })).not.toBeInTheDocument()
    expect(api.respondPermission).not.toHaveBeenCalled()
  })

  it('hides the inline buttons once the entry is settled (read)', () => {
    setItems([
      makeItem({
        id: 35,
        source: 'session_approval',
        sessionId: 'sess-77',
        title: 'bash',
        status: 'read',
        requestId: 'perm-35',
        risk: 'low',
      }),
    ])
    renderPage()
    expect(screen.getByRole('button', { name: 'Open the session to handle the waiting approval request' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Approve' })).not.toBeInTheDocument()
  })

  // Risk badge: the Header permission dialog's four container pairs —
  // critical=error / high=secondary / medium=warning / low=tertiary.
  it('maps the risk tier onto the Header-consistent container pairs', () => {
    const cases: Array<[string, string]> = [
      ['critical', 'bg-error-container'],
      ['high', 'bg-secondary-container'],
      ['medium', 'bg-warning-container'],
      ['low', 'bg-tertiary-container'],
    ]
    for (const [tier, expected] of cases) {
      const { unmount } = renderPageWithItem({ risk: tier })
      const badge = screen.getByTestId('inbox-approval-risk')
      expect(badge.className).toContain(expected)
      expect(badge).toHaveAttribute('aria-label', `Risk level: ${tier[0].toUpperCase()}${tier.slice(1)}`)
      unmount()
    }
  })

  it('renders no risk badge when the risk column is null or off-vocabulary', () => {
    const { unmount } = renderPageWithItem({ risk: null })
    expect(screen.queryByTestId('inbox-approval-risk')).not.toBeInTheDocument()
    unmount()
    renderPageWithItem({ risk: 'unknown-tier' })
    expect(screen.queryByTestId('inbox-approval-risk')).not.toBeInTheDocument()
  })
})

describe('Triage — 来源链 #N (design 05)', () => {
  it('renders the mono run number for run-produced items', () => {
    renderPageWithItem({ id: 1, source: 'routine', runId: 43, title: 'CI 巡检' })
    expect(screen.getByTestId('inbox-run-number')).toHaveTextContent('#43')
  })

  it('hides the run number on non-run sources (honesty rule)', () => {
    renderPageWithItem({ id: 2, source: 'session_approval', runId: null })
    expect(screen.queryByTestId('inbox-run-number')).not.toBeInTheDocument()
  })
})

// ── M7 (design-parity R2 §3.2): the failure card's merged
// 「失败 · 已自动暂停」 badge and the 「查看日志」 direct entry. ─────────────
describe('M7 — merged pause badge and View logs direct entry', () => {
  it('merges 「Failed · Auto-paused · <reason>」 when the producing routine is system-paused', async () => {
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      { id: 'sched-001', enabled: false, pause_reason: 'consecutive_failures' },
    ] as unknown as ScheduledRoutine[])
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001', error: 'NAS disk full' })])
    renderPage()
    const chip = await screen.findByTestId('inbox-run-status')
    // Same wording family as TaskCard's auto-paused badge (TaskCard.tsx:75-88).
    expect(chip).toHaveTextContent('Failed')
    expect(chip).toHaveTextContent('Auto-paused')
    expect(chip).toHaveTextContent('Consecutive failures')
    expect(chip).not.toHaveTextContent('Needs action')
    // The tooltip carries the reason phrase, matching TaskCard's badge.
    expect(chip).toHaveAttribute('title', 'Consecutive failures')
  })

  it('keeps the plain 「Failed · Needs action」 badge when the routine is not paused', async () => {
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      { id: 'sched-001', enabled: true, pause_reason: null },
    ] as unknown as ScheduledRoutine[])
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001', error: 'boom' })])
    renderPage()
    const chip = await screen.findByTestId('inbox-run-status')
    expect(chip).toHaveTextContent('Needs action')
    expect(chip).not.toHaveTextContent('Auto-paused')
  })

  it('keeps the plain badge when the routine list fails to load (no pause claim without evidence)', async () => {
    vi.mocked(api.listScheduledTasks).mockRejectedValue(new Error('ipc down'))
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001', error: 'boom' })])
    renderPage()
    const chip = await screen.findByTestId('inbox-run-status')
    expect(chip).toHaveTextContent('Needs action')
    expect(chip).not.toHaveTextContent('Auto-paused')
  })

  it('reads 「View logs」 on a failed card and lands on the routine drawer deep link with focusError', () => {
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001', error: 'boom' })])
    renderPage()
    const btn = screen.getByRole('button', { name: 'Open the error log of the routine that produced this item' })
    expect(btn).toHaveTextContent('View logs')
    fireEvent.click(btn)
    const probe = screen.getByTestId('location')
    expect(probe).toHaveTextContent('/tasks')
    // 补3: the failed card hands over the error-anchor flag alongside the
    // routine id — Tasks scrolls/highlights the drawer's last-error block.
    expect(JSON.parse(probe.getAttribute('data-state')!)).toEqual({ openRoutineId: 'sched-001', focusError: true })
    // The generic source wording is REPLACED on failed cards (design 05's
    // ops row draws 继续会话 + 查看日志, not a third source button).
    expect(screen.queryByRole('button', { name: 'Open the automation that produced this item' })).not.toBeInTheDocument()
  })

  it('keeps 「View source automation」 on healthy (non-failed) cards', () => {
    setItems([makeItem({ id: 1, source: 'routine', sourceId: 'sched-001', error: null })])
    renderPage()
    expect(screen.getByRole('button', { name: 'Open the automation that produced this item' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Open the error log of the routine that produced this item' })).not.toBeInTheDocument()
  })
})

// ── G6 (design-parity R2 §4, 05-inbox.html:157-161): 本周节省 stat. ─────────
describe('G6 — weekly-saved stat (5 min/item stand-in)', () => {
  it('derives minutes from items processed this week and always shows the footnote', () => {
    const now = Date.now()
    setItems([
      makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 }),
      makeItem({ id: 2, status: 'archived', updatedAtMs: now - 86_400_000 }),
      // Pending = not processed; last-week = outside 本周.
      makeItem({ id: 3, status: 'pending', updatedAtMs: now - 1_000 }),
      makeItem({ id: 4, status: 'read', updatedAtMs: now - 8 * 86_400_000 }),
    ])
    renderPage()
    const stat = screen.getByTestId('inbox-stat-saved')
    expect(stat).toHaveTextContent('≈10 min saved')
    expect(stat).toHaveTextContent('2 items')
    // Honesty discipline: the estimation footnote is always attached.
    expect(stat).toHaveTextContent('Estimated at ~5 min of manual work per item')
  })

  it('renders an honest ≈0 min · 0 items when nothing was processed this week', () => {
    setItems([makeItem({ id: 1, status: 'pending' })])
    renderPage()
    const stat = screen.getByTestId('inbox-stat-saved')
    expect(stat).toHaveTextContent('≈0 min saved')
    expect(stat).toHaveTextContent('0 items')
  })
})

// ── 补2 (R2 审查 §7.1): the per-item manual-work minutes are a CONFIG dial
// (`manual_minutes_per_item`, 1-120, standing default 5) — the footnote
// prints the value in force and the number itself is the inline edit
// affordance (click → input → configure; failure toasts and reverts). ──────
describe('补2 — manual_minutes_per_item parameterized footnote', () => {
  it('weekSavedOf multiplies by the configured per-item minutes', () => {
    const now = Date.now()
    const items = [
      makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 }),
      makeItem({ id: 2, status: 'archived', updatedAtMs: now - 7_200_000 }),
    ]
    expect(weekSavedOf(items, new Date(), 12).minutes).toBe(24)
    // Callers without an explicit dial keep the standing default of 5.
    expect(weekSavedOf(items).minutes).toBe(10)
  })

  it('prints the CONFIGURED value in the footnote and the savings math', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 12 })
    const now = Date.now()
    setItems([
      makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 }),
      makeItem({ id: 2, status: 'read', updatedAtMs: now - 7_200_000 }),
    ])
    renderPage()
    const stat = await screen.findByTestId('inbox-stat-saved')
    await waitFor(() => expect(stat).toHaveTextContent('≈24 min saved'))
    await waitFor(() => expect(stat).toHaveTextContent('Estimated at ~12 min of manual work per item'))
  })

  it('keeps the standing 5 when the config read fails (best-effort dial)', async () => {
    vi.mocked(api.getConfig).mockRejectedValue(new Error('ipc down'))
    const now = Date.now()
    setItems([makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 })])
    renderPage()
    const stat = await screen.findByTestId('inbox-stat-saved')
    await waitFor(() => expect(stat).toHaveTextContent('≈5 min saved'))
    expect(stat).toHaveTextContent('Estimated at ~5 min of manual work per item')
  })

  it('edits inline: clicking the number commits a new value through configure', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 5 })
    const now = Date.now()
    setItems([makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 })])
    renderPage()
    // The number itself is the edit affordance (点击数字).
    fireEvent.click(await screen.findByTestId('manual-minutes-edit'))
    const input = screen.getByTestId('manual-minutes-input')
    fireEvent.change(input, { target: { value: '15' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() =>
      expect(api.configure).toHaveBeenCalledWith({ key: 'manual_minutes_per_item', value: '15' }),
    )
    await waitFor(() => expect(toast.success).toHaveBeenCalledWith('Estimate updated to 15 min per item'))
    // The display and the savings math follow the ACCEPTED value.
    const stat = screen.getByTestId('inbox-stat-saved')
    await waitFor(() => expect(stat).toHaveTextContent('≈15 min saved'))
    expect(stat).toHaveTextContent('Estimated at ~15 min of manual work per item')
    expect(screen.queryByTestId('manual-minutes-input')).not.toBeInTheDocument()
  })

  it('refuses out-of-range drafts client-side without calling configure', async () => {
    setItems([])
    renderPage()
    fireEvent.click(await screen.findByTestId('manual-minutes-edit'))
    const input = screen.getByTestId('manual-minutes-input')
    fireEvent.change(input, { target: { value: '500' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith('Enter a whole number of minutes between 1 and 120'),
    )
    expect(api.configure).not.toHaveBeenCalled()
    // Back to the display — never stuck in the editor.
    await waitFor(() => expect(screen.queryByTestId('manual-minutes-input')).not.toBeInTheDocument())
    expect(screen.getByTestId('manual-minutes-edit')).toHaveTextContent('5')
  })

  it('reverts the display and toasts when configure rejects the write', async () => {
    vi.mocked(api.configure).mockRejectedValue(new Error('disk full'))
    setItems([])
    renderPage()
    fireEvent.click(await screen.findByTestId('manual-minutes-edit'))
    const input = screen.getByTestId('manual-minutes-input')
    fireEvent.change(input, { target: { value: '20' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Couldn't save the estimate — reverted"))
    expect(toast.success).not.toHaveBeenCalled()
    expect(api.getConfig).toHaveBeenCalledTimes(1)
    // 回退: the footnote still shows the previous (still-configured) value.
    await waitFor(() => expect(screen.queryByTestId('manual-minutes-input')).not.toBeInTheDocument())
    expect(screen.getByTestId('manual-minutes-edit')).toHaveTextContent('5')
  })
})

// ── R3 报告 §4 遗留项: 跨窗回显 — the mount read is a one-shot snapshot, so
// a manual_minutes write from ANOTHER window reaches this page only via the
// backend's `config-updated` emission (apply_manual_minutes_arm). The echo
// re-reads the config through the same 1-120 gate (illegal → standing 5),
// filters on the payload key, and stays silent (this window's own saves
// already toast from the commit path). ─────────────────────────────────────
describe('manual_minutes config-updated echo (cross-window)', () => {
  type CfgEvent = { payload: unknown }
  // Controllable `listen` (setup.ts stubs it inert globally): capture the
  // handler the page registers for config-updated so a test can fire the
  // backend echo at it — same pattern as companionBridge.test.tsx.
  function captureConfigUpdatedListener() {
    const handlers: Array<(e: CfgEvent) => void> = []
    vi.mocked(listen).mockImplementation(async (event: string, handler: (e: CfgEvent) => void) => {
      if (event === EVENT_NAMES.CONFIG_UPDATED) handlers.push(handler)
      return () => {}
    })
    return {
      fire(payload: { key: string; value: string }) {
        act(() => { for (const h of handlers) h({ payload }) })
      },
    }
  }

  afterEach(() => {
    // Restore the setup.ts default — the capturing implementation above
    // must not leak into the later tests of this file.
    vi.mocked(listen).mockReset()
    vi.mocked(listen).mockResolvedValue(() => {})
  })

  it('re-reads the config and repaints the dial when the echo arrives for its key', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 12 })
    const now = Date.now()
    setItems([makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 })])
    const echo = captureConfigUpdatedListener()
    renderPage()
    const stat = await screen.findByTestId('inbox-stat-saved')
    await waitFor(() => expect(stat).toHaveTextContent('Estimated at ~12 min of manual work per item'))
    expect(api.getConfig).toHaveBeenCalledTimes(1)

    // An echo for an unrelated key triggers no re-read (payload-key filter).
    echo.fire({ key: 'effort_level', value: 'high' })
    expect(api.getConfig).toHaveBeenCalledTimes(1)

    // Another window wrote 7: the backend echo re-reads and repaints —
    // silently (the local commit path owns the toast, so no double-toast).
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 7 })
    echo.fire({ key: 'manual_minutes_per_item', value: '7' })
    await waitFor(() => expect(stat).toHaveTextContent('≈7 min saved'))
    await waitFor(() => expect(stat).toHaveTextContent('Estimated at ~7 min of manual work per item'))
    expect(api.getConfig).toHaveBeenCalledTimes(2)
    expect(toast.success).not.toHaveBeenCalled()
    expect(toast.error).not.toHaveBeenCalled()
  })

  it('falls back to the standing 5 when the echoed value fails the 1-120 gate', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 12 })
    const now = Date.now()
    setItems([makeItem({ id: 1, status: 'read', updatedAtMs: now - 3_600_000 })])
    const echo = captureConfigUpdatedListener()
    renderPage()
    const stat = await screen.findByTestId('inbox-stat-saved')
    await waitFor(() => expect(stat).toHaveTextContent('Estimated at ~12 min of manual work per item'))

    // A stray out-of-range value on the re-read never repaints fiction —
    // the dial drops back to the standing default (5).
    vi.mocked(api.getConfig).mockResolvedValue({ manual_minutes_per_item: 999 })
    echo.fire({ key: 'manual_minutes_per_item', value: '999' })
    await waitFor(() => expect(stat).toHaveTextContent('≈5 min saved'))
    await waitFor(() => expect(stat).toHaveTextContent('Estimated at ~5 min of manual work per item'))
    expect(toast.error).not.toHaveBeenCalled()
  })
})

// Tests for the P0-3 inbox-backed Triage page: filters, per-item actions
// (mark read / archive / continue-in-session / rerun), error expander,
// bulk operations, and keyboard navigation.
//
// The data layer (`@/hooks/inbox`) is mocked like the old triage tests
// mocked `useTriageItems`; the bulk bar's direct `api.updateInboxItemStatus`
// calls are asserted through the `@/lib/tauri-api` mock.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { IntlProvider } from 'react-intl'
import { MemoryRouter, useLocation } from 'react-router-dom'
import Triage from '@/pages/Triage'
import * as api from '@/lib/tauri-api'
import type { InboxItem, InboxListFilter } from '@/types'

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
  'inbox.list.aria': 'Inbox items. Use j or k to move focus, Enter to mark read, and a to archive.',
  'inbox.empty.title': 'All clear.',
  'inbox.empty.description': 'The automation inbox collects results from routines and triggers.',
  'inbox.empty.cta': 'Refresh',
}

// Hook spies — useInboxItems returns
// { items, loading, error, filter, setFilter, refresh, markRead, archive, rerun, getSessionId }
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
  toast: { success: vi.fn(), error: vi.fn() },
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await import('@/lib/tauri-api')
  return {
    ...actual,
    updateInboxItemStatus: vi.fn().mockResolvedValue(undefined),
  }
})

function makeItem(o: Partial<InboxItem> & { id: number }): InboxItem {
  return {
    source: 'routine',
    sourceId: 'sched-001',
    sessionId: null,
    title: o.title ?? `Item ${o.id}`,
    summary: 'run summary',
    error: null,
    status: 'pending',
    createdAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    ...o,
  }
}

const baseStats = { pending: 0, today: 0 }

function setItems(items: InboxItem[], stats: { pending: number; today: number } = baseStats) {
  const actions = {
    markRead: vi.fn(async (_id: number) => true),
    archive: vi.fn(async (_id: number) => true),
    rerun: vi.fn(async (_id: number) => 'run-1'),
    getSessionId: vi.fn(async (_id: number) => 'sess-006'),
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
  return <div data-testid="location">{location.pathname}</div>
}

function renderPage() {
  return render(
    <MemoryRouter initialEntries={['/triage']}>
      <IntlProvider locale="en" messages={testMessages} defaultLocale="en">
        <div>
          <Triage />
          <LocationCapture />
        </div>
      </IntlProvider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  itemsSpy.mockReset()
  statsSpy.mockReset()
  switchSessionSpy.mockReset()
  switchSessionSpy.mockResolvedValue(undefined)
  vi.mocked(api.updateInboxItemStatus).mockClear()
  setItems([])
})

describe('Triage page (inbox)', () => {
  it('renders the guided empty state when there are no items', () => {
    setItems([])
    renderPage()
    expect(screen.getByText('All clear.')).toBeInTheDocument()
    expect(screen.getByText(/automation inbox collects results/i)).toBeInTheDocument()
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
    fireEvent.click(screen.getByRole('button', { name: 'Goal' }))
    expect(setFilter).toHaveBeenCalledWith({ status: undefined, source: 'goal' })
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
    const { getSessionId } = setItems([
      makeItem({ id: 1, sessionId: 'sess-006' }),
      makeItem({ id: 2, sessionId: null }),
    ])
    renderPage()
    const continueButtons = screen.getAllByRole('button', { name: 'Continue this item session' })
    expect(continueButtons).toHaveLength(1)
    fireEvent.click(continueButtons[0])
    await waitFor(() => expect(getSessionId).toHaveBeenCalledWith(1))
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
    const cards = container.querySelectorAll('.glass-panel')
    expect(cards[0]).toHaveTextContent('Newer')
    expect(cards[1]).toHaveTextContent('Older')
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    const cardsAfter = container.querySelectorAll('.glass-panel')
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
      rerun: vi.fn(), getSessionId: vi.fn(),
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

// Tests for W11 Triage page: bulk operations, filters, sort, selection.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { IntlProvider } from 'react-intl'
import Triage from '@/pages/Triage'
import * as api from '@/lib/tauri-api'
import type { TriageItem, TriageStats } from '@/types'

// Test locale messages (minimal set for test assertions)
const testMessages: Record<string, string> = {
  'triage.title': 'Triage',
  'triage.subtitle': 'Failed runs and errors',
  'triage.empty.title': 'All clear.',
  'triage.empty.description': 'No items to triage.',
  'triage.empty.cta': 'Refresh',
  'triage.kind.label': 'Kind',
  'triage.filter.all': 'All',
  'triage.filter.unread': 'unread',
  'triage.filter.read': 'read',
  'triage.bulk.title': 'Bulk actions',
  'triage.bulk.markRead': 'Mark read',
  'triage.bulk.archive': 'Archive',
  'triage.bulk.clear': 'Clear',
  'triage.bulk.selected': '{count} selected',
  'triage.select.aria': 'Select item {id}',
  'triage.select.selectAll': 'Select all visible items',
  'triage.select.deselectAll': 'Deselect all',
  'triage.select.shown': 'shown',
  'triage.list.aria': 'Triage items. Use j or k to move focus, Enter to mark read, and a to archive.',
  'triage.markRead.aria': 'Mark item {id} as read',
  'triage.markRead.title': 'Mark read',
  'triage.archive.aria': 'Archive item {id}',
  'triage.archive.title': 'Archive',
  'triage.archived.label': 'Archived',
  'triage.archived.hide': 'Hide archived',
  'triage.read.label': 'Read',
  'triage.unread.title': 'Unread',
  'triage.unread': '{count} unread',
  'triage.total': '{count} total',
  'triage.toast.markRead': 'Marked {count} item as read',
  'triage.toast.markRead.plural': 'Marked {count} items as read',
  'triage.toast.archived': 'Archived {count} item',
  'triage.toast.archived.plural': 'Archived {count} items',
  'triage.noMatch.title': 'No matches',
  'triage.noMatch.description': 'No items match your filter',
  'triage.sort.label': 'Sort',
  'triage.sort.aria': 'Toggle sort order',
  'triage.sort.newest': 'Newest first',
  'triage.sort.oldest': 'Oldest first',
}

// useTriageItems returns { items, loading, error, filter, setFilter, refresh, markRead, archive }
// useTriageStats returns { stats, loading, error, refresh }
const itemsSpy = vi.hoisted(() => vi.fn())
const statsSpy = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/scheduled-tasks', () => ({
  useTriageItems: (filter?: unknown) => itemsSpy(filter),
  useTriageStats: () => statsSpy(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

function makeItem(o: Partial<TriageItem> & { id: string }): TriageItem {
  return {
    kind: o.kind ?? 'failed_run',
    message: o.message ?? 'boom',
    created_at: o.created_at ?? 1000,
    read: o.read ?? false,
    archived: o.archived ?? false,
    ...o,
  } as TriageItem
}

const baseStats: TriageStats = { total: 0, unread: 0, archived: 0, by_kind: {} }

function setItems(items: TriageItem[], stats: TriageStats = baseStats) {
  const markRead = vi.fn(async (_id: string) => true)
  const archive = vi.fn(async (_id: string) => true)
  itemsSpy.mockReturnValue({
    items, loading: false, error: null, filter: undefined, setFilter: vi.fn(),
    refresh: vi.fn(), markRead, archive,
  })
  statsSpy.mockReturnValue({ stats, loading: false, error: null, refresh: vi.fn() })
  return { markRead, archive }
}

function renderWithIntl(ui: React.ReactElement) {
  return render(
    <IntlProvider locale="en" messages={testMessages} defaultLocale="en">
      {ui}
    </IntlProvider>
  )
}

beforeEach(() => {
  itemsSpy.mockReset()
  statsSpy.mockReset()
  vi.mocked(api.markTriageRead).mockClear()
  vi.mocked(api.archiveTriageItem).mockClear()
  setItems([])
})

describe('Triage page', () => {
  it('renders empty state when there are no triage items', () => {
    setItems([])
    renderWithIntl(<Triage />)
    expect(screen.getByText('All clear.')).toBeInTheDocument()
  })

  it('renders Refresh CTA in empty state that calls hook refresh', () => {
    const refresh = vi.fn()
    itemsSpy.mockReturnValue({
      items: [], loading: false, error: null, filter: undefined, setFilter: vi.fn(),
      refresh, markRead: vi.fn(), archive: vi.fn(),
    })
    statsSpy.mockReturnValue({ stats: baseStats, loading: false, error: null, refresh: vi.fn() })
    renderWithIntl(<Triage />)
    fireEvent.click(screen.getByText('Refresh'))
    expect(refresh).toHaveBeenCalled()
  })

  it('renders one card per triage item and shows the message', () => {
    setItems([
      makeItem({ id: 'a', message: 'A failed' }),
      makeItem({ id: 'b', message: 'B failed' }),
    ])
    renderWithIntl(<Triage />)
    expect(screen.getByText('A failed')).toBeInTheDocument()
    expect(screen.getByText('B failed')).toBeInTheDocument()
  })

  it('shows total count from stats in the header', () => {
    setItems([], { total: 7, unread: 3, archived: 0, by_kind: {} })
    renderWithIntl(<Triage />)
    expect(screen.getByText('7 total')).toBeInTheDocument()
    expect(screen.getByText('3 unread')).toBeInTheDocument()
  })

  it('selecting items shows the bulk action bar with the count', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' })])
    renderWithIntl(<Triage />)
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('checkbox')[1])
    expect(screen.getByRole('region', { name: 'Bulk actions' })).toBeInTheDocument()
    expect(screen.getByText('1 selected')).toBeInTheDocument()
  })

  it('clicking select-all toggles all visible item checkboxes', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' }), makeItem({ id: 'c' })])
    renderWithIntl(<Triage />)
    const selectAll = screen.getByLabelText('Select all visible items')
    fireEvent.click(selectAll)
    expect(screen.getByText('3 selected')).toBeInTheDocument()
    // 1 select-all + 3 item checkboxes
    expect(screen.getAllByRole('checkbox').filter(cb => (cb as HTMLInputElement).checked).length).toBe(4)
  })

  it('Clear button in bulk bar empties the selection', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' })])
    renderWithIntl(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    fireEvent.click(screen.getByRole('button', { name: /^Clear$/ }))
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
  })

  it('Mark read bulk action calls api.markTriageRead for each selected id', async () => {
    setItems([
      makeItem({ id: 'a' }), makeItem({ id: 'b' }), makeItem({ id: 'c' }),
    ])
    const spy = vi.mocked(api.markTriageRead)
    renderWithIntl(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Mark read/ }))
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(3))
  })

  it('Archive bulk action calls api.archiveTriageItem for each selected id', async () => {
    setItems([makeItem({ id: 'x' }), makeItem({ id: 'y' })])
    const spy = vi.mocked(api.archiveTriageItem)
    renderWithIntl(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Archive/ }))
    await waitFor(() => expect(spy).toHaveBeenCalledTimes(2))
  })

  it('read filter hides read items when "unread" is selected', () => {
    setItems([
      makeItem({ id: 'a', message: 'unread one', read: false }),
      makeItem({ id: 'b', message: 'read one', read: true }),
    ])
    renderWithIntl(<Triage />)
    fireEvent.click(screen.getByRole('button', { name: 'unread' }))
    expect(screen.getByText('unread one')).toBeInTheDocument()
    expect(screen.queryByText('read one')).not.toBeInTheDocument()
  })

  it('sort toggle flips between newest-first and oldest-first', () => {
    setItems([
      makeItem({ id: 'old', message: 'Older', created_at: 1000 }),
      makeItem({ id: 'new', message: 'Newer', created_at: 5000 }),
    ])
    const { container } = renderWithIntl(<Triage />)
    // Initial: newest first → "Newer" appears before "Older"
    const cards = container.querySelectorAll('.glass-panel')
    expect(cards[0]).toHaveTextContent('Newer')
    expect(cards[1]).toHaveTextContent('Older')
    // Toggle to oldest first
    fireEvent.click(screen.getByRole('button', { name: 'Toggle sort order' }))
    const cardsAfter = container.querySelectorAll('.glass-panel')
    expect(cardsAfter[0]).toHaveTextContent('Older')
    expect(cardsAfter[1]).toHaveTextContent('Newer')
  })

  it('j moves focus and Enter marks the focused item read', () => {
    const { markRead } = setItems([
      makeItem({ id: 'a', message: 'first', read: false }),
      makeItem({ id: 'b', message: 'second', read: false }),
    ])
    renderWithIntl(<Triage />)
    const list = screen.getByRole('list', { name: /Triage items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    fireEvent.keyDown(list, { key: 'Enter' })
    expect(markRead).toHaveBeenCalledWith('a')
  })

  it('a archives the focused item', () => {
    const { archive } = setItems([
      makeItem({ id: 'a', message: 'first', read: false }),
      makeItem({ id: 'b', message: 'second', read: false }),
    ])
    renderWithIntl(<Triage />)
    const list = screen.getByRole('list', { name: /Triage items/ })
    list.focus()
    fireEvent.keyDown(list, { key: 'j' })
    fireEvent.keyDown(list, { key: 'a' })
    expect(archive).toHaveBeenCalledWith('a')
  })

  it('keyboard nav is ignored when the keystroke comes from a form control', () => {
    const { markRead } = setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' })])
    renderWithIntl(<Triage />)
    const list = screen.getByRole('list', { name: /Triage items/ })
    const checkbox = within(list).getAllByRole('checkbox')[0]
    list.focus()
    fireEvent.keyDown(checkbox, { key: 'j' })
    fireEvent.keyDown(checkbox, { key: 'Enter' })
    expect(markRead).not.toHaveBeenCalled()
  })
})

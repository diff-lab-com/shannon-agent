// Tests for W11 Triage page: bulk operations, filters, sort, selection.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import Triage from '@/pages/Triage'
import type { TriageItem, TriageStats } from '@/types'

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

beforeEach(() => {
  itemsSpy.mockReset()
  statsSpy.mockReset()
  setItems([])
})

describe('Triage page', () => {
  it('renders empty state when there are no triage items', () => {
    setItems([])
    render(<Triage />)
    expect(screen.getByText('All clear.')).toBeInTheDocument()
  })

  it('renders one card per triage item and shows the message', () => {
    setItems([
      makeItem({ id: 'a', message: 'A failed' }),
      makeItem({ id: 'b', message: 'B failed' }),
    ])
    render(<Triage />)
    expect(screen.getByText('A failed')).toBeInTheDocument()
    expect(screen.getByText('B failed')).toBeInTheDocument()
  })

  it('shows total count from stats in the header', () => {
    setItems([], { total: 7, unread: 3, archived: 0, by_kind: {} })
    render(<Triage />)
    expect(screen.getByText('7 total')).toBeInTheDocument()
    expect(screen.getByText('3 unread')).toBeInTheDocument()
  })

  it('selecting items shows the bulk action bar with the count', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' })])
    render(<Triage />)
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
    fireEvent.click(screen.getAllByRole('checkbox')[1])
    expect(screen.getByRole('region', { name: 'Bulk actions' })).toBeInTheDocument()
    expect(screen.getByText('1 selected')).toBeInTheDocument()
  })

  it('clicking select-all toggles all visible item checkboxes', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' }), makeItem({ id: 'c' })])
    render(<Triage />)
    const selectAll = screen.getByLabelText('Select all visible items')
    fireEvent.click(selectAll)
    expect(screen.getByText('3 selected')).toBeInTheDocument()
    // 1 select-all + 3 item checkboxes
    expect(screen.getAllByRole('checkbox').filter(cb => (cb as HTMLInputElement).checked).length).toBe(4)
  })

  it('Clear button in bulk bar empties the selection', () => {
    setItems([makeItem({ id: 'a' }), makeItem({ id: 'b' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    fireEvent.click(screen.getByRole('button', { name: /^Clear$/ }))
    expect(screen.queryByRole('region', { name: 'Bulk actions' })).not.toBeInTheDocument()
  })

  it('Mark read bulk action calls markRead for each selected id', async () => {
    const { markRead } = setItems([
      makeItem({ id: 'a' }), makeItem({ id: 'b' }), makeItem({ id: 'c' }),
    ])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Mark read/ }))
    await waitFor(() => expect(markRead).toHaveBeenCalledTimes(3))
  })

  it('Archive bulk action calls archive for each selected id', async () => {
    const { archive } = setItems([makeItem({ id: 'x' }), makeItem({ id: 'y' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Archive/ }))
    await waitFor(() => expect(archive).toHaveBeenCalledTimes(2))
  })

  it('Delete bulk action opens confirmation modal then deletes on confirm', async () => {
    const { archive } = setItems([makeItem({ id: 'x' }), makeItem({ id: 'y' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    const bar = screen.getByRole('region', { name: 'Bulk actions' })
    fireEvent.click(within(bar).getByRole('button', { name: /Delete/ }))
    // Confirmation dialog appears
    const dialog = await screen.findByRole('dialog', { name: 'Bulk delete items' })
    expect(dialog).toHaveTextContent(/Delete 2 items/)
    // Confirm
    fireEvent.click(within(dialog).getByRole('button', { name: /^Delete$/ }))
    await waitFor(() => expect(archive).toHaveBeenCalledTimes(2))
  })

  it('Cancel in bulk delete modal does NOT call archive', () => {
    const { archive } = setItems([makeItem({ id: 'x' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Select all visible items'))
    fireEvent.click(within(screen.getByRole('region', { name: 'Bulk actions' })).getByRole('button', { name: /Delete/ }))
    const dialog = screen.getByRole('dialog', { name: 'Bulk delete items' })
    fireEvent.click(within(dialog).getByRole('button', { name: /Cancel/ }))
    expect(archive).not.toHaveBeenCalled()
  })

  it('per-item Delete button opens single-item confirm and deletes on confirm', async () => {
    const { archive } = setItems([makeItem({ id: 'only' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Delete item'))
    const dialog = await screen.findByRole('dialog', { name: 'Delete item' })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Delete$/ }))
    await waitFor(() => expect(archive).toHaveBeenCalledWith('only'))
  })

  it('Cancel in single-item delete modal does NOT call archive', () => {
    const { archive } = setItems([makeItem({ id: 'only' })])
    render(<Triage />)
    fireEvent.click(screen.getByLabelText('Delete item'))
    const dialog = screen.getByRole('dialog', { name: 'Delete item' })
    fireEvent.click(within(dialog).getByRole('button', { name: /Cancel/ }))
    expect(archive).not.toHaveBeenCalled()
  })

  it('read filter hides read items when "unread" is selected', () => {
    setItems([
      makeItem({ id: 'a', message: 'unread one', read: false }),
      makeItem({ id: 'b', message: 'read one', read: true }),
    ])
    render(<Triage />)
    fireEvent.click(screen.getByRole('button', { name: 'unread' }))
    expect(screen.getByText('unread one')).toBeInTheDocument()
    expect(screen.queryByText('read one')).not.toBeInTheDocument()
  })

  it('sort toggle flips between newest-first and oldest-first', () => {
    setItems([
      makeItem({ id: 'old', message: 'Older', created_at: 1000 }),
      makeItem({ id: 'new', message: 'Newer', created_at: 5000 }),
    ])
    const { container } = render(<Triage />)
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
})

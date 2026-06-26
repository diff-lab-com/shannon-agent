import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import ConversationsList from '@/components/conversations/ConversationsList'
import type { SessionInfo } from '@/types'

vi.mock('@/context/AppContext', () => ({
  AppProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useApp: () => ({ switchSession: vi.fn() }),
}))

const now = Date.now()

const sessions: SessionInfo[] = [
  { id: 's1', title: 'Alpha chat', created_at: now, message_count: 3, is_pinned: true },
  { id: 's2', title: 'Beta chat', created_at: now - 86_400_000, message_count: 10, is_agent_run: true },
  { id: 's3', title: 'Gamma chat', created_at: now - 2 * 86_400_000, message_count: 1, is_scheduled: true },
  { id: 's4', title: 'Delta chat', created_at: now - 3 * 86_400_000, message_count: 5 },
]

function wrap(ui: React.ReactElement) {
  return <MemoryRouter>{ui}</MemoryRouter>
}

describe('ConversationsList', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders search input', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByPlaceholderText('Search conversations...')).toBeInTheDocument()
  })

  it('renders sort dropdown', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByLabelText('Sort conversations')).toBeInTheDocument()
  })

  it('renders all session titles by default', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByText('Alpha chat')).toBeInTheDocument()
    expect(screen.getByText('Beta chat')).toBeInTheDocument()
    expect(screen.getByText('Gamma chat')).toBeInTheDocument()
  })

  it('filters sessions by search query (case-insensitive)', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.change(screen.getByPlaceholderText('Search conversations...'), { target: { value: 'alpha' } })
    expect(screen.getByText('Alpha chat')).toBeInTheDocument()
    expect(screen.queryByText('Beta chat')).not.toBeInTheDocument()
    expect(screen.queryByText('Gamma chat')).not.toBeInTheDocument()
  })

  it('shows message count for each session', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByText(/3 messages?/)).toBeInTheDocument()
    expect(screen.getByText(/10 messages?/)).toBeInTheDocument()
  })

  it('sorts by most messages when selected', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.change(screen.getByLabelText('Sort conversations'), { target: { value: 'messages' } })
    // Beta (10) should come before Alpha (3) in document order
    const beta = screen.getByText('Beta chat')
    const alpha = screen.getByText('Alpha chat')
    expect(beta.compareDocumentPosition(alpha)).toBe(Node.DOCUMENT_POSITION_FOLLOWING)
  })

  it('shows empty state when no sessions match search', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.change(screen.getByPlaceholderText('Search conversations...'), { target: { value: 'nomatchxyz' } })
    expect(screen.getByText(/No conversations matching/)).toBeInTheDocument()
  })

  it('shows empty state when sessions list is empty', () => {
    render(wrap(<ConversationsList sessions={[]} />))
    expect(screen.getByText('No conversations yet.')).toBeInTheDocument()
  })

  it('groups sessions by date (shows date headings)', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    // Today, Yesterday, and 2 days ago should appear as date headers
    const headers = screen.getAllByRole('heading', { level: 3 })
    expect(headers.length).toBeGreaterThan(0)
  })

  it('search input is case-insensitive on partial match', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.change(screen.getByPlaceholderText('Search conversations...'), { target: { value: 'GAMMA' } })
    expect(screen.getByText('Gamma chat')).toBeInTheDocument()
    expect(screen.queryByText('Alpha chat')).not.toBeInTheDocument()
  })

  // C1: Filter tabs
  it('renders 4 filter tabs with counts', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByLabelText('All tab')).toHaveTextContent('All (4)')
    expect(screen.getByLabelText('Agent-run tab')).toHaveTextContent('Agent-run (1)')
    expect(screen.getByLabelText('Scheduled tab')).toHaveTextContent('Scheduled (1)')
    expect(screen.getByLabelText('Pinned tab')).toHaveTextContent('Pinned (1)')
  })

  it('defaults to All filter showing all sessions', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    expect(screen.getByLabelText('All tab')).toHaveAttribute('aria-selected', 'true')
    expect(screen.getByText('Alpha chat')).toBeInTheDocument()
    expect(screen.getByText('Beta chat')).toBeInTheDocument()
    expect(screen.getByText('Gamma chat')).toBeInTheDocument()
    expect(screen.getByText('Delta chat')).toBeInTheDocument()
  })

  it('filters to agent-run sessions when Agent-run tab clicked', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.click(screen.getByLabelText('Agent-run tab'))
    expect(screen.queryByText('Alpha chat')).not.toBeInTheDocument()
    expect(screen.getByText('Beta chat')).toBeInTheDocument()
    expect(screen.queryByText('Gamma chat')).not.toBeInTheDocument()
    expect(screen.queryByText('Delta chat')).not.toBeInTheDocument()
  })

  it('filters to scheduled sessions when Scheduled tab clicked', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.click(screen.getByLabelText('Scheduled tab'))
    expect(screen.getByText('Gamma chat')).toBeInTheDocument()
    expect(screen.queryByText('Alpha chat')).not.toBeInTheDocument()
    expect(screen.queryByText('Beta chat')).not.toBeInTheDocument()
  })

  it('filters to pinned sessions when Pinned tab clicked', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.click(screen.getByLabelText('Pinned tab'))
    expect(screen.getByText('Alpha chat')).toBeInTheDocument()
    expect(screen.queryByText('Beta chat')).not.toBeInTheDocument()
    expect(screen.queryByText('Gamma chat')).not.toBeInTheDocument()
  })

  it('shows filter-specific empty state when no pinned sessions', () => {
    render(wrap(<ConversationsList sessions={[{ id: 'x', title: 'Plain', created_at: now, message_count: 1 }]} />))
    fireEvent.click(screen.getByLabelText('Pinned tab'))
    expect(screen.getByText(/No pinned conversations yet/i)).toBeInTheDocument()
  })

  it('combines filter with search query', () => {
    render(wrap(<ConversationsList sessions={sessions} />))
    fireEvent.click(screen.getByLabelText('Agent-run tab'))
    fireEvent.change(screen.getByPlaceholderText('Search conversations...'), { target: { value: 'zzz' } })
    expect(screen.getByText(/No conversations matching/)).toBeInTheDocument()
  })
})

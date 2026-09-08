// PM-12 display half: the Settings card aggregates persisted ratings.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { FeedbackSummaryCard } from '@/components/settings/FeedbackSummaryCard'
import type * as TauriApi from '@/lib/tauri-api'

const listFeedbackSessions = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof TauriApi>('@/lib/tauri-api')
  return { ...actual, listFeedbackSessions: (...a: unknown[]) => listFeedbackSessions(...a) }
})

beforeEach(() => {
  listFeedbackSessions.mockReset()
})

describe('FeedbackSummaryCard', () => {
  it('renders the empty state when no feedback exists', async () => {
    listFeedbackSessions.mockResolvedValue([])
    render(<FeedbackSummaryCard />)
    await waitFor(() => expect(screen.getByText(/no feedback recorded/i)).toBeInTheDocument())
  })

  it('lists one row per session with up/down counts', async () => {
    listFeedbackSessions.mockResolvedValue([
      { session_id: '8b1f0a22-1e1f-4d2e-9a11-222333444555', up: 3, down: 1, updated_at: 1_700_000_000 },
      { session_id: '9c2a1b33-2f2e-4d3e-8a22-333444555666', up: 0, down: 2, updated_at: 1_700_000_100 },
    ])
    render(<FeedbackSummaryCard />)
    await waitFor(() => expect(screen.getAllByRole('listitem')).toHaveLength(2))
    expect(screen.getByText('8b1f0a22')).toBeInTheDocument()
    // Accessible names carry the counts for screen readers (2 rows each).
    expect(screen.getAllByLabelText('thumbs up')).toHaveLength(2)
    expect(screen.getAllByLabelText('thumbs down')).toHaveLength(2)
  })
})

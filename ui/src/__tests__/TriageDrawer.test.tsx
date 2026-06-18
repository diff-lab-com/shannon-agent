import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import userEvent from '@testing-library/user-event'
import { TriageDrawer } from '@/components/triage/TriageDrawer'
import * as api from '@/lib/tauri-api'

// Mock the tauri-api module
vi.mock('@/lib/tauri-api', () => ({
  listTriageItems: vi.fn(),
  markTriageRead: vi.fn(),
  archiveTriageItem: vi.fn(),
}))

const mockTriageItems = [
  {
    id: '1',
    kind: 'failed_run',
    message: 'Task failed',
    created_at: 1700000000,
    read: false,
    archived: false,
    task_id: 'task-1',
    task_name: 'Test Task',
  },
  {
    id: '2',
    kind: 'needs_review',
    message: 'Please review',
    created_at: 1700000100,
    read: true,
    archived: false,
  },
]

describe('TriageDrawer', () => {
  const mockOnStatsRefresh = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    // Mock successful API responses
    vi.mocked(api.listTriageItems).mockResolvedValue(mockTriageItems)
    vi.mocked(api.markTriageRead).mockResolvedValue(true)
    vi.mocked(api.archiveTriageItem).mockResolvedValue(true)
  })

  it('renders empty state when no items', async () => {
    vi.mocked(api.listTriageItems).mockResolvedValue([])

    render(<TriageDrawer open={true} onOpenChange={() => {}} />)

    await waitFor(() => {
      expect(screen.getByText(/no items need attention/i)).toBeInTheDocument()
    })
  })

  it('renders triage items from mock', async () => {
    render(<TriageDrawer open={true} onOpenChange={() => {}} />)

    await waitFor(() => {
      expect(screen.getByText('Task failed')).toBeInTheDocument()
      expect(screen.getByText('Please review')).toBeInTheDocument()
    })
  })

  it('shows loading state', () => {
    vi.mocked(api.listTriageItems).mockImplementation(
      () => new Promise(() => {}) // Never resolves
    )

    render(<TriageDrawer open={true} onOpenChange={() => {}} />)

    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })

  it('calls onStatsRefresh after successful mark-read', async () => {
    render(<TriageDrawer open={true} onOpenChange={() => {}} onStatsRefresh={mockOnStatsRefresh} />)

    await waitFor(() => {
      expect(screen.getByText('Task failed')).toBeInTheDocument()
    })

    // Click the "Mark Read" button for the first item
    const markReadButtons = screen.getAllByText('Mark Read')
    await userEvent.click(markReadButtons[0])

    await waitFor(() => {
      expect(api.markTriageRead).toHaveBeenCalledWith('1')
      expect(mockOnStatsRefresh).toHaveBeenCalledTimes(1)
    })
  })

  it('calls onStatsRefresh after successful archive', async () => {
    render(<TriageDrawer open={true} onOpenChange={() => {}} onStatsRefresh={mockOnStatsRefresh} />)

    await waitFor(() => {
      expect(screen.getByText('Task failed')).toBeInTheDocument()
    })

    // Click the "Archive" button for the first item
    const archiveButtons = screen.getAllByText('Archive')
    await userEvent.click(archiveButtons[0])

    await waitFor(() => {
      expect(api.archiveTriageItem).toHaveBeenCalledWith('1')
      expect(mockOnStatsRefresh).toHaveBeenCalledTimes(1)
    })
  })
})

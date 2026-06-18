import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { TriageDrawer } from '@/components/triage/TriageDrawer'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/context/I18nContext'

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
  beforeEach(() => {
    vi.clearAllMocks()
    // Mock successful API responses
    vi.mocked(api.listTriageItems).mockResolvedValue(mockTriageItems)
    vi.mocked(api.markTriageRead).mockResolvedValue(true)
    vi.mocked(api.archiveTriageItem).mockResolvedValue(true)
  })

  it('renders empty state when no items', async () => {
    vi.mocked(api.listTriageItems).mockResolvedValue([])

    render(
      <I18nProvider>
        <TriageDrawer open={true} onOpenChange={() => {}} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText(/no items need attention/i)).toBeInTheDocument()
    })
  })

  it('renders triage items from mock', async () => {
    render(
      <I18nProvider>
        <TriageDrawer open={true} onOpenChange={() => {}} />
      </I18nProvider>
    )

    await waitFor(() => {
      expect(screen.getByText('Task failed')).toBeInTheDocument()
      expect(screen.getByText('Please review')).toBeInTheDocument()
    })
  })

  it('shows loading state', () => {
    vi.mocked(api.listTriageItems).mockImplementation(
      () => new Promise(() => {}) // Never resolves
    )

    render(
      <I18nProvider>
        <TriageDrawer open={true} onOpenChange={() => {}} />
      </I18nProvider>
    )

    expect(screen.getByText(/loading/i)).toBeInTheDocument()
  })
})

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { Sidebar } from '@/components/Sidebar'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/context/I18nContext'

// Mock the tauri-api module
vi.mock('@/lib/tauri-api', () => ({
  getTriageStats: vi.fn(),
}))

const mockTriageStats = {
  total: 5,
  unread: 3,
  archived: 2,
  by_kind: {
    failed_run: 2,
    needs_review: 1,
  },
}

describe('Sidebar Badge', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getTriageStats).mockResolvedValue(mockTriageStats)
  })

  it('shows unread count badge when > 0', async () => {
    render(
      <I18nProvider>
        <Sidebar />
      </I18nProvider>
    )

    await waitFor(() => {
      const badge = screen.getByText('3')
      expect(badge).toBeInTheDocument()
      expect(badge).toHaveClass('bg-error')
    })
  })

  it('hides badge when unread count is 0', async () => {
    vi.mocked(api.getTriageStats).mockResolvedValue({
      ...mockTriageStats,
      unread: 0,
    })

    render(
      <I18nProvider>
        <Sidebar />
      </I18nProvider>
    )

    await waitFor(() => {
      const badge = screen.queryByText('0')
      expect(badge).not.toBeInTheDocument()
    })
  })
})

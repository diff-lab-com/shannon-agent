import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import Usage from '@/pages/Usage'
import * as api from '@/lib/tauri-api'
import type { UsageStats } from '@/types'

const bucket = {
  input_tokens: 1000,
  output_tokens: 500,
  cache_creation_tokens: 100,
  cache_read_tokens: 50,
  cost_usd: 0.25,
  requests: 3,
}

const fixture: UsageStats = {
  days: 30,
  totals: { label: 'total', ...bucket },
  by_model: [{ label: 'claude-sonnet-4-6', ...bucket }],
  by_provider: [{ label: 'anthropic', ...bucket }],
  by_day: [{ label: '2024-01-02', ...bucket }],
}

function renderUsage() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Usage />
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('Usage page', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getUsageStats).mockReset()
  })

  it('renders totals and breakdowns when data is present', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => {
      expect(screen.getByText('claude-sonnet-4-6')).toBeInTheDocument()
    })
    expect(screen.getByText('By model')).toBeInTheDocument()
    expect(screen.getByText('By provider')).toBeInTheDocument()
    expect(screen.getByText('By day')).toBeInTheDocument()
    expect(screen.getByText('anthropic')).toBeInTheDocument()
    expect(screen.getByText('2024-01-02')).toBeInTheDocument()
  })

  it('shows the empty state when nothing is recorded yet', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue({
      days: 30,
      totals: { label: 'total', ...bucket, requests: 0 },
      by_model: [],
      by_provider: [],
      by_day: [],
    })
    renderUsage()

    await waitFor(() => {
      expect(
        screen.getByText('No usage recorded yet. Send a message to start tracking.'),
      ).toBeInTheDocument()
    })
  })

  it('re-fetches when the day range changes', async () => {
    vi.mocked(api.getUsageStats).mockResolvedValue(fixture)
    renderUsage()

    await waitFor(() => expect(api.getUsageStats).toHaveBeenCalledWith(30))

    fireEvent.click(screen.getByText('7 days'))
    await waitFor(() => expect(api.getUsageStats).toHaveBeenCalledWith(7))
  })
})

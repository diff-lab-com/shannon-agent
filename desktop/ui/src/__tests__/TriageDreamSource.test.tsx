// Dream pass (梦境提炼) coverage on the Triage page: the `dream_report`
// source's meta mapping (icon / colour / i18n label key), its presence in
// the source filter options, and the card's 查看报告 jump to /memory.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { AppProvider } from '@/context/AppContext'
import Triage, { sourceMeta, SOURCE_OPTIONS } from '@/pages/Triage'
import * as api from '@/lib/tauri-api'

describe('Triage — dream_report source mapping', () => {
  it('maps dream_report to the bedtime icon, tertiary colour, and dream label key', () => {
    expect(sourceMeta('dream_report')).toEqual({
      icon: 'bedtime',
      color: 'text-tertiary',
      labelKey: 'inbox.source.dream_report',
    })
  })

  it('offers dream_report in the source filter options', () => {
    expect(SOURCE_OPTIONS).toContain('dream_report')
  })
})

function LocationProbe() {
  const location = useLocation()
  return <div data-testid="triage-location">{location.pathname}</div>
}

describe('Triage — dream report card action', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.listInboxItems).mockResolvedValue([
      {
        id: 1,
        source: 'dream_report',
        sourceId: 'dream-2026-09-24',
        sessionId: null,
        title: 'Dream distillation report',
        summary: 'Merged 2 · Removed 1 · New insights 1 · 3 session(s) scanned',
        error: null,
        status: 'pending',
        createdAtMs: Date.parse('2026-09-24T02:00:00Z'),
        updatedAtMs: Date.parse('2026-09-24T02:00:00Z'),
      },
    ])
    vi.mocked(api.getInboxStats).mockResolvedValue({ pending: 1, today: 1 })
  })

  it('renders the 查看报告 action and jumps to /memory', async () => {
    render(
      <AppProvider>
        <MemoryRouter initialEntries={['/triage']}>
          <Triage />
          <LocationProbe />
        </MemoryRouter>
      </AppProvider>,
    )

    // The dream_report label + summary render on the card.
    expect(await screen.findByText('Dream report')).toBeInTheDocument()
    expect(screen.getByText(/Merged 2/)).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'View the dream distillation report on the Memory page' }))
    await waitFor(() => {
      expect(screen.getByTestId('triage-location')).toHaveTextContent('/memory')
    })
  })
})

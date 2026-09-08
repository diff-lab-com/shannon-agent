import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { Sidebar } from '@/components/Sidebar'

// The /triage badge reads `get_inbox_stats().pending` via useInboxStats (P0-3).
const statsMock = vi.hoisted(() => ({
  current: { pending: 3, today: 5 },
}))

vi.mock('@/hooks/inbox', () => ({
  useInboxStats: () => ({
    stats: statsMock.current,
    refresh: vi.fn(),
  }),
}))

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>{ui}</MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Sidebar Badge', () => {
  beforeEach(() => {
    statsMock.current = { pending: 3, today: 5 }
  })

  it('shows pending count badge when > 0', async () => {
    render(wrap(<Sidebar />))
    await waitFor(() => {
      const badge = screen.getByText('3')
      expect(badge).toBeInTheDocument()
    })
  })

  it('hides badge when pending count is 0', async () => {
    statsMock.current = { pending: 0, today: 0 }
    render(wrap(<Sidebar />))
    await waitFor(() => {
      expect(screen.queryByText('0')).not.toBeInTheDocument()
    })
  })
})

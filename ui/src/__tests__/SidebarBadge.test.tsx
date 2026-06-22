import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { Sidebar } from '@/components/Sidebar'

const statsMock = vi.hoisted(() => ({
  current: { unread: 3, total: 5 },
}))

vi.mock('@/hooks/scheduled-tasks', () => ({
  useTriageStats: () => ({
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
    statsMock.current = { unread: 3, total: 5 }
  })

  it('shows unread count badge when > 0', async () => {
    render(wrap(<Sidebar />))
    await waitFor(() => {
      const badge = screen.getByText('3')
      expect(badge).toBeInTheDocument()
    })
  })

  it('hides badge when unread count is 0', async () => {
    statsMock.current = { unread: 0, total: 0 }
    render(wrap(<Sidebar />))
    await waitFor(() => {
      expect(screen.queryByText('0')).not.toBeInTheDocument()
    })
  })
})

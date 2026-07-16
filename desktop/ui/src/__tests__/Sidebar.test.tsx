import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { Sidebar, SIDEBAR_MODE_KEY } from '@/components/Sidebar'

// Mock useTriageStats hook
vi.mock('@/hooks/scheduled-tasks', () => ({
  useTriageStats: () => ({
    stats: { unread: 3, total: 5 },
    refresh: vi.fn(),
  }),
}))

function wrap(ui: React.ReactElement, { path = '/chat' } = {}) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={[path]}>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

// Helper component to capture current location
function LocationCapture() {
  const location = useLocation()
  return <div data-testid="current-location">{location.pathname}</div>
}

describe('Sidebar', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('renders Shannon branding', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Shannon')).toBeInTheDocument()
  })

  it('renders subtitle', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Your AI Workspace')).toBeInTheDocument()
  })

  it('renders New Chat button', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('New Chat')).toBeInTheDocument()
  })

  it('renders primary nav links', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
  })

  it('renders Settings section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Settings')).toBeInTheDocument()
  })

  it('expands Settings section on click', () => {
    render(wrap(<Sidebar />))
    // Settings sub-links are collapsed by default
    expect(screen.queryByText('General')).not.toBeInTheDocument()

    fireEvent.click(screen.getByText('Settings'))
    expect(screen.getByText('General')).toBeInTheDocument()
    expect(screen.getByText('Theme')).toBeInTheDocument()
    expect(screen.getByText('Models')).toBeInTheDocument()
    expect(screen.getByText('Notifications')).toBeInTheDocument()
    // Billing + Advanced are dev-only (P3-2): hidden in the default Simple mode.
    expect(screen.queryByText('Usage & Billing')).not.toBeInTheDocument()
    expect(screen.queryByText('Advanced')).not.toBeInTheDocument()
  })
})

describe('Sidebar — Simple mode (default)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('defaults to Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByRole('button', { name: /Switch to Advanced mode/ })).toBeInTheDocument()
    expect(screen.getByText('Simple mode')).toBeInTheDocument()
  })

  it('shows flat Extensions entry in Simple mode (no dev sub-links)', () => {
    render(wrap(<Sidebar />))
    // P1-2: Simple mode surfaces a flat Extensions link to the Hub index so
    // general users can reach it without dev mode. The dev-mode collapsible
    // group (with Skills / My Agents / Connections sub-links) stays hidden.
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.queryByText('Skills')).not.toBeInTheDocument()
    expect(screen.queryByText('My Agents')).not.toBeInTheDocument()
  })

  it('hides OPC section in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByText('OPC')).not.toBeInTheDocument()
    expect(screen.queryByText('One Person Company')).not.toBeInTheDocument()
  })

  it('hides Quick Fix, Editor in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByText('Quick Fix')).not.toBeInTheDocument()
    expect(screen.queryByText('Editor')).not.toBeInTheDocument()
  })

  it('still shows core nav in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
  })

  it('toggles to Advanced mode on mode button click', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Switch to Advanced mode/ }))
    // Now in Advanced mode — Extensions visible
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.getByText('Advanced mode')).toBeInTheDocument()
  })

  it('persists mode to localStorage', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Switch to Advanced mode/ }))
    expect(window.localStorage.getItem(SIDEBAR_MODE_KEY)).toBe('dev')
  })

  it('remembers Advanced mode from localStorage on subsequent mount', () => {
    window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
    render(wrap(<Sidebar />))
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.getByText('Advanced mode')).toBeInTheDocument()
  })

  it('mode toggle button has correct aria-pressed', () => {
    render(wrap(<Sidebar />))
    const toggle = screen.getByRole('button', { name: /Switch to Advanced mode/ })
    expect(toggle).toHaveAttribute('aria-pressed', 'false')
    fireEvent.click(toggle)
    expect(screen.getByRole('button', { name: /Switch to Simple mode/ })).toHaveAttribute('aria-pressed', 'true')
  })
})

describe('Sidebar — Advanced mode', () => {
  beforeEach(() => {
    window.localStorage.clear()
    window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
  })

  it('renders Extensions section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Extensions')).toBeInTheDocument()
  })

  it('renders OPC section', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('OPC')).toBeInTheDocument()
  })

  it('renders extension sub-links when expanded', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Skills')).toBeInTheDocument()
    expect(screen.getByText('My Agents')).toBeInTheDocument()
    expect(screen.getByText('Connections')).toBeInTheDocument()
  })

  it('renders OPC sub-link when expanded', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('One Person Company')).toBeInTheDocument()
  })

  it('shows dev-only Settings sub-links (Billing, Advanced) when expanded', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByText('Settings'))
    expect(screen.getByText('Usage & Billing')).toBeInTheDocument()
    expect(screen.getByText('Advanced')).toBeInTheDocument()
  })

  it('collapses and expands Extensions section', () => {
    render(wrap(<Sidebar />))
    // Extensions is open by default
    expect(screen.getByText('Skills')).toBeInTheDocument()

    // Click Extensions button to collapse
    const integrationsButtons = screen.getAllByText('Extensions')
    fireEvent.click(integrationsButtons[0])

    // Sub-links should be gone
    expect(screen.queryByText('Skills')).not.toBeInTheDocument()

    // Click again to expand
    fireEvent.click(screen.getByText('Extensions'))
    expect(screen.getByText('Skills')).toBeInTheDocument()
  })

  it('shows experiment badge on OPC', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Experiment')).toBeInTheDocument()
  })

  it('toggles back to Simple mode on click', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Switch to Simple mode/ }))
    // P1-2: Simple mode still shows the flat Extensions link; what disappears
    // is the dev-mode Extensions group and its sub-links (Skills).
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.queryByText('Skills')).not.toBeInTheDocument()
    expect(screen.getByText('Simple mode')).toBeInTheDocument()
  })
})

describe('Sidebar — Navigation', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('navigates to /tasks when clicking Scheduled', async () => {
    render(
      wrap(
        <>
          <Sidebar />
          <LocationCapture />
        </>
      )
    )

    const scheduledLink = screen.getByText('Scheduled')
    fireEvent.click(scheduledLink)

    await waitFor(() => {
      const location = screen.getByTestId('current-location')
      expect(location.textContent).toBe('/tasks')
    })
  })

  it('renders Triage button with badge when there are unread items', () => {
    render(wrap(<Sidebar />))

    expect(screen.getByText('Triage')).toBeInTheDocument()
    expect(screen.getByText('3')).toBeInTheDocument() // Badge shows unread count
  })

  it('Triage link has proper aria-label', () => {
    render(wrap(<Sidebar />))

    const triageLink = screen.getByRole('link', { name: /Open Triage page/i })
    expect(triageLink).toBeInTheDocument()
  })
})

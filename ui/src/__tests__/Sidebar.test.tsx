import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { Sidebar, SIDEBAR_MODE_KEY } from '@/components/Sidebar'

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
    expect(screen.getByText('Projects')).toBeInTheDocument()
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
    expect(screen.getByText('Usage & Billing')).toBeInTheDocument()
    expect(screen.getByText('Advanced')).toBeInTheDocument()
  })
})

describe('Sidebar — Simple mode (default)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('defaults to Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByRole('button', { name: /Switch to Dev mode/ })).toBeInTheDocument()
    expect(screen.getByText('Simple mode')).toBeInTheDocument()
  })

  it('hides Extensions section in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByText('Extensions')).not.toBeInTheDocument()
  })

  it('shows Automations section in Simple mode (collapsed by default)', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Automations')).toBeInTheDocument()
    // Sub-links hidden until expanded
    expect(screen.queryByText('Schedules')).not.toBeInTheDocument()
    expect(screen.queryByText('Triggers')).not.toBeInTheDocument()
    expect(screen.queryByText('Permission Modes')).not.toBeInTheDocument()
  })

  it('expands Automations in Simple mode on click', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByText('Automations'))
    expect(screen.getByText('Schedules')).toBeInTheDocument()
    expect(screen.getByText('Triggers')).toBeInTheDocument()
    expect(screen.getByText('Permission Modes')).toBeInTheDocument()
  })

  it('hides OPC section in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByText('OPC')).not.toBeInTheDocument()
    expect(screen.queryByText('One Person Company')).not.toBeInTheDocument()
  })

  it('hides Quick Fix, Editor, Performance in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByText('Quick Fix')).not.toBeInTheDocument()
    expect(screen.queryByText('Editor')).not.toBeInTheDocument()
    expect(screen.queryByText('Performance')).not.toBeInTheDocument()
  })

  it('still shows core nav in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Projects')).toBeInTheDocument()
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
    expect(screen.getByText('Conversations')).toBeInTheDocument()
    expect(screen.getByText('Inbox')).toBeInTheDocument()
  })

  it('toggles to Dev mode on mode button click', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Switch to Dev mode/ }))
    // Now in Dev mode — Extensions visible
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.getByText('Dev mode')).toBeInTheDocument()
  })

  it('persists mode to localStorage', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Switch to Dev mode/ }))
    expect(window.localStorage.getItem(SIDEBAR_MODE_KEY)).toBe('dev')
  })

  it('remembers Dev mode from localStorage on subsequent mount', () => {
    window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
    render(wrap(<Sidebar />))
    expect(screen.getByText('Extensions')).toBeInTheDocument()
    expect(screen.getByText('Dev mode')).toBeInTheDocument()
  })

  it('mode toggle button has correct aria-pressed', () => {
    render(wrap(<Sidebar />))
    const toggle = screen.getByRole('button', { name: /Switch to Dev mode/ })
    expect(toggle).toHaveAttribute('aria-pressed', 'false')
    fireEvent.click(toggle)
    expect(screen.getByRole('button', { name: /Switch to Simple mode/ })).toHaveAttribute('aria-pressed', 'true')
  })
})

describe('Sidebar — Dev mode', () => {
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
    expect(screen.getByText('Data Sources')).toBeInTheDocument()
  })

  it('renders OPC sub-link when expanded', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByText('One Person Company')).toBeInTheDocument()
  })

  it('collapses and expands Extensions section', () => {
    render(wrap(<Sidebar />))
    // Extensions is open by default
    expect(screen.getByText('Skills')).toBeInTheDocument()

    // Click Extensions button to collapse
    const extensionsButtons = screen.getAllByText('Extensions')
    fireEvent.click(extensionsButtons[0])

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
    expect(screen.queryByText('Extensions')).not.toBeInTheDocument()
    expect(screen.getByText('Simple mode')).toBeInTheDocument()
  })
})

import { describe, it, expect, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter, Routes, Route, useNavigate } from 'react-router-dom'
import { Layout } from '@/components/Layout'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>
          <Routes>
            <Route element={ui}>
              <Route path="/chat" element={<div data-testid="outlet-content">Chat Page Content</div>} />
            </Route>
          </Routes>
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Layout', () => {
  it('renders sidebar', () => {
    render(wrap(<Layout />))
    expect(screen.getAllByText('Shannon').length).toBeGreaterThanOrEqual(1)
  })

  it('renders outlet content', () => {
    render(wrap(<Layout />))
    expect(screen.getByTestId('outlet-content')).toBeInTheDocument()
  })

  it('renders no global footer (裁决 D 2026-10-15: ChatStatusBar is the only status line)', () => {
    render(wrap(<Layout />))
    // 裁决 D: the footer is removed entirely — the design has one status
    // line (the chat page's ChatStatusBar), so the shell renders none, not
    // even the idle "Shannon Code" brand segment.
    expect(screen.queryByRole('contentinfo')).not.toBeInTheDocument()
    expect(screen.queryByText('Shannon Code')).not.toBeInTheDocument()
  })
})

// ── H3 (导航批, plan §2.3-4): restart-restore of the last route ─────────
// The shell writes the settled location to `shannon.lastRoute` (300ms
// debounce, transient routes excluded) and replaces the default /chat
// landing with it on the next boot. Deep links and window mode never
// restore.
describe('Layout — H3 restart-restore', () => {
  const LAST_ROUTE_KEY = 'shannon.lastRoute'

  afterEach(() => {
    window.localStorage.removeItem(LAST_ROUTE_KEY)
  })

  function NavProbe({ to, label, testid }: { to: string; label: string; testid: string }) {
    const navigate = useNavigate()
    return (
      <button data-testid={testid} onClick={() => navigate(to)}>
        {label}
      </button>
    )
  }

  function wrapRoutes(initialEntry: string) {
    return (
      <I18nProvider>
        <AppProvider>
          <MemoryRouter initialEntries={[initialEntry]}>
            {/* Nav probes live OUTSIDE <Routes> so they survive the
                route changes they trigger. */}
            <NavProbe to="/tasks" label="go-tasks" testid="nav-tasks" />
            <NavProbe to="/welcome" label="go-welcome" testid="nav-welcome" />
            <Routes>
              <Route element={<Layout />}>
                <Route path="/chat" element={<div data-testid="chat-page" />} />
                <Route path="/tasks" element={<div data-testid="tasks-page" />} />
                <Route path="/welcome" element={<div data-testid="welcome-page" />} />
                <Route path="/timeline/:id" element={<div data-testid="timeline-page" />} />
                <Route path="/settings" element={<div data-testid="settings-page" />} />
              </Route>
            </Routes>
          </MemoryRouter>
        </AppProvider>
      </I18nProvider>
    )
  }

  it('restores the stored route over the default /chat landing', async () => {
    window.localStorage.setItem(LAST_ROUTE_KEY, '/tasks')
    render(wrapRoutes('/chat'))
    expect(await screen.findByTestId('tasks-page')).toBeInTheDocument()
    expect(screen.queryByTestId('chat-page')).not.toBeInTheDocument()
  })

  it('never restores into a transient route (stale pre-exclusion value)', async () => {
    window.localStorage.setItem(LAST_ROUTE_KEY, '/timeline/abc')
    render(wrapRoutes('/chat'))
    expect(await screen.findByTestId('chat-page')).toBeInTheDocument()
    expect(screen.queryByTestId('timeline-page')).not.toBeInTheDocument()
  })

  it('an explicit deep link wins — no restore off the default landing', async () => {
    window.localStorage.setItem(LAST_ROUTE_KEY, '/tasks')
    render(wrapRoutes('/settings'))
    expect(await screen.findByTestId('settings-page')).toBeInTheDocument()
    expect(screen.queryByTestId('tasks-page')).not.toBeInTheDocument()
  })

  it('records the settled route after the debounce window', async () => {
    render(wrapRoutes('/chat'))
    expect(await screen.findByTestId('chat-page')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('nav-tasks'))
    await waitFor(() => {
      expect(window.localStorage.getItem(LAST_ROUTE_KEY)).toBe('/tasks')
    })
  })

  it('never records a transient route (/welcome)', async () => {
    // Boot restores into /tasks (stored), then hop to /welcome — the writer
    // must skip it, leaving the previous value untouched.
    window.localStorage.setItem(LAST_ROUTE_KEY, '/tasks')
    render(wrapRoutes('/chat'))
    expect(await screen.findByTestId('tasks-page')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('nav-welcome'))
    expect(await screen.findByTestId('welcome-page')).toBeInTheDocument()
    // Outlive the 300ms debounce window, then confirm nothing was written.
    await waitFor(() => {
      expect(window.localStorage.getItem(LAST_ROUTE_KEY)).toBe('/tasks')
    }, { timeout: 1500 })
    await new Promise(resolve => setTimeout(resolve, 450))
    expect(window.localStorage.getItem(LAST_ROUTE_KEY)).toBe('/tasks')
  })
})

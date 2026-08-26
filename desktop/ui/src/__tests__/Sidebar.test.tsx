import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
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

// U1 — the sidebar SessionsSection is the app's single session list (the
// Chat-page session rail was removed). These cases were migrated from
// Chat.test.tsx plus new coverage for the ⋯ menu, pin/drag persistence.
describe('Sidebar — Sessions rail (U1)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.clearAllMocks()
  })

  const now = Date.now()
  const mockSessions = [
    { id: 's1', title: 'Alpha Chat', created_at: now - 1000, message_count: 3 },
    { id: 's2', title: 'Beta Debug', created_at: now - 2000, message_count: 1 },
    { id: 's3', title: 'Gamma Plan', created_at: now - 3000, message_count: 0 },
  ]

  async function renderWithSessions(sessions = mockSessions) {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.listSessions).mockResolvedValue(sessions as any)
    const utils = render(wrap(<Sidebar />))
    await screen.findByText(sessions[0].title)
    return utils
  }

  function row(title: string) {
    return screen.getByRole('button', { name: new RegExp(`^${title}$`) })
  }

  function rowItem(title: string) {
    return row(title).closest('[role="listitem"]') as HTMLElement
  }

  async function openMenu(title: string) {
    fireEvent.click(screen.getByRole('button', { name: `Actions for ${title}` }))
    await screen.findByRole('menu')
  }

  it('renders every session (no 8-item cap)', async () => {
    const many = Array.from({ length: 12 }, (_, i) => ({
      id: `s${i}`, title: `Session ${i}`, created_at: now - i, message_count: 0,
    }))
    await renderWithSessions(many)
    for (let i = 0; i < 12; i++) {
      expect(screen.getByRole('button', { name: `Session ${i}` })).toBeInTheDocument()
    }
  })

  it('switches session on row click and marks it current', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.switchSession).mockResolvedValue([] as any)
    await renderWithSessions()
    fireEvent.click(row('Beta Debug'))
    await waitFor(() => expect(api.switchSession).toHaveBeenCalledWith('s2'))
    await waitFor(() => expect(row('Beta Debug')).toHaveAttribute('aria-current', 'page'))
    expect(row('Alpha Chat')).not.toHaveAttribute('aria-current')
  })

  it('activates a row with the keyboard (Enter)', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.switchSession).mockResolvedValue([] as any)
    await renderWithSessions()
    row('Gamma Plan').focus()
    await userEvent.setup().keyboard('{Enter}')
    await waitFor(() => expect(api.switchSession).toHaveBeenCalledWith('s3'))
  })

  it('filters by title client-side for short queries', async () => {
    await renderWithSessions()
    fireEvent.change(screen.getByLabelText('Search sessions'), { target: { value: 'be' } })
    expect(screen.getByRole('button', { name: 'Beta Debug' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Alpha Chat' })).not.toBeInTheDocument()
  })

  it('calls backend search after debounce for queries ≥ 3 chars and keeps rail order', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    // s3 is a content match — its title doesn't contain the query.
    vi.mocked(api.searchSessions).mockResolvedValue([{ ...mockSessions[2] }] as any)
    fireEvent.change(screen.getByLabelText('Search sessions'), { target: { value: 'use' } })
    await waitFor(() => expect(api.searchSessions).toHaveBeenCalledWith('use'))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Gamma Plan' })).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Alpha Chat' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Beta Debug' })).not.toBeInTheDocument()
    })
  })

  it('does not call backend when query is shorter than 3 chars', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    fireEvent.change(screen.getByLabelText('Search sessions'), { target: { value: 'go' } })
    expect(api.searchSessions).not.toHaveBeenCalled()
  })

  it('renames inline via the ⋯ menu (Enter commits, Escape cancels)', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    await openMenu('Alpha Chat')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const input = screen.getByLabelText('Rename') as HTMLInputElement
    expect(input.value).toBe('Alpha Chat')
    fireEvent.change(input, { target: { value: 'Alpha Renamed' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(api.renameSession).toHaveBeenCalledWith('s1', 'Alpha Renamed'))
    // Escape path
    await openMenu('Beta Debug')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename' }))
    const input2 = screen.getByLabelText('Rename')
    fireEvent.change(input2, { target: { value: 'nope' } })
    fireEvent.keyDown(input2, { key: 'Escape' })
    expect(api.renameSession).toHaveBeenCalledTimes(1)
  })

  it('pins a session to the top and persists across remounts', async () => {
    await renderWithSessions()
    expect(row('Gamma Plan').closest('[role="listitem"]')).toBeTruthy()
    await openMenu('Gamma Plan')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Pin' }))
    // Pinned row sorts first and shows the pin glyph.
    const first = screen.getAllByRole('listitem')[0]
    expect(within(first).getByText('Gamma Plan')).toBeInTheDocument()
    expect(within(first).getByText('push_pin')).toBeInTheDocument()
    expect(JSON.parse(window.localStorage.getItem('shannon-sessions-pinned')!)).toEqual(['s3'])
    // Unmount + remount — pin survives (was component state before U1).
    cleanup()
    await renderWithSessions()
    const firstAfter = screen.getAllByRole('listitem')[0]
    expect(within(firstAfter).getByText('Gamma Plan')).toBeInTheDocument()
    // Menu now offers Unpin.
    await openMenu('Gamma Plan')
    expect(screen.getByRole('menuitem', { name: 'Unpin' })).toBeInTheDocument()
  })

  it('persists drag reorder to localStorage and restores it on remount', async () => {
    await renderWithSessions()
    fireEvent.dragStart(rowItem('Beta Debug'))
    fireEvent.drop(rowItem('Alpha Chat'))
    const stored = JSON.parse(window.localStorage.getItem('shannon-sessions-order')!)
    expect(stored).toEqual({ s2: 0, s1: 1, s3: 2 })
    cleanup()
    await renderWithSessions()
    expect(within(screen.getAllByRole('listitem')[0]).getByText('Beta Debug')).toBeInTheDocument()
  })

  it('delete via ⋯ menu asks for confirmation, then calls deleteSession', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    await openMenu('Beta Debug')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }))
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText('Delete Session')).toBeInTheDocument()
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }))
    await waitFor(() => expect(api.deleteSession).toHaveBeenCalledWith('s2'))
  })

  it('delete confirmation dialog can be cancelled', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    await openMenu('Beta Debug')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete' }))
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(api.deleteSession).not.toHaveBeenCalled()
  })

  it('export via ⋯ menu opens save dialog and writes the file', async () => {
    const api = await import('@/lib/tauri-api')
    const dialog = await import('@tauri-apps/plugin-dialog')
    vi.mocked(api.exportSession).mockResolvedValueOnce('# Title\n\nbody')
    vi.mocked(dialog.save).mockResolvedValueOnce('/tmp/sess.md')
    await renderWithSessions()
    await openMenu('Alpha Chat')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Export' }))
    await waitFor(() => {
      expect(api.exportSession).toHaveBeenCalledWith('s1', 'markdown')
      expect(api.saveTextFile).toHaveBeenCalledWith('/tmp/sess.md', '# Title\n\nbody')
    })
  })

  it('print via ⋯ menu opens a new window with the transcript', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.exportSession).mockResolvedValueOnce('# Title')
    const fakeDoc: any = {
      title: '',
      head: { appendChild: vi.fn() },
      body: { appendChild: vi.fn() },
      createElement: vi.fn(() => ({ textContent: '', appendChild: vi.fn() })),
    }
    const fakeWin: any = { document: fakeDoc, focus: vi.fn(), print: vi.fn() }
    const spy = vi.spyOn(window, 'open').mockReturnValueOnce(fakeWin)
    await renderWithSessions()
    await openMenu('Alpha Chat')
    fireEvent.click(screen.getByRole('menuitem', { name: 'Print / PDF' }))
    await waitFor(() => {
      expect(api.exportSession).toHaveBeenCalledWith('s1', 'markdown')
      expect(spy).toHaveBeenCalledWith('', '_blank', 'width=900,height=700')
    })
    spy.mockRestore()
  })
})

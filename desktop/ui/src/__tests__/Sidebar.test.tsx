import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup, within, act } from '@testing-library/react'
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

  it('renders OPC as a direct link in the Experiments group (U6 flatten)', () => {
    render(wrap(<Sidebar />))
    // U6 flattened the OPC disclosure (it held a single sub-link); the link
    // now lives directly inside the Experiments group.
    expect(screen.queryByText('One Person Company')).not.toBeInTheDocument()
    const opc = screen.getByText('OPC').closest('a')
    expect(opc).toHaveAttribute('href', '/opc')
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
    // Row buttons carry aria-label "Chat: {title}" (U4 terminology).
    return screen.getByRole('button', { name: `Chat: ${title}` })
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
      expect(screen.getByRole('button', { name: `Chat: Session ${i}` })).toBeInTheDocument()
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
    fireEvent.change(screen.getByLabelText('Search chats'), { target: { value: 'be' } })
    expect(screen.getByRole('button', { name: 'Chat: Beta Debug' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Chat: Alpha Chat' })).not.toBeInTheDocument()
  })

  it('calls backend search after debounce for queries ≥ 3 chars and keeps rail order', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    // s3 is a content match — its title doesn't contain the query.
    vi.mocked(api.searchSessions).mockResolvedValue([{ ...mockSessions[2] }] as any)
    fireEvent.change(screen.getByLabelText('Search chats'), { target: { value: 'use' } })
    await waitFor(() => expect(api.searchSessions).toHaveBeenCalledWith('use'))
    await waitFor(() => {
      expect(screen.getByRole('button', { name: 'Chat: Gamma Plan' })).toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Chat: Alpha Chat' })).not.toBeInTheDocument()
      expect(screen.queryByRole('button', { name: 'Chat: Beta Debug' })).not.toBeInTheDocument()
    })
  })

  it('does not call backend when query is shorter than 3 chars', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    fireEvent.change(screen.getByLabelText('Search chats'), { target: { value: 'go' } })
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
    expect(within(dialog).getByText('Delete Chat')).toBeInTheDocument()
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

describe('Sidebar — Sessions rail a11y (U5)', () => {
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
    return screen.getByRole('button', { name: `Chat: ${title}` })
  }

  function rowItem(title: string) {
    return row(title).closest('[role="listitem"]') as HTMLElement
  }

  it('activates a row with the keyboard (Space)', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.switchSession).mockResolvedValue([] as any)
    await renderWithSessions()
    row('Alpha Chat').focus()
    await userEvent.setup().keyboard('[Space]')
    await waitFor(() => expect(api.switchSession).toHaveBeenCalledWith('s1'))
  })

  it('Alt+ArrowDown moves the focused row down and persists the order', async () => {
    await renderWithSessions()
    // Default order: Alpha(0) Beta(1) Gamma(2)
    row('Alpha Chat').focus()
    fireEvent.keyDown(row('Alpha Chat'), { key: 'ArrowDown', altKey: true })
    const stored = JSON.parse(window.localStorage.getItem('shannon-sessions-order')!)
    expect(stored).toEqual({ s2: 0, s1: 1, s3: 2 })
    expect(within(screen.getAllByRole('listitem')[0]).getByText('Beta Debug')).toBeInTheDocument()
  })

  it('Alt+ArrowUp moves the focused row up', async () => {
    await renderWithSessions()
    row('Gamma Plan').focus()
    fireEvent.keyDown(row('Gamma Plan'), { key: 'ArrowUp', altKey: true })
    const stored = JSON.parse(window.localStorage.getItem('shannon-sessions-order')!)
    expect(stored).toEqual({ s1: 0, s3: 1, s2: 2 })
    expect(within(screen.getAllByRole('listitem')[1]).getByText('Gamma Plan')).toBeInTheDocument()
  })

  it('Alt+Arrow does nothing on the edge rows or mid-search', async () => {
    await renderWithSessions()
    // Top edge: no state change, no persistence.
    fireEvent.keyDown(row('Alpha Chat'), { key: 'ArrowUp', altKey: true })
    expect(window.localStorage.getItem('shannon-sessions-order')).toBeNull()
    // Mid-search: reorder is inert (ambiguous on a filtered subset).
    const search = screen.getByLabelText('Search chats')
    fireEvent.change(search, { target: { value: 'be' } })
    fireEvent.keyDown(row('Beta Debug'), { key: 'ArrowUp', altKey: true })
    expect(window.localStorage.getItem('shannon-sessions-order')).toBeNull()
  })

  it('hides the drag grip until hover/focus (visual noise)', async () => {
    await renderWithSessions()
    const grip = row('Alpha Chat').querySelector('.material-symbols-outlined')!
    expect(grip.textContent).toBe('drag_indicator')
    expect(grip.className).toContain('opacity-0')
    expect(grip.className).toContain('group-hover:opacity-100')
    expect(grip.className).toContain('group-focus-within:opacity-100')
  })

  it('⋯ menu is fully keyboard-operable (open, arrows, escape)', async () => {
    await renderWithSessions()
    const menuBtn = screen.getByRole('button', { name: 'Actions for Alpha Chat' })
    menuBtn.focus()
    await userEvent.setup().keyboard('{Enter}')
    const menu = await screen.findByRole('menu')
    // First enabled item receives focus on open.
    await waitFor(() => expect(screen.getByRole('menuitem', { name: 'Rename' })).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}')
    await waitFor(() => expect(screen.getByRole('menuitem', { name: 'Pin' })).toHaveFocus())
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(screen.queryByRole('menu')).not.toBeInTheDocument())
    expect(menu).not.toBeInTheDocument()
  })

  it('long-press (touch) opens the ⋯ menu and swallows the follow-up click', async () => {
    const api = await import('@/lib/tauri-api')
    await renderWithSessions()
    vi.useFakeTimers()
    try {
      fireEvent.touchStart(rowItem('Beta Debug'))
      // TouchMove cancels the pending press.
      fireEvent.touchMove(rowItem('Beta Debug'))
      fireEvent.touchEnd(rowItem('Beta Debug'))
      act(() => { vi.advanceTimersByTime(600) })
      expect(screen.queryByRole('menu')).not.toBeInTheDocument()
      // A press that lasts 500ms opens the menu.
      fireEvent.touchStart(rowItem('Beta Debug'))
      act(() => { vi.advanceTimersByTime(500) })
      expect(screen.getByRole('menu')).toBeInTheDocument()
      fireEvent.touchEnd(rowItem('Beta Debug'))
      // The synthetic click after a completed long-press must not switch.
      fireEvent.click(row('Beta Debug'))
      expect(api.switchSession).not.toHaveBeenCalled()
    } finally {
      vi.useRealTimers()
    }
  })
})

describe('Sidebar — resize handle (U5)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    document.documentElement.style.removeProperty('--sidebar-w')
    vi.clearAllMocks()
  })

  function handle() {
    return screen.getByRole('separator', { name: 'Resize sidebar' })
  }

  it('is a focusable separator with width semantics', () => {
    render(wrap(<Sidebar />))
    const h = handle()
    expect(h).toHaveAttribute('tabindex', '0')
    expect(h).toHaveAttribute('aria-valuenow', '280')
    expect(h).toHaveAttribute('aria-valuemin', '200')
    expect(h).toHaveAttribute('aria-valuemax', '400')
  })

  it('hot zone is 8px wide (visual bar stays 4px)', () => {
    render(wrap(<Sidebar />))
    const h = handle()
    expect(h.className).toContain('w-2')
    expect(h.firstElementChild!.className).toContain('w-1')
  })

  it('arrow keys resize and persist the width', async () => {
    render(wrap(<Sidebar />))
    handle().focus()
    await userEvent.setup().keyboard('{ArrowRight}')
    await waitFor(() => expect(handle()).toHaveAttribute('aria-valuenow', '296'))
    expect(document.documentElement.style.getPropertyValue('--sidebar-w')).toBe('296px')
    expect(window.localStorage.getItem('shannon-sidebar-width')).toBe('296')
    // Clamped at the max.
    await userEvent.keyboard('{ArrowLeft>20}')
    await waitFor(() => expect(handle()).toHaveAttribute('aria-valuenow', '200'))
  })

  it('double-click resets to the default 280px', async () => {
    render(wrap(<Sidebar />))
    handle().focus()
    await userEvent.setup().keyboard('{ArrowRight>3}')
    fireEvent.dblClick(handle())
    await waitFor(() => expect(handle()).toHaveAttribute('aria-valuenow', '280'))
    expect(window.localStorage.getItem('shannon-sidebar-width')).toBe('280')
  })
})

describe('Sidebar — nav IA groups (U6)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('groups nav: Work visible, Resources folded in Simple mode', () => {
    render(wrap(<Sidebar />))
    expect(screen.getByRole('button', { name: /Work/ })).toHaveAttribute('aria-expanded', 'true')
    const resources = screen.getByRole('button', { name: /Resources/ })
    expect(resources).toHaveAttribute('aria-expanded', 'false')
    // Work group is open: Chat / Scheduled / Triage visible.
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Scheduled')).toBeInTheDocument()
    expect(screen.getByText('Triage')).toBeInTheDocument()
    // Resources folded: Memory / Usage hidden; Extensions stays a flat entry.
    expect(screen.queryByText('Memory')).not.toBeInTheDocument()
    expect(screen.queryByText('Usage')).not.toBeInTheDocument()
    expect(screen.getByText('Extensions')).toBeInTheDocument()
  })

  it('expanding Resources reveals Memory and Usage', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Resources/ }))
    expect(screen.getByText('Memory')).toBeInTheDocument()
    expect(screen.getByText('Usage')).toBeInTheDocument()
  })

  it('collapsing the Work group hides Chat/Scheduled', () => {
    render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Work/ }))
    expect(screen.queryByText('Chat')).not.toBeInTheDocument()
    expect(screen.queryByText('Scheduled')).not.toBeInTheDocument()
  })

  it('Experiments group is dev-only and holds the OPC link', () => {
    render(wrap(<Sidebar />))
    expect(screen.queryByRole('button', { name: /Experiments/ })).not.toBeInTheDocument()
    window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
    const { unmount } = render(wrap(<Sidebar />))
    expect(screen.getByRole('button', { name: /Experiments/ })).toBeInTheDocument()
    expect(screen.getByText('OPC')).toBeInTheDocument()
    unmount()
  })

  it('persists group + settings expansion to shannon-nav-open across remounts', () => {
    const { unmount } = render(wrap(<Sidebar />))
    fireEvent.click(screen.getByRole('button', { name: /Resources/ }))
    fireEvent.click(screen.getByText('Settings'))
    const stored = JSON.parse(window.localStorage.getItem('shannon-nav-open')!)
    expect(stored).toMatchObject({ resources: true, settings: true, work: true })
    unmount()
    render(wrap(<Sidebar />))
    // Remounted: Resources still open, Settings still expanded.
    expect(screen.getByText('Memory')).toBeInTheDocument()
    expect(screen.getByText('General')).toBeInTheDocument()
    // Collapse Work, remount: still collapsed.
    fireEvent.click(screen.getByRole('button', { name: /Work/ }))
    cleanup()
    render(wrap(<Sidebar />))
    expect(screen.queryByText('Chat')).not.toBeInTheDocument()
  })
})

describe('Sidebar — zero-session guide card (U7)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.clearAllMocks()
  })

  it('renders the guide card instead of a blank rail when there are no sessions', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.listSessions).mockResolvedValue([] as any)
    render(wrap(<Sidebar />))
    expect(await screen.findByText('Start your first chat')).toBeInTheDocument()
    expect(screen.getByText('Pick a starter below — your chats will live here.')).toBeInTheDocument()
    // Two starter prompts, sharing copy with WelcomeState's example cards.
    expect(screen.getByRole('button', { name: /Draft an email/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^Summarize/ })).toBeInTheDocument()
    // No research/code starters in the compact rail (dedupe with WelcomeState).
    expect(screen.queryByRole('button', { name: /Research/ })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Write code/ })).not.toBeInTheDocument()
    // The session rail controls (search box) don't render on an empty rail.
    expect(screen.queryByLabelText('Search chats')).not.toBeInTheDocument()
  })

  it('starter prompt creates the first session and prefills the composer via /chat state', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.listSessions).mockResolvedValue([] as any)
    vi.mocked(api.newSession).mockResolvedValue('fresh-id' as any)
    // LocationProbe captures pathname + state after the suggestion click.
    function LocationProbe() {
      const location = useLocation()
      return <div data-testid="probe" data-path={location.pathname} data-prefill={(location.state as { prefill?: string } | null)?.prefill ?? ''} />
    }
    render(wrap(<div><Sidebar /><LocationProbe /></div>, { path: '/triage' }))
    fireEvent.click(await screen.findByRole('button', { name: /Draft an email/ }))
    await waitFor(() => expect(api.newSession).toHaveBeenCalledTimes(1))
    await waitFor(() => {
      const probe = screen.getByTestId('probe')
      expect(probe.getAttribute('data-path')).toBe('/chat')
      expect(probe.getAttribute('data-prefill')).toMatch(/follow-up email/i)
    })
  })

  it('keeps the session rail once a session exists', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.listSessions).mockResolvedValue([
      { id: 's1', title: 'Alpha Chat', created_at: Date.now(), message_count: 0 },
    ] as any)
    render(wrap(<Sidebar />))
    expect(await screen.findByRole('button', { name: 'Chat: Alpha Chat' })).toBeInTheDocument()
    expect(screen.queryByText('Start your first chat')).not.toBeInTheDocument()
  })
})

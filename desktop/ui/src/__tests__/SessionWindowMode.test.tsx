// P1-1 — session multi-window UI behavior:
// - window mode (`/?windowSession=<uuid>`): sidebar hidden, content spans
//   full width, native title follows the session title;
// - main window (no param): completely unchanged (sidebar stays);
// - the session rail's「Open in New Window」menu entry dispatches
//   `openSessionWindow` with the session id.
//
// Mirrors SessionsPanel.test.tsx: mount the real AppProvider and mock only
// the tauri-api layer.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { AppProvider } from '@/context/AppContext'
import { Layout } from '@/components/Layout'
import { Sidebar } from '@/components/Sidebar'
import { getCurrentWindow } from '@tauri-apps/api/window'
import * as api from '@/lib/tauri-api'

const UUID_A = '7e6c3f18-4a2e-4f6a-9a52-6d1c1a0f83f1'

const listSessions = vi.mocked(api.listSessions)
const switchSession = vi.mocked(api.switchSession)
const openSessionWindow = vi.mocked(api.openSessionWindow)

function setUrlSearch(search: string) {
  window.history.replaceState(null, '', `/${search}`)
}

function renderLayout() {
  return render(
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>
          <Layout />
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>,
  )
}

function renderSidebar() {
  return render(
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>
          <Sidebar />
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  setUrlSearch('')
  vi.spyOn(console, 'warn').mockImplementation(() => undefined)
  listSessions.mockResolvedValue([
    {
      id: UUID_A,
      title: 'Fixture session',
      created_at: 1,
      message_count: 0,
    },
  ])
  switchSession.mockResolvedValue([])
  openSessionWindow.mockResolvedValue({ label: `session-${UUID_A}`, sessionId: UUID_A })
})

describe('Layout — window mode', () => {
  it('hides the sidebar and pins --sidebar-w to 0px', async () => {
    setUrlSearch(`?windowSession=${UUID_A}`)
    renderLayout()
    await waitFor(() => {
      expect(switchSession).toHaveBeenCalledWith(UUID_A)
    })
    expect(document.querySelector('[data-sidebar]')).toBeNull()
    expect(
      document.documentElement.style.getPropertyValue('--sidebar-w'),
    ).toBe('0px')
  })

  it('syncs the native window title with the session title', async () => {
    setUrlSearch(`?windowSession=${UUID_A}`)
    const setTitle = getCurrentWindow().setTitle
    vi.mocked(setTitle).mockClear()
    renderLayout()
    await waitFor(() => {
      expect(setTitle).toHaveBeenCalledWith('Fixture session')
    })
  })

  it('keeps the main window unchanged: sidebar stays, no window-mode chrome', async () => {
    setUrlSearch('')
    renderLayout()
    await waitFor(() => {
      expect(document.querySelector('[data-sidebar]')).not.toBeNull()
    })
    expect(screen.queryByTitle('This window is pinned to a single session')).toBeNull()
    expect(switchSession).not.toHaveBeenCalled()
  })
})

describe('session rail —「Open in New Window」menu entry', () => {
  it('dispatches openSessionWindow with the session id', async () => {
    renderSidebar()
    // Open the per-session ⋯ menu.
    const menuButton = await screen.findByRole('button', {
      name: 'Actions for Fixture session',
    })
    fireEvent.click(menuButton)
    const item = await screen.findByRole('menuitem', { name: /Open in New Window/i })
    fireEvent.click(item)
    await waitFor(() => {
      expect(openSessionWindow).toHaveBeenCalledWith(UUID_A)
    })
  })
})

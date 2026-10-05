// Settings R3 T7 — the rail's pin backend-ification: pin/unpin through
// set_session_pinned (the curation sidecar is the source of truth; the rail
// re-derives its sort/glyph from the list DTO's `pinned` flag), plus the
// one-time migration of the legacy localStorage pin list (read once →
// set_session_pinned per id → key removed on full success, kept on failure
// so the next mount retries). Backend mocked at the tauri-api seam (same
// pattern as sidebarArchived.test.tsx).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type { SessionInfo } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listArchivedSessions: vi.fn(async () => []),
  archiveSession: vi.fn(async () => true),
  unarchiveSession: vi.fn(async () => true),
  setSessionPinned: vi.fn(async () => true),
  searchSessions: vi.fn(async () => []),
  listScheduledTasks: vi.fn(async () => []),
  openSessionWindow: vi.fn(async () => undefined),
}))

vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

const NOW = Date.now()

function session(over: Partial<SessionInfo> = {}): SessionInfo {
  return { id: 's1', title: 'Session One', created_at: NOW - 3600_000, message_count: 2, ...over }
}

function renderRail(sessions: SessionInfo[] = [session()]) {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={sessions}
          sessionActivity={{}}
          currentSessionId={null}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={async () => {}}
        />
      </MemoryRouter>
    </I18nProvider>,
  )
}

function rowItem(title: string) {
  return screen.getByRole('button', { name: `Chat: ${title}` }).closest('[role="listitem"]') as HTMLElement
}

beforeEach(() => {
  vi.clearAllMocks()
  window.localStorage.clear()
})

describe('pin through the backend (Settings R3 T7)', () => {
  it('calls set_session_pinned with the requested state and never writes localStorage', async () => {
    renderRail([session(), session({ id: 's2', title: 'Session Two', created_at: NOW - 100 })])
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Session One' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Pin' }))
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('s1', true))
    expect(window.localStorage.getItem('shannon-sessions-pinned')).toBeNull()
    // Optimistic glyph flips on without a refresh, and the menu flips to Unpin.
    await waitFor(() => expect(rowItem('Session One').textContent).toContain('push_pin'))
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Session One' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Unpin' }))
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('s1', false))
  })

  it('sorts pinned DTO rows first and reverts the optimistic glyph when the call fails', async () => {
    // A pinned row sorts ahead of a newer unpinned one…
    renderRail([
      session({ id: 'new', title: 'Newest', created_at: NOW - 10 }),
      session({ id: 'old', title: 'Old but pinned', created_at: NOW - 5000, pinned: true }),
    ])
    await waitFor(() => expect(rowItem('Old but pinned').textContent).toContain('push_pin'))
    expect(screen.getAllByRole('listitem')[0].textContent).toContain('Old but pinned')
    cleanup()

    // …and a failed pin call reverts instead of leaving the rail lying.
    vi.mocked(api.setSessionPinned).mockRejectedValueOnce(new Error('backend refused'))
    renderRail([session({ id: 'new', title: 'Newest', created_at: NOW - 10 })])
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Newest' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Pin' }))
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('new', true))
    await waitFor(() => expect(rowItem('Newest').textContent).not.toContain('push_pin'))
    cleanup()
  })
})

describe('legacy localStorage pin migration (one-time)', () => {
  it('migrates each legacy id through set_session_pinned and retires the key on full success', async () => {
    window.localStorage.setItem('shannon-sessions-pinned', JSON.stringify(['a', 'b']))
    renderRail()
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('a', true))
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('b', true))
    await waitFor(() =>
      expect(window.localStorage.getItem('shannon-sessions-pinned')).toBeNull(),
    )
    cleanup()
  })

  it('keeps the key for a retry when any migration call fails (never silently drops pins)', async () => {
    window.localStorage.setItem('shannon-sessions-pinned', JSON.stringify(['ok-1', 'bad-2']))
    vi.mocked(api.setSessionPinned).mockImplementation(async (id: string) => {
      if (id === 'bad-2') throw new Error('backend refused')
      return true
    })
    renderRail()
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('bad-2', true))
    // Give the migration's cleanup microtasks a chance to (not) run.
    await new Promise(r => setTimeout(r, 20))
    expect(window.localStorage.getItem('shannon-sessions-pinned')).toEqual(
      JSON.stringify(['ok-1', 'bad-2']),
    )
    cleanup()
    // And a later mount retries the whole list.
    vi.mocked(api.setSessionPinned).mockResolvedValue(true)
    renderRail()
    await waitFor(() => expect(api.setSessionPinned).toHaveBeenCalledWith('bad-2', true))
    await waitFor(() =>
      expect(window.localStorage.getItem('shannon-sessions-pinned')).toBeNull(),
    )
    cleanup()
  })

  it('leaves everything untouched when the legacy key was never written', async () => {
    renderRail()
    await new Promise(r => setTimeout(r, 20))
    expect(api.setSessionPinned).not.toHaveBeenCalled()
    cleanup()
  })
})

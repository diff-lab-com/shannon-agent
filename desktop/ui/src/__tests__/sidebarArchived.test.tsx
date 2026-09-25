// 卡A session archive — the rail's 已归档 section (collapsed by default,
// lens/visual language of the 项目 folders) + the 归档 action in a row's ⋯
// menu + the 恢复 invocation. Backend is mocked at the tauri-api seam; the
// component's own archive/restore handlers and toast calls are what's
// under test.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type { SessionInfo } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listArchivedSessions: vi.fn(async () => [
    { id: 'arch-1', title: 'Old project chat', updated_at: Date.now() - 3 * 24 * 3600_000 },
    { id: 'arch-2', title: null, updated_at: Date.now() - 3600_000 },
  ]),
  archiveSession: vi.fn(async () => true),
  unarchiveSession: vi.fn(async () => true),
  searchSessions: vi.fn(async () => []),
  listScheduledTasks: vi.fn(async () => []),
  openSessionWindow: vi.fn(async () => undefined),
}))

// sonner is called for the success toasts — assert the invocation, keep the
// error path out of scope (toastError already covered elsewhere).
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

beforeEach(() => {
  vi.clearAllMocks()
})

describe('已归档 section (卡A)', () => {
  it('renders collapsed with a count and expands to archived rows', async () => {
    renderRail()
    // The lens loads (defensively) and shows the collapsed toggle with the
    // archived count.
    await waitFor(() => expect(api.listArchivedSessions).toHaveBeenCalled())
    const toggle = screen.getByTestId('sidebar-archived-toggle')
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    expect(screen.getByText('Archived')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Restore Old project chat/ })).not.toBeInTheDocument()

    fireEvent.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByTestId('archived-row-arch-1')).toBeInTheDocument()
    expect(screen.getByTestId('archived-row-arch-2')).toBeInTheDocument()
    // Untitled rows degrade to the untitled placeholder.
    expect(screen.getByRole('button', { name: /Restore Untitled$/ })).toBeInTheDocument()
  })

  it('keeps the section hidden while searching', async () => {
    renderRail()
    await waitFor(() => expect(api.listArchivedSessions).toHaveBeenCalled())
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'Ses' } })
    await waitFor(() => expect(screen.queryByTestId('sidebar-archived-section')).not.toBeInTheDocument())
  })

  it('keeps the section hidden when nothing is archived', () => {
    vi.mocked(api.listArchivedSessions).mockResolvedValueOnce([])
    renderRail()
    expect(screen.queryByTestId('sidebar-archived-section')).not.toBeInTheDocument()
  })

  it('restores an archived session through the backend', async () => {
    renderRail()
    await waitFor(() => expect(api.listArchivedSessions).toHaveBeenCalled())
    fireEvent.click(screen.getByTestId('sidebar-archived-toggle'))
    fireEvent.click(screen.getByTestId('archived-restore-arch-1'))
    await waitFor(() => expect(api.unarchiveSession).toHaveBeenCalledWith('arch-1'))
    expect(vi.mocked(api.archiveSession)).not.toHaveBeenCalled()
  })

  it('offers 归档 in the ⋯ menu and calls the backend', async () => {
    renderRail()
    await waitFor(() => expect(api.listArchivedSessions).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Session One' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Archive' }))
    await waitFor(() => expect(api.archiveSession).toHaveBeenCalledWith('s1'))
    expect(api.unarchiveSession).not.toHaveBeenCalled()
  })
})

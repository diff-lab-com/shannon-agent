// B4 P2-6/P2-7 — the delete dialog's named target + pending state, the
// archived section's 永久删除 action, close-on-success / stay-open-on-failure,
// and the stale pin/order map pruning on real delete. Backend mocked at the
// tauri-api seam (same pattern as sidebarArchived.test.tsx).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import type { SessionInfo } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listArchivedSessions: vi.fn(async () => []),
  archiveSession: vi.fn(async () => true),
  unarchiveSession: vi.fn(async () => true),
  searchSessions: vi.fn(async () => []),
  listScheduledTasks: vi.fn(async () => []),
  openSessionWindow: vi.fn(async () => undefined),
}))

vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

const NOW = Date.now()

function session(over: Partial<SessionInfo> = {}): SessionInfo {
  return { id: 's1', title: 'Session One', created_at: NOW - 3600_000, message_count: 2, ...over }
}

function sessions(over: Partial<SessionInfo> = {}): SessionInfo[] {
  return [
    session({ id: 's1', title: 'Alpha', created_at: NOW - 3000, ...over }),
    session({ id: 's2', title: 'Beta', created_at: NOW - 2000 }),
    session({ id: 's3', title: 'Gamma', created_at: NOW - 1000 }),
  ]
}

type DeleteSpy = (id: string) => Promise<void>

function renderRail(opts: {
  list?: SessionInfo[]
  deleteSession?: DeleteSpy
  currentSessionId?: string | null
} = {}) {
  const onDelete = opts.deleteSession ?? (async () => {})
  const currentSessionId = opts.currentSessionId ?? null
  const utils = render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={opts.list ?? sessions()}
          sessionActivity={{}}
          currentSessionId={currentSessionId}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={onDelete}
        />
      </MemoryRouter>
    </I18nProvider>,
  )
  return { ...utils, rerenderWith: (list: SessionInfo[]) => {
    utils.rerender(
      <I18nProvider>
        <MemoryRouter>
          <SessionsSection
            sessions={list}
            sessionActivity={{}}
            currentSessionId={currentSessionId}
            switchSession={async () => {}}
            renameSession={async () => {}}
            deleteSession={onDelete}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
  } }
}

async function openDeleteDialog(title: string) {
  fireEvent.click(screen.getByRole('button', { name: `Actions for ${title}` }))
  fireEvent.click(await screen.findByRole('menuitem', { name: 'Delete' }))
  return screen.findByRole('alertdialog')
}

beforeEach(() => {
  window.localStorage.clear()
  vi.clearAllMocks()
})

describe('delete dialog (B4 P2-6)', () => {
  it('names the target session in the confirm copy', async () => {
    renderRail()
    const dialog = await openDeleteDialog('Beta')
    // Old copy was an anonymous "Are you sure you want to delete this chat?".
    expect(dialog.textContent).toContain('Delete “Beta”?')
    expect(dialog.textContent).not.toContain('this chat?')
  })

  it('keeps the dialog open with a pending state while the delete runs, closes when the row leaves the list', async () => {
    let release!: () => void
    const gate = new Promise<void>(resolve => { release = resolve })
    const list = sessions()
    const { rerenderWith } = renderRail({
      deleteSession: async () => { await gate },
    })
    const dialog = await openDeleteDialog('Beta')
    const confirm = dialog.querySelector('[data-testid="delete-session-confirm"]') as HTMLButtonElement
    fireEvent.click(confirm)
    // In flight: confirm disabled with the working label, cancel locked.
    await waitFor(() => expect(confirm).toBeDisabled())
    expect(confirm.textContent).toBe('Deleting…')
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled()
    release()
    // Success surfaces as the row disappearing from the refreshed list.
    rerenderWith(list.filter(s => s.id !== 's2'))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('keeps the dialog open when the delete fails (row still present)', async () => {
    renderRail({
      deleteSession: async () => { throw new Error('backend refused') },
    })
    const dialog = await openDeleteDialog('Beta')
    fireEvent.click(dialog.querySelector('[data-testid="delete-session-confirm"]') as HTMLButtonElement)
    // The handler swallows the rejection (error funnels to the shared
    // banner); the dialog must not silently close.
    await waitFor(() => expect(screen.getByRole('button', { name: 'Cancel' })).toBeEnabled())
    expect(screen.getByRole('alertdialog')).toBeInTheDocument()
  })
})

describe('archived 永久删除 (B4 P2-6)', () => {
  it('offers a permanent delete with its own confirm copy and calls deleteSession', async () => {
    const api = await import('@/lib/tauri-api')
    vi.mocked(api.listArchivedSessions).mockResolvedValue([
      { id: 'arch-1', title: 'Old project chat', updated_at: NOW - 86_400_000 },
    ] as any)
    const onDelete = vi.fn(async () => {})
    const list = sessions()
    const { rerenderWith } = renderRail({ deleteSession: onDelete, list })
    // The archived lens loads async — wait for the section to appear.
    fireEvent.click(await screen.findByTestId('sidebar-archived-toggle'))
    fireEvent.click(screen.getByTestId('archived-delete-arch-1'))
    const dialog = await screen.findByRole('alertdialog')
    expect(dialog.textContent).toContain('Permanently delete “Old project chat”?')
    fireEvent.click(dialog.querySelector('[data-testid="delete-session-confirm"]') as HTMLButtonElement)
    await waitFor(() => expect(onDelete).toHaveBeenCalledWith('arch-1'))
    // The archived lens refreshes (session list changed) and no longer has
    // the row → the dialog closes.
    vi.mocked(api.listArchivedSessions).mockResolvedValue([] as any)
    rerenderWith([...list])
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })
})

describe('stale map pruning (B4 P2-7)', () => {
  it('prunes deleted sessions from shannon-sessions-order and shannon-sessions-pinned', async () => {
    window.localStorage.setItem('shannon-sessions-pinned', JSON.stringify(['s1', 's2']))
    window.localStorage.setItem('shannon-sessions-order', JSON.stringify({ s1: 0, s2: 1, s3: 2 }))
    const list = sessions()
    const { rerenderWith } = renderRail()
    const dialog = await openDeleteDialog('Beta')
    fireEvent.click(dialog.querySelector('[data-testid="delete-session-confirm"]') as HTMLButtonElement)
    rerenderWith(list.filter(s => s.id !== 's2'))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(JSON.parse(window.localStorage.getItem('shannon-sessions-pinned')!)).toEqual(['s1'])
    expect(JSON.parse(window.localStorage.getItem('shannon-sessions-order')!)).toEqual({ s1: 0, s3: 2 })
  })

  it('leaves pin/order entries alone on archive (reversible; inert while archived)', async () => {
    const api = await import('@/lib/tauri-api')
    window.localStorage.setItem('shannon-sessions-pinned', JSON.stringify(['s1']))
    window.localStorage.setItem('shannon-sessions-order', JSON.stringify({ s1: 0 }))
    renderRail()
    fireEvent.click(screen.getByRole('button', { name: 'Actions for Alpha' }))
    fireEvent.click(await screen.findByRole('menuitem', { name: 'Archive' }))
    await waitFor(() => expect(api.archiveSession).toHaveBeenCalledWith('s1'))
    expect(JSON.parse(window.localStorage.getItem('shannon-sessions-pinned')!)).toEqual(['s1'])
    expect(JSON.parse(window.localStorage.getItem('shannon-sessions-order')!)).toEqual({ s1: 0 })
  })
})

describe('incremental rendering (B4 P2-4)', () => {
  it('caps a long flat list at 50 rows with a 显示全部 expander, always showing the active session', () => {
    const many: SessionInfo[] = Array.from({ length: 120 }, (_, i) =>
      session({ id: `s${i}`, title: `Session ${i}`, created_at: NOW - i }))
    renderRail({ list: many, currentSessionId: 's119' })
    // Newest 50 + the active session (deep in the list) are visible.
    expect(screen.getAllByRole('listitem').length).toBeLessThan(120)
    expect(screen.getByRole('button', { name: 'Chat: Session 0' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Chat: Session 119' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Chat: Session 100' })).not.toBeInTheDocument()
    expect(screen.getByTestId('sidebar-show-all').textContent).toContain('120')
  })

  it('expands to the full list when 显示全部 is clicked', () => {
    const many: SessionInfo[] = Array.from({ length: 120 }, (_, i) =>
      session({ id: `s${i}`, title: `Session ${i}`, created_at: NOW - i }))
    renderRail({ list: many })
    fireEvent.click(screen.getByTestId('sidebar-show-all'))
    expect(screen.getAllByRole('listitem').length).toBe(120)
  })
})

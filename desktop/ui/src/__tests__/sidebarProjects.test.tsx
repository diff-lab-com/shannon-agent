// P-U1/P-U2 — the rail's project tree (Task 3):
//  - P-U1: routines with a working_dir nest into their project group, mixed
//    with sessions (routine recency = next_fire_at); the standalone 自动化
//    section lists only unhoused routines and hides when empty
//  - P-U1: the grouping key is the FULL working dir (trailing-slash
//    normalized), so /w/x and /h/x are different projects
//  - P-U2: registry-only projects render a non-interactive 暂无任务 row;
//    the header ⋯ menu drives new-session/routine, open-folder, rename,
//    color and archive; archived projects collapse at the bottom with 恢复
//  - the legacy localStorage `shannon-projects` registry migrates into the
//    engine registry once, then the key is removed

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type { ProjectRecord, ScheduledRoutine, SessionInfo } from '@/types'

const NOW = Date.now()

const fixtures = vi.hoisted(() => ({
  routines: [] as ScheduledRoutine[],
  registry: [] as ProjectRecord[],
}))

function projectRecord(path: string, over: Partial<ProjectRecord> = {}): ProjectRecord {
  return { path, name: null, icon: null, color: null, archivedAtMs: null, createdAtMs: 0, ...over }
}

vi.mock('@/lib/tauri-api', () => ({
  listScheduledTasks: vi.fn(async () => fixtures.routines),
  searchSessions: vi.fn(async () => []),
  listArchivedSessions: vi.fn(async () => []),
  listProjects: vi.fn(async () => fixtures.registry),
  renameProject: vi.fn(async (path: string, name: string | null) => {
    const record = projectRecord(path, { name })
    fixtures.registry = fixtures.registry.filter(p => p.path !== path).concat(record)
    return record
  }),
  setProjectAppearance: vi.fn(async (path: string, icon: string | null, color: string | null) => {
    const record = projectRecord(path, { icon, color })
    fixtures.registry = fixtures.registry.filter(p => p.path !== path).concat(record)
    return record
  }),
  archiveProject: vi.fn(async (path: string) => {
    const prev = fixtures.registry.find(p => p.path === path) ?? projectRecord(path)
    const record = { ...prev, archivedAtMs: Date.now() }
    fixtures.registry = fixtures.registry.filter(p => p.path !== path).concat(record)
    return record
  }),
  unarchiveProject: vi.fn(async (path: string) => {
    const prev = fixtures.registry.find(p => p.path === path) ?? projectRecord(path)
    const record = { ...prev, archivedAtMs: null }
    fixtures.registry = fixtures.registry.filter(p => p.path !== path).concat(record)
    return record
  }),
  registerProject: vi.fn(async (path: string) => projectRecord(path)),
  newSession: vi.fn(async () => 'new-1'),
  setSessionWorkingDir: vi.fn(async () => undefined),
  revealInFolder: vi.fn(async () => undefined),
  openWithDefaultApp: vi.fn(async () => undefined),
}))

// Assert the success toasts without touching real sonner portals.
vi.mock('sonner', () => ({ toast: { success: vi.fn(), error: vi.fn() } }))

function session(id: string, over: Partial<SessionInfo> = {}): SessionInfo {
  return {
    id,
    title: `S-${id}`,
    created_at: NOW - 3600_000,
    updated_at: NOW - 3600_000,
    message_count: 1,
    working_dir: '/w/alpha',
    ...over,
  }
}

function LocationProbe() {
  const loc = useLocation()
  return <div data-testid="location-probe">{loc.pathname}</div>
}

function renderRail(sessions: SessionInfo[], grouping: 'project' | 'session' | 'smart' = 'project') {
  window.localStorage.setItem('shannon-sessions-grouping', grouping)
  return render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={sessions}
          sessionActivity={{}}
          currentSessionId={null}
          switchSession={vi.fn(async () => {})}
          renameSession={vi.fn(async () => {})}
          deleteSession={vi.fn(async () => {})}
        />
        <LocationProbe />
      </MemoryRouter>
    </I18nProvider>,
  )
}

/** Menu tests need an actual project group: a lone session in one dir stays
 *  flat unless the registry already knows the project. */
function renderProjectRail() {
  fixtures.registry = [projectRecord('/w/alpha')]
  return renderRail([session('s1')])
}

beforeEach(() => {
  window.localStorage.clear()
  fixtures.routines = []
  fixtures.registry = []
  vi.clearAllMocks()
})

describe('P-U1 nesting', () => {
  it('nests a housed routine into its project group ahead of older sessions', async () => {
    fixtures.routines = [
      { id: 'r1', name: 'Nightly sweep', enabled: true, working_dir: '/w/alpha', next_fire_at: NOW + 60_000 } as ScheduledRoutine,
    ]
    // s2 is the more recent session (created_at desc); the imminent routine
    // sorts above both.
    renderRail([
      session('s1', { created_at: NOW - 3600_000, updated_at: NOW - 3600_000 }),
      session('s2', { created_at: NOW - 60_000, updated_at: NOW - 60_000 }),
    ])
    expect(await screen.findByTestId('sidebar-routine-row-r1')).toBeInTheDocument()
    const header = screen.getByTestId('project-header-/w/alpha')
    const routine = screen.getByTestId('sidebar-routine-row-r1')
    const newer = screen.getByTestId('desktop-session-row-s2')
    const older = screen.getByTestId('desktop-session-row-s1')
    const following = Node.DOCUMENT_POSITION_FOLLOWING
    expect(header.compareDocumentPosition(routine) & following).toBeTruthy()
    expect(routine.compareDocumentPosition(newer) & following).toBeTruthy()
    expect(newer.compareDocumentPosition(older) & following).toBeTruthy()
    // Imminent fire → the existing 即将 badge.
    expect(screen.getByRole('img', { name: 'Soon' })).toBeInTheDocument()
  })

  it('keeps /w/x and /h/x as distinct groups (full-path key, shared tail)', async () => {
    renderRail([session('s1', { working_dir: '/w/x' }), session('s2', { working_dir: '/h/x' })])
    expect(screen.getByTestId('project-header-/w/x')).toBeInTheDocument()
    expect(screen.getByTestId('project-header-/h/x')).toBeInTheDocument()
  })

  it('merges trailing-slash variants into one group and prefers the registry name', async () => {
    fixtures.registry = [projectRecord('/w/alpha', { name: 'Alpha Custom' })]
    renderRail([
      session('s1', { working_dir: '/w/alpha' }),
      session('s2', { working_dir: '/w/alpha/' }),
      session('s3', { working_dir: '/w/beta' }),
    ])
    expect(await screen.findByText('Alpha Custom')).toBeInTheDocument()
    expect(screen.getByTestId('project-header-/w/alpha')).toBeInTheDocument()
    expect(screen.getByTestId('project-header-/w/beta')).toBeInTheDocument()
  })

  it('lists only unhoused routines in the 自动化 section and hides it when none remain', async () => {
    fixtures.routines = [
      { id: 'housed', name: 'Housed job', enabled: true, working_dir: '/w/alpha', next_fire_at: NOW + 60_000 } as ScheduledRoutine,
      { id: 'free', name: 'Unhoused job', enabled: true, next_fire_at: null } as ScheduledRoutine,
    ]
    const first = renderRail([session('s1')])
    const sec = await screen.findByTestId('sidebar-automations')
    expect(sec.textContent).toContain('Unhoused job')
    expect(sec.textContent).not.toContain('Housed job')
    // The housed one still lives in the tree.
    expect(screen.getByTestId('sidebar-routine-row-housed')).toBeInTheDocument()
    first.unmount()

    // Only housed routines → the standalone section disappears entirely.
    fixtures.routines = [
      { id: 'housed', name: 'Housed job', enabled: true, working_dir: '/w/alpha', next_fire_at: NOW + 60_000 } as ScheduledRoutine,
    ]
    renderRail([session('s1')])
    await vi.waitFor(() => {
      expect(screen.queryByTestId('sidebar-automations')).not.toBeInTheDocument()
    })
    expect(await screen.findByTestId('sidebar-routine-row-housed')).toBeInTheDocument()
  })

  // I3 review fix: outside the project lens the tree is not rendered at
  // all, so the section must carry ALL enabled routines (housed ∪ unhoused)
  // — otherwise housed automations go dark in the time/smart lenses.
  it('lists housed routines too in the time/smart lenses', async () => {
    fixtures.routines = [
      { id: 'housed', name: 'Housed job', enabled: true, working_dir: '/w/alpha', next_fire_at: NOW + 60_000 } as ScheduledRoutine,
      { id: 'free', name: 'Unhoused job', enabled: true, next_fire_at: null } as ScheduledRoutine,
    ]
    for (const lens of ['session', 'smart'] as const) {
      const view = renderRail([session('s1')], lens)
      const sec = await screen.findByTestId('sidebar-automations')
      expect(sec.textContent).toContain('Housed job')
      expect(sec.textContent).toContain('Unhoused job')
      // The tree is not rendered in these lenses — exactly one row per routine.
      expect(screen.queryByTestId('sidebar-routine-row-housed')).not.toBeInTheDocument()
      view.unmount()
    }
  })
})

describe('P-U2 empty projects and tree-worthiness', () => {
  it('renders registry-only projects with a non-interactive placeholder row', async () => {
    fixtures.registry = [projectRecord('/w/gamma', { name: 'Gamma' })]
    renderRail([session('s1', { working_dir: undefined })])
    expect(await screen.findByTestId('project-header-/w/gamma')).toBeInTheDocument()
    expect(screen.getByText('Gamma')).toBeInTheDocument()
    const placeholder = screen.getByTestId('project-empty-row')
    expect(placeholder.textContent).toBe('No sessions yet')
    // Deliberately NOT focusable/interactive.
    expect(placeholder.tagName).toBe('DIV')
    expect(screen.queryByRole('button', { name: 'No sessions yet' })).not.toBeInTheDocument()
  })

  it('stays flat for a single session group when nothing else is tree-worthy', () => {
    renderRail([session('s1')])
    expect(screen.queryByTestId('project-header-/w/alpha')).not.toBeInTheDocument()
  })

  it('engages the tree for a single session group once a registry project exists', async () => {
    fixtures.registry = [projectRecord('/w/gamma', { name: 'Gamma' })]
    renderRail([session('s1')])
    expect(await screen.findByTestId('project-header-/w/alpha')).toBeInTheDocument()
    expect(screen.getByTestId('project-header-/w/gamma')).toBeInTheDocument()
  })
})

describe('P-U2 project actions menu', () => {
  it('creates a session rooted in the project and navigates to /chat', async () => {
    renderProjectRail()
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'New session in this project' }))
    await waitFor(() => expect(api.setSessionWorkingDir).toHaveBeenCalledWith('new-1', '/w/alpha'))
    expect(api.newSession).toHaveBeenCalledTimes(1)
    expect(screen.getByTestId('location-probe').textContent).toBe('/chat')
  })

  it('reveals the project directory through the opener chain', async () => {
    renderProjectRail()
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open folder' }))
    await waitFor(() => expect(api.revealInFolder).toHaveBeenCalledWith('/w/alpha'))
    expect(api.openWithDefaultApp).not.toHaveBeenCalled()
  })

  it('falls back to openWithDefaultApp when reveal is rejected', async () => {
    vi.mocked(api.revealInFolder).mockRejectedValueOnce('out of scope')
    renderProjectRail()
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open folder' }))
    await waitFor(() => expect(api.openWithDefaultApp).toHaveBeenCalledWith('/w/alpha'))
  })

  it('archives the project from the menu and toasts', async () => {
    const { toast } = await import('sonner')
    renderProjectRail()
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Archive project' }))
    await waitFor(() => expect(api.archiveProject).toHaveBeenCalledWith('/w/alpha'))
    expect(toast.success).toHaveBeenCalledWith('Project archived')
  })

  it('writes a palette color (and the default clears it) via set_project_appearance', async () => {
    renderProjectRail()
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Project color' }))
    const popover = await screen.findByTestId('project-color-popover')
    expect(within(popover).getAllByRole('menuitemradio')).toHaveLength(7) // 6 swatches + default
    fireEvent.click(within(popover).getByRole('menuitemradio', { name: 'Color 2' }))
    await waitFor(() => expect(api.setProjectAppearance).toHaveBeenCalledWith('/w/alpha', null, 'var(--chart-series-3)'))

    // Reopen and clear back to the default.
    fireEvent.click(screen.getByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Project color' }))
    const reopened = await screen.findByTestId('project-color-popover')
    fireEvent.click(within(reopened).getByRole('menuitemradio', { name: 'Default (clear color)' }))
    await waitFor(() => expect(api.setProjectAppearance).toHaveBeenCalledWith('/w/alpha', null, null))
    expect(screen.queryByTestId('project-color-popover')).not.toBeInTheDocument()
  })
})

describe('P-U2 archived projects section', () => {
  it('collapses at the rail bottom and restores via unarchive', async () => {
    const { toast } = await import('sonner')
    fixtures.registry = [projectRecord('/w/old', { name: 'Old Thing', archivedAtMs: NOW - 1000 })]
    renderRail([session('s1')])
    const section = await screen.findByTestId('sidebar-archived-projects')
    const toggle = within(section).getByTestId('sidebar-archived-projects-toggle')
    expect(toggle).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(toggle)
    expect(toggle).toHaveAttribute('aria-expanded', 'true')
    expect(screen.getByTestId('archived-project-row-/w/old')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Restore project Old Thing' }))
    await waitFor(() => expect(api.unarchiveProject).toHaveBeenCalledWith('/w/old'))
    expect(toast.success).toHaveBeenCalledWith('Project restored')
  })

  it('keeps the section hidden when nothing is archived', async () => {
    fixtures.registry = [projectRecord('/w/alpha')]
    renderRail([session('s1')])
    await screen.findByTestId('project-header-/w/alpha')
    expect(screen.queryByTestId('sidebar-archived-projects')).not.toBeInTheDocument()
  })

  // I5 review fix: a project with LIVE sessions must not render twice
  // (tree folder from its sessions + the archived row). Archiving skips
  // its session/routine buckets from the tree; restoring rebuilds them.
  it('archived project with live sessions leaves the tree and returns on restore', async () => {
    fixtures.registry = [projectRecord('/w/alpha')]
    renderRail([
      session('s1', { working_dir: '/w/alpha' }),
      session('s2', { working_dir: '/w/beta' }),
    ])
    expect(await screen.findByTestId('project-header-/w/alpha')).toBeInTheDocument()
    expect(screen.getByTestId('project-header-/w/beta')).toBeInTheDocument()

    // Archive from the ⋯ menu (applyProjectRecord updates local state).
    fireEvent.click(screen.getByRole('button', { name: 'Project actions: alpha' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'Archive project' }))
    await waitFor(() => expect(api.archiveProject).toHaveBeenCalledWith('/w/alpha'))

    // Gone from the tree (its live session s1 renders no duplicate folder);
    // the other project stays; the archived row keeps its 恢复 action.
    await waitFor(() =>
      expect(screen.queryByTestId('project-header-/w/alpha')).not.toBeInTheDocument(),
    )
    expect(screen.getByTestId('project-header-/w/beta')).toBeInTheDocument()
    fireEvent.click(screen.getByTestId('sidebar-archived-projects-toggle'))
    expect(screen.getByTestId('archived-project-row-/w/alpha')).toBeInTheDocument()

    // Restore → the project (and its sessions) return to the tree.
    fireEvent.click(screen.getByRole('button', { name: 'Restore project alpha' }))
    await waitFor(() =>
      expect(screen.getByTestId('project-header-/w/alpha')).toBeInTheDocument(),
    )
    expect(screen.queryByTestId('archived-project-row-/w/alpha')).not.toBeInTheDocument()
    expect(screen.getByTestId('desktop-session-row-s1')).toBeInTheDocument()
  })
})

describe('P-U4 grouping-lens persistence', () => {
  it('restores the smart lens from localStorage across an unmount/remount', () => {
    const first = renderRail([session('s1')], 'project')
    expect(screen.getByRole('button', { name: 'Smart' })).toHaveAttribute('aria-pressed', 'false')
    // Switch to the smart lens — persistGrouping writes the raw value.
    fireEvent.click(screen.getByRole('button', { name: 'Smart' }))
    expect(screen.getByRole('button', { name: 'Smart' })).toHaveAttribute('aria-pressed', 'true')
    first.unmount()

    // Remount WITHOUT re-seeding localStorage: readGrouping must restore
    // 'smart' from the persisted key on its own.
    render(
      <I18nProvider>
        <MemoryRouter>
          <SessionsSection
            sessions={[session('s1')]}
            sessionActivity={{}}
            currentSessionId={null}
            switchSession={vi.fn(async () => {})}
            renameSession={vi.fn(async () => {})}
            deleteSession={vi.fn(async () => {})}
          />
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.getByRole('button', { name: 'Smart' })).toHaveAttribute('aria-pressed', 'true')
  })
})

describe('localStorage migration (P-U2 retirement)', () => {
  it('migrates legacy names through rename_project and removes the key', async () => {
    fixtures.registry = [projectRecord('/w/alpha')]
    window.localStorage.setItem('shannon-projects', JSON.stringify({ '/w/alpha': 'Legacy Name' }))
    renderRail([session('s1')])
    await waitFor(() => expect(api.renameProject).toHaveBeenCalledWith('/w/alpha', 'Legacy Name'))
    await waitFor(() => expect(window.localStorage.getItem('shannon-projects')).toBeNull())
    // The migrated name renders once the registry reload lands.
    expect(await screen.findByText('Legacy Name')).toBeInTheDocument()
  })

  it('skips the rename when the registry already carries that name, still retiring the key', async () => {
    fixtures.registry = [projectRecord('/w/alpha', { name: 'Already There' })]
    window.localStorage.setItem('shannon-projects', JSON.stringify({ '/w/alpha': 'Already There' }))
    renderRail([session('s1')])
    await waitFor(() => expect(window.localStorage.getItem('shannon-projects')).toBeNull())
    expect(api.renameProject).not.toHaveBeenCalled()
  })

  it('resolves legacy tail-segment keys against the registry when unambiguous', async () => {
    fixtures.registry = [projectRecord('/w/alpha')]
    window.localStorage.setItem('shannon-projects', JSON.stringify({ alpha: 'Tail Name' }))
    renderRail([session('s1')])
    await waitFor(() => expect(api.renameProject).toHaveBeenCalledWith('/w/alpha', 'Tail Name'))
    await waitFor(() => expect(window.localStorage.getItem('shannon-projects')).toBeNull())
  })

  it('skips ambiguous tail keys instead of registering junk paths', async () => {
    fixtures.registry = [projectRecord('/w/alpha'), projectRecord('/h/alpha')]
    window.localStorage.setItem('shannon-projects', JSON.stringify({ alpha: 'Ambiguous' }))
    renderRail([session('s1', { working_dir: '/w/alpha' }), session('s2', { working_dir: '/h/alpha' })])
    await waitFor(() => expect(window.localStorage.getItem('shannon-projects')).toBeNull())
    expect(api.renameProject).not.toHaveBeenCalled()
  })

  it('retires the key even when entries are malformed', async () => {
    window.localStorage.setItem('shannon-projects', JSON.stringify({ '/w/alpha': 42 }))
    renderRail([session('s1')])
    await waitFor(() => expect(window.localStorage.getItem('shannon-projects')).toBeNull())
    expect(api.renameProject).not.toHaveBeenCalled()
  })
})

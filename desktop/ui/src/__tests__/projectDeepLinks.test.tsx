// P-U3 — project deep-link filters (2026-09-26 projects-plugins plan, Task 4):
//   /tasks?project=   — the removable chip shows the registry display name;
//     goal-run cards filter by dto.workingDir, the 例行 (Routines) tab by the
//     routine's working_dir, and the execution-history tab via the
//     task_id → routine working_dir join
//   /triage?project=  — inbox items join session_id → session working_dir;
//     items with no session (or an unknown one) drop out while active
//   /memory?project=  — presets the page's EXISTING project filter (resolved
//     against listMemoryProjects labels: exact match, then unique tail)
//
// The × on the chip strips the `project` search param (replace navigation)
// and every surface returns to the unscoped view.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { AppProvider } from '@/context/AppContext'
import Tasks from '@/pages/Tasks'
import Triage from '@/pages/Triage'
import Memory from '@/pages/Memory'
import { SessionsSection } from '@/components/SidebarSessions'
import * as api from '@/lib/tauri-api'
import type {
  GoalRunDto,
  InboxItem,
  ProjectRecord,
  ScheduledRoutine,
  SessionInfo,
  TaskExecution,
} from '@/types'

const NOW = Date.now()

function projectRecord(path: string, over: Partial<ProjectRecord> = {}): ProjectRecord {
  return { path, name: null, icon: null, color: null, archivedAtMs: null, createdAtMs: 0, ...over }
}

function routine(o: Partial<ScheduledRoutine> & { id: string; name: string }): ScheduledRoutine {
  return {
    description: '',
    trigger_type: 'cron',
    trigger: { cron: '0 9 * * 1' },
    command: 'do things',
    enabled: true,
    next_fire_at: Math.floor((NOW + 3600_000) / 1000),
    execution_policy: {},
    ...o,
  } as unknown as ScheduledRoutine
}

function goalRun(o: Partial<GoalRunDto> & { sessionId: string; title: string }): GoalRunDto {
  return {
    objective: 'objective',
    status: 'running',
    iterations: 1,
    maxTurns: 10,
    spentUsd: 0.1,
    budgetUsd: null,
    stallStrikes: 0,
    lastError: null,
    startedAtMs: NOW - 60_000,
    updatedAtMs: NOW - 60_000,
    workingDir: null,
    ...o,
  }
}

function execution(o: Partial<TaskExecution> & { run_id: string; task_id: string; task_name: string }): TaskExecution {
  return {
    started_at: Math.floor(NOW / 1000) - 600,
    finished_at: Math.floor(NOW / 1000) - 540,
    status: 'succeeded',
    ...o,
  }
}

function inboxItem(o: Partial<InboxItem> & { id: number; title: string }): InboxItem {
  return {
    source: 'routine',
    sourceId: 'r-alpha',
    sessionId: null,
    summary: 'summary',
    error: null,
    status: 'pending',
    createdAtMs: NOW - 60_000,
    updatedAtMs: NOW - 60_000,
    ...o,
  }
}

function session(o: Partial<SessionInfo> & { id: string }): SessionInfo {
  return {
    title: `S-${o.id}`,
    created_at: NOW - 3600_000,
    message_count: 1,
    ...o,
  }
}

function SearchProbe() {
  const loc = useLocation()
  return <div data-testid="search-probe" data-search={loc.search} data-pathname={loc.pathname} />
}

/** Tasks/Triage render inside the real AppProvider so useSessions() and
 *  useCatalog() resolve exactly like production; per-test overrides swap
 *  the tauri-api mock's resolved values (setup.ts owns the base mock). */
function renderAppPage(ui: React.ReactElement, initialEntry: string) {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[initialEntry]}>
        <AppProvider>
          {ui}
          <SearchProbe />
        </AppProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

const registry = [projectRecord('/w/alpha/', { name: 'Alpha Reg' }), projectRecord('/w/beta')]

beforeEach(() => {
  window.localStorage.clear()
  vi.clearAllMocks()
  // Defaults every page in this file can live with; individual tests
  // override the pieces they assert on.
  vi.mocked(api.listProjects).mockResolvedValue(registry)
  vi.mocked(api.listScheduledTasks).mockResolvedValue([])
  vi.mocked(api.listGoalRuns).mockResolvedValue([])
  vi.mocked(api.listTaskExecutions).mockResolvedValue([])
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.listInboxItems).mockResolvedValue([])
})

describe('/tasks?project= (P-U3)', () => {
  it('shows the removable chip with the registry display name (path fallback otherwise)', async () => {
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha')
    const chip = await screen.findByTestId('project-filter-chip')
    expect(chip.textContent).toContain('Alpha Reg')
    // Registry row carries a trailing slash — the normalized key still matches.
    expect(screen.getByTestId('project-filter-chip-remove')).toBeInTheDocument()

    fireEvent.click(screen.getByTestId('project-filter-chip-remove'))
    await waitFor(() => expect(screen.queryByTestId('project-filter-chip')).not.toBeInTheDocument())
    expect(screen.getByTestId('search-probe').getAttribute('data-search')).toBe('')
  })

  it('falls back to the path tail when the registry has no name for the project', async () => {
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Fbeta')
    const chip = await screen.findByTestId('project-filter-chip')
    expect(chip.textContent).toContain('beta')
  })

  it('renders no chip without the param', async () => {
    renderAppPage(<Tasks />, '/tasks')
    await screen.findByTestId('goal-run-panel')
    expect(screen.queryByTestId('project-filter-chip')).not.toBeInTheDocument()
  })

  it('filters goal-run cards by dto.workingDir', async () => {
    vi.mocked(api.listGoalRuns).mockResolvedValue([
      goalRun({ sessionId: 'g1', title: 'Alpha goal', workingDir: '/w/alpha' }),
      goalRun({ sessionId: 'g2', title: 'Beta goal', workingDir: '/w/beta' }),
      goalRun({ sessionId: 'g3', title: 'Unhoused goal', workingDir: null }),
    ])
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha')
    expect(await screen.findByText('Alpha goal')).toBeInTheDocument()
    expect(screen.queryByText('Beta goal')).not.toBeInTheDocument()
    expect(screen.queryByText('Unhoused goal')).not.toBeInTheDocument()

    // Removing the chip restores the full roster.
    fireEvent.click(screen.getByTestId('project-filter-chip-remove'))
    expect(await screen.findByText('Beta goal')).toBeInTheDocument()
    expect(screen.getByText('Unhoused goal')).toBeInTheDocument()
  })

  it('filters the Routines tab by routine working_dir', async () => {
    // P-U3 targets the 例行 tab — dev mode surfaces it.
    window.localStorage.setItem('shannon-sidebar-mode', 'dev')
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      routine({ id: 'r-alpha', name: 'Alpha digest', working_dir: '/w/alpha/' }),
      routine({ id: 'r-free', name: 'Free sweep' }),
      routine({ id: 'r-beta', name: 'Beta digest', working_dir: '/w/beta' }),
    ])
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha')
    fireEvent.click(await screen.findByRole('tab', { name: 'Routines' }))
    expect(await screen.findByText('Alpha digest')).toBeInTheDocument()
    // Trailing-slash variant of the param still matches; everything else drops.
    expect(screen.getByText('Alpha digest')).toBeInTheDocument()
    expect(screen.queryByText('Free sweep')).not.toBeInTheDocument()
    expect(screen.queryByText('Beta digest')).not.toBeInTheDocument()
  })

  it('filters execution-history rows through the task_id → routine working_dir join', async () => {
    vi.mocked(api.listScheduledTasks).mockResolvedValue([
      routine({ id: 'r-alpha', name: 'Alpha digest', working_dir: '/w/alpha' }),
      routine({ id: 'r-free', name: 'Free sweep' }),
    ])
    vi.mocked(api.listTaskExecutions).mockResolvedValue([
      execution({ run_id: 'run-1', task_id: 'r-alpha', task_name: 'Alpha run record' }),
      execution({ run_id: 'run-2', task_id: 'r-free', task_name: 'Free run record' }),
      execution({ run_id: 'run-3', task_id: 'r-unknown', task_name: 'Unknown run record' }),
    ])
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha')
    fireEvent.click(await screen.findByRole('tab', { name: 'History' }))
    expect(await screen.findByText('Alpha run record')).toBeInTheDocument()
    expect(screen.queryByText('Free run record')).not.toBeInTheDocument()
    expect(screen.queryByText('Unknown run record')).not.toBeInTheDocument()
  })
})

describe('/triage?project= (P-U3 session join)', () => {
  it('hides items whose session lives in another project (or has none)', async () => {
    vi.mocked(api.listSessions).mockResolvedValue([
      session({ id: 's-1', working_dir: '/w/alpha' }),
      session({ id: 's-2', working_dir: '/w/beta' }),
      session({ id: 's-3' }),
    ])
    vi.mocked(api.listInboxItems).mockResolvedValue([
      inboxItem({ id: 1, title: 'Alpha result', sessionId: 's-1' }),
      inboxItem({ id: 2, title: 'Beta result', sessionId: 's-2' }),
      inboxItem({ id: 3, title: 'Orphan result', sessionId: null }),
      inboxItem({ id: 4, title: 'Ghost result', sessionId: 's-unknown' }),
    ])
    renderAppPage(<Triage />, '/triage?project=%2Fw%2Falpha')

    expect(await screen.findByText('Alpha result')).toBeInTheDocument()
    expect(screen.queryByText('Beta result')).not.toBeInTheDocument()
    expect(screen.queryByText('Orphan result')).not.toBeInTheDocument()
    expect(screen.queryByText('Ghost result')).not.toBeInTheDocument()

    const chip = screen.getByTestId('project-filter-chip')
    expect(chip.textContent).toContain('Alpha Reg')

    // × restores the unscoped inbox.
    fireEvent.click(screen.getByTestId('project-filter-chip-remove'))
    expect(await screen.findByText('Beta result')).toBeInTheDocument()
    expect(screen.getByText('Orphan result')).toBeInTheDocument()
    expect(screen.getByText('Ghost result')).toBeInTheDocument()
    expect(screen.queryByTestId('project-filter-chip')).not.toBeInTheDocument()
  })

  it('shows the whole inbox without the param', async () => {
    vi.mocked(api.listSessions).mockResolvedValue([session({ id: 's-1', working_dir: '/w/alpha' })])
    vi.mocked(api.listInboxItems).mockResolvedValue([
      inboxItem({ id: 1, title: 'Alpha result', sessionId: 's-1' }),
      inboxItem({ id: 3, title: 'Orphan result', sessionId: null }),
    ])
    renderAppPage(<Triage />, '/triage')
    expect(await screen.findByText('Alpha result')).toBeInTheDocument()
    expect(screen.getByText('Orphan result')).toBeInTheDocument()
    expect(screen.queryByTestId('project-filter-chip')).not.toBeInTheDocument()
  })
})

describe('/memory?project= (P-U3 preset)', () => {
  it('presets the existing project filter via a unique path-tail match', async () => {
    vi.mocked(api.listMemoryProjects).mockResolvedValue(['alpha', 'personal'])
    render(
      <MemoryRouter initialEntries={['/memory?project=%2Fw%2Falpha']}>
        <Memory />
      </MemoryRouter>,
    )
    // The preset lands after listMemoryProjects resolves — the EXISTING
    // select carries it, and the list query is issued scoped.
    expect(await screen.findByDisplayValue('alpha')).toBeInTheDocument()
    await waitFor(() =>
      expect(vi.mocked(api.listMemories).mock.calls.some(c => (c[0] as { project: string | null } | undefined)?.project === 'alpha')).toBe(true),
    )
  })

  it('prefers an exact project-label match over the tail', async () => {
    vi.mocked(api.listMemoryProjects).mockResolvedValue(['/w/alpha', 'alpha'])
    render(
      <MemoryRouter initialEntries={['/memory?project=%2Fw%2Falpha']}>
        <Memory />
      </MemoryRouter>,
    )
    expect(await screen.findByDisplayValue('/w/alpha')).toBeInTheDocument()
  })

  it('leaves the filter at 全部 when the param matches nothing', async () => {
    vi.mocked(api.listMemoryProjects).mockResolvedValue(['alpha', 'personal'])
    render(
      <MemoryRouter initialEntries={['/memory?project=%2Fnowhere%2Fzzz']}>
        <Memory />
      </MemoryRouter>,
    )
    await waitFor(() => expect(api.listMemoryProjects).toHaveBeenCalled())
    // The project select (not the category one, which also sits at 'all').
    const projectSelect = screen.getByRole('combobox', { name: 'Filter by project' })
    expect((projectSelect as HTMLSelectElement).value).toBe('all')
  })
})

// Cross-check the join key contract with the rail: the same normalized
// full-path key drives the project menu's deep links and the pages' filters.
describe('deep-link key contract (rail ↔ pages)', () => {
  it('a project-menu navigation lands on /tasks with the encoded project param', async () => {
    window.localStorage.setItem('shannon-sessions-grouping', 'project')
    render(
      <I18nProvider>
        <MemoryRouter>
          <SessionsSection
            sessions={[session({ id: 's-1', working_dir: '/w/alpha' })]}
            sessionActivity={{}}
            currentSessionId={null}
            switchSession={vi.fn(async () => {})}
            renameSession={vi.fn(async () => {})}
            deleteSession={vi.fn(async () => {})}
          />
          <SearchProbe />
        </MemoryRouter>
      </I18nProvider>,
    )
    // Single-project rails stay flat — the registry fixture from beforeEach
    // (Alpha Reg + beta rows) is what engages the tree and names the group.
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: Alpha Reg' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'View automations' }))
    expect(screen.getByTestId('search-probe').getAttribute('data-search')).toBe('?project=%2Fw%2Falpha')

    // I2 review fix: 新建例行 no longer navigates to the byte-identical URL
    // — it carries the &new=routine create marker.
    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: Alpha Reg' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'New routine' }))
    expect(screen.getByTestId('search-probe').getAttribute('data-pathname')).toBe('/tasks')
    expect(screen.getByTestId('search-probe').getAttribute('data-search')).toBe('?project=%2Fw%2Falpha&new=routine')

    fireEvent.click(await screen.findByRole('button', { name: 'Project actions: Alpha Reg' }))
    fireEvent.click(screen.getByRole('menuitem', { name: 'View in inbox' }))
    expect(screen.getByTestId('search-probe').getAttribute('data-pathname')).toBe('/triage')
    expect(screen.getByTestId('search-probe').getAttribute('data-search')).toBe('?project=%2Fw%2Falpha')
  })
})

describe('/tasks &new=routine marker (I2 review fix)', () => {
  it('opens the create-schedule form and drains the marker param', async () => {
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha&new=routine')
    // The form opens without any extra click…
    expect(await screen.findByText('Create Scheduled Routine')).toBeInTheDocument()
    // …and the one-shot marker is drained (replace navigation), leaving the
    // project scope intact so a refresh doesn't re-open the form.
    await waitFor(() =>
      expect(screen.getByTestId('search-probe').getAttribute('data-search')).toBe('?project=%2Fw%2Falpha'),
    )
    expect(screen.getByTestId('project-filter-chip')).toBeInTheDocument()
  })

  it('leaves the form closed on the plain 查看自动化 URL', async () => {
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha')
    await screen.findByTestId('goal-run-panel')
    expect(screen.queryByText('Create Scheduled Routine')).not.toBeInTheDocument()
  })

  it('defaults a created routine’s working_dir to the active project key', async () => {
    renderAppPage(<Tasks />, '/tasks?project=%2Fw%2Falpha&new=routine')
    fireEvent.change(await screen.findByRole('textbox', { name: 'Name *' }), {
      target: { value: 'Project sweep' },
    })
    fireEvent.change(screen.getByRole('textbox', { name: 'Prompt *' }), {
      target: { value: 'Do the sweep' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Create Routine' }))
    await waitFor(() => expect(api.createScheduledTask).toHaveBeenCalledTimes(1))
    // The form sets no working_dir; the page defaults it to the normalized
    // ?project= key — the routine lands housed instead of vanishing from
    // the scoped view.
    expect(vi.mocked(api.createScheduledTask).mock.calls[0][0]).toMatchObject({
      name: 'Project sweep',
      working_dir: '/w/alpha',
    })
  })
})

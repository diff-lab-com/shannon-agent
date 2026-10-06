// Batch F (2026-09-20 delta analysis §批F): structural upgrades, v1 scopes.
//  - F1-v1: the rail's 自动化 section lists enabled scheduled routines
//  - F2: project folders rename in place (double-click) and persist
//  - F4: the document reader's table toolbar copies TSV
//  - F6: the 智能 grouping pins running → needs-attention → recent

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { SessionsSection } from '@/components/SidebarSessions'
import { DocumentRenderer } from '@/components/artifact/DocumentRenderer'
import * as api from '@/lib/tauri-api'
import type { ScheduledRoutine, SessionActivity, SessionInfo } from '@/types'

const NOW = Date.now()

const routines = vi.hoisted(() => ({
  list: [] as ScheduledRoutine[],
}))

const projects = vi.hoisted(() => ({
  registry: [] as Array<{
    path: string
    name: string | null
    icon: string | null
    color: string | null
    archivedAtMs: number | null
    createdAtMs: number
  }>,
}))

vi.mock('@/lib/tauri-api', () => ({
  listScheduledTasks: vi.fn(async () => routines.list),
  searchSessions: vi.fn(async () => []),
  // P-U2: the engine project registry backs the rail's project names.
  listProjects: vi.fn(async () => projects.registry),
  renameProject: vi.fn(async (path: string, name: string | null) => {
    const record = { path, name, icon: null, color: null, archivedAtMs: null, createdAtMs: 0 }
    projects.registry = projects.registry.filter(p => p.path !== path).concat(record)
    return record
  }),
}))

function session(id: string, over: Partial<SessionInfo> = {}): SessionInfo {
  return { id, title: `S-${id}`, created_at: NOW - 3600_000, message_count: 1, working_dir: '/repo/proj-a', ...over }
}

function activity(over: Partial<SessionActivity> = {}): SessionActivity {
  return { running: false, startedAt: null, lastActivity: NOW, activeTool: null, ...over }
}

function renderRail(sessions: SessionInfo[], acts: Record<string, SessionActivity>, grouping: 'project' | 'session' | 'smart') {
  window.localStorage.setItem('shannon-sessions-grouping', grouping)
  return render(
    <I18nProvider>
      <MemoryRouter>
        <SessionsSection
          sessions={sessions}
          sessionActivity={acts}
          currentSessionId={null}
          switchSession={async () => {}}
          renameSession={async () => {}}
          deleteSession={async () => {}}
        />
      </MemoryRouter>
    </I18nProvider>,
  )
}

describe('smart grouping (F6)', () => {
  beforeEach(() => window.localStorage.clear())

  it('pins running first, then needs-attention, then the recent remainder', () => {
    const sessions = [
      session('idle', { working_dir: undefined }),
      session('bad', { working_dir: undefined }),
      session('live', { working_dir: undefined }),
    ]
    const acts = {
      live: activity({ running: true, startedAt: NOW - 1000 }),
      bad: activity({ failed: true }),
    }
    renderRail(sessions, acts, 'smart')
    // ScrollArea also carries role="presentation" — locate the section
    // headers by their label text and assert document order instead.
    const running = screen.getByText('Running')
    const attention = screen.getByText('Needs attention')
    const recent = screen.getByText('Recent')
    expect(running.compareDocumentPosition(attention) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
    expect(attention.compareDocumentPosition(recent) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })
})

describe('project rename (F2, now registry-backed — P-U2)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    projects.registry = []
    vi.clearAllMocks()
  })

  it('renames a project folder via double-click and commits through the engine registry', async () => {
    const user = userEvent.setup()
    // Two projects so project grouping engages (>1 distinct).
    renderRail([session('s1'), session('s2', { working_dir: '/repo/proj-b' })], {}, 'project')
    const header = screen.getByText('proj-a').closest('button')!
    await user.dblClick(header)
    const input = await screen.findByRole('textbox')
    fireEvent.change(input, { target: { value: 'My Project' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    // P-U2: the rename commits via rename_project (the localStorage
    // `shannon-projects` registry was retired), and the label updates from
    // the returned registry record.
    await vi.waitFor(() => {
      expect(api.renameProject).toHaveBeenCalledWith('/repo/proj-a', 'My Project')
    })
    expect(await screen.findByText('My Project')).toBeInTheDocument()
    expect(window.localStorage.getItem('shannon-projects')).toBeNull()
  })
})

describe('automations rail section (F1-v1)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    routines.list = []
    projects.registry = []
  })

  it('lists enabled routines with the soon badge when next fire is imminent', async () => {
    routines.list = [
      { id: 'r1', name: '每30分钟巡检', enabled: true, next_fire_at: NOW + 60_000 } as ScheduledRoutine,
      { id: 'r2', name: '已停用任务', enabled: false, next_fire_at: null } as ScheduledRoutine,
    ]
    renderRail([session('s1', { working_dir: undefined })], {}, 'session')
    const sec = await screen.findByTestId('sidebar-automations')
    expect(sec.textContent).toContain('每30分钟巡检')
    expect(sec.textContent).not.toContain('已停用任务')
    expect(screen.getByRole('img', { name: 'Soon' })).toBeInTheDocument()
  })

  it('hides the section when no routines are enabled', async () => {
    renderRail([session('s1', { working_dir: undefined })], {}, 'session')
    await vi.waitFor(() => {
      expect(screen.queryByTestId('sidebar-automations')).not.toBeInTheDocument()
    })
  })
})

describe('document table toolbar (F4)', () => {
  it('copies the table as TSV', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined)
    Object.defineProperty(navigator, 'clipboard', {
      value: { writeText },
      configurable: true,
    })
    const md = '| A | B |\n| - | - |\n| 1 | 2 |'
    render(
      <I18nProvider>
        <DocumentRenderer source={md} />
      </I18nProvider>,
    )
    const copy = screen.getByRole('button', { name: 'Copy' })
    fireEvent.click(copy)
    await vi.waitFor(() => {
      expect(writeText).toHaveBeenCalledWith('A\tB\n1\t2')
    })
  })
})

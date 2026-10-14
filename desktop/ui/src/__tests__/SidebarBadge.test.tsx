import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import '@testing-library/jest-dom'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { Sidebar } from '@/components/Sidebar'
import * as api from '@/lib/tauri-api'
import type { TaskItem } from '@/types'

// The /triage badge reads `get_inbox_stats().pending` via useInboxStats (P0-3).
const statsMock = vi.hoisted(() => ({
  current: { pending: 3, today: 5 },
}))

vi.mock('@/hooks/inbox', () => ({
  useInboxStats: () => ({
    stats: statsMock.current,
    refresh: vi.fn(),
  }),
}))

// M6 (design-parity R2): the 「任务」 nav row's amber dot counts
// failed/blocked tasks from the catalog slice AppProvider already loads
// (`list_tasks`). The global setup mocks it to []; tests override per
// scenario here.
const taskFixture = (o: Partial<TaskItem> & { id: string }): TaskItem => ({
  title: `Task ${o.id}`,
  status: 'completed',
  ...o,
})

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter initialEntries={['/chat']}>{ui}</MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Sidebar Badge', () => {
  beforeEach(() => {
    statsMock.current = { pending: 3, today: 5 }
    vi.mocked(api.listTasks).mockResolvedValue([])
  })

  it('shows pending count badge when > 0', async () => {
    render(wrap(<Sidebar />))
    await waitFor(() => {
      const badge = screen.getByText('3')
      expect(badge).toBeInTheDocument()
    })
  })

  it('hides badge when pending count is 0', async () => {
    statsMock.current = { pending: 0, today: 0 }
    render(wrap(<Sidebar />))
    await waitFor(() => {
      expect(screen.queryByText('0')).not.toBeInTheDocument()
    })
  })
})

// ── M6 (design-parity R2 §3.2, 02-chat.html:106): the 「任务」 nav row's
// amber attention dot — failed/blocked tasks > 0, tooltip carries the count. ─
describe('Sidebar tasks attention dot (M6)', () => {
  beforeEach(() => {
    statsMock.current = { pending: 0, today: 0 }
    vi.mocked(api.listTasks).mockResolvedValue([])
  })

  it('shows the amber dot with the count when tasks failed or are blocked', async () => {
    vi.mocked(api.listTasks).mockResolvedValue([
      taskFixture({ id: 't1', status: 'failed' }),
      taskFixture({ id: 't2', status: 'blocked' }),
      taskFixture({ id: 't3', status: 'completed' }),
      taskFixture({ id: 't4', status: 'running' }),
    ])
    render(wrap(<Sidebar />))
    const dot = await screen.findByTestId('sidebar-tasks-attention-dot')
    expect(dot).toHaveAttribute('title', '2 tasks failed or blocked')
    // Amber = attention (warning token), not the error red of the session
    // health dots (G7 ruling: both signals coexist, different granularity).
    expect(dot.className).toContain('bg-warning')
  })

  it('hides the dot when every task is healthy', async () => {
    vi.mocked(api.listTasks).mockResolvedValue([
      taskFixture({ id: 't1', status: 'completed' }),
      taskFixture({ id: 't2', status: 'running' }),
    ])
    render(wrap(<Sidebar />))
    await waitFor(() => {
      expect(screen.queryByTestId('sidebar-tasks-attention-dot')).not.toBeInTheDocument()
    })
  })

  it('hides the dot when the task list fails to load (honest off)', async () => {
    vi.mocked(api.listTasks).mockRejectedValue(new Error('ipc down'))
    render(wrap(<Sidebar />))
    await waitFor(() => {
      // refreshTasks swallows into a soft failure — tasks stay empty.
      expect(screen.queryByTestId('sidebar-tasks-attention-dot')).not.toBeInTheDocument()
    })
  })
})

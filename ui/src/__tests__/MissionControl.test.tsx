// Tests for Mission Control / Conversations page — tabs + Board (kanban).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import MissionControl from '@/pages/MissionControl'
import { SIDEBAR_MODE_KEY } from '@/components/Sidebar'
import type { TaskItem } from '@/types'

const useAppSpy = vi.hoisted(() => vi.fn())
vi.mock('@/context/AppContext', () => ({
  useApp: (...args: unknown[]) => useAppSpy(...args),
}))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  updateTask: vi.fn(),
}))

vi.mock('@/components/tasks/TaskDetailDrawer', () => ({
  __esModule: true,
  default: ({ task, onClose }: { task: { id: string; title: string } | null; onClose: () => void }) =>
    task ? (
      <div data-testid="drawer" onClick={onClose}>
        drawer:{task.id}
      </div>
    ) : null,
}))

function makeTask(overrides: Partial<TaskItem>): TaskItem {
  return {
    id: overrides.id ?? 't1',
    title: overrides.title ?? 'Task',
    status: overrides.status ?? 'pending',
    ...overrides,
  } as TaskItem
}

function wrap(ui: React.ReactElement) {
  return <MemoryRouter>{ui}</MemoryRouter>
}

beforeEach(() => {
  useAppSpy.mockReturnValue({ tasks: [], sessions: [], agents: [], refreshTasks: vi.fn() })
  window.localStorage.removeItem(SIDEBAR_MODE_KEY)
})

// Helper: enable dev sidebar mode so the Board tab renders.
function enableDevMode() {
  window.localStorage.setItem(SIDEBAR_MODE_KEY, 'dev')
}

// Helper: switch to the Board tab where kanban columns live.
function switchToBoard() {
  fireEvent.click(screen.getByRole('button', { name: /Board tab/ }))
}

describe('MissionControl — tabs', () => {
  it('renders only Today/All tabs in simple mode (default)', () => {
    render(wrap(<MissionControl />))
    expect(screen.getByRole('button', { name: /Today tab/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /All tab/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Board tab/ })).not.toBeInTheDocument()
  })

  it('renders Today/All/Board tabs in dev mode', () => {
    enableDevMode()
    render(wrap(<MissionControl />))
    expect(screen.getByRole('button', { name: /Today tab/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /All tab/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Board tab/ })).toBeInTheDocument()
  })

  it('defaults to All tab (list replaces board as default)', () => {
    render(wrap(<MissionControl />))
    expect(screen.getByRole('button', { name: /All tab/ })).toHaveAttribute('aria-pressed', 'true')
  })

  it('Today tab shows Chats today stat after switching', () => {
    render(wrap(<MissionControl />))
    fireEvent.click(screen.getByRole('button', { name: /Today tab/ }))
    expect(screen.getByText('Chats today')).toBeInTheDocument()
  })

  it('All tab shows search input (default)', () => {
    render(wrap(<MissionControl />))
    expect(screen.getByPlaceholderText('Search conversations...')).toBeInTheDocument()
  })
})

describe('MissionControl — Board tab', () => {
  beforeEach(() => {
    enableDevMode()
  })

  it('renders all five status columns after switching to Board', () => {
    render(wrap(<MissionControl />))
    switchToBoard()
    expect(screen.getByRole('row', { name: 'Queued' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'In Progress' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Blocked' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Completed' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Failed' })).toBeInTheDocument()
  })

  it('shows empty-state hint for an empty column on Board', () => {
    render(wrap(<MissionControl />))
    switchToBoard()
    expect(screen.getAllByText('Nothing here.').length).toBeGreaterThan(0)
  })

  it('classifies in_progress into the In Progress column', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'a', title: 'Active Task', status: 'in_progress' })],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    const col = screen.getByRole('row', { name: 'In Progress' })
    expect(within(col).getByText('Active Task')).toBeInTheDocument()
  })

  it('classifies blocked into the Blocked column', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'b', title: 'Stuck Task', status: 'blocked' })],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    const col = screen.getByRole('row', { name: 'Blocked' })
    expect(within(col).getByText('Stuck Task')).toBeInTheDocument()
  })

  it('classifies failed and cancelled into the Failed column', () => {
    useAppSpy.mockReturnValue({
      tasks: [
        makeTask({ id: 'f1', title: 'Boom', status: 'failed' }),
        makeTask({ id: 'f2', title: 'Aborted', status: 'cancelled' }),
      ],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    const col = screen.getByRole('row', { name: 'Failed' })
    expect(within(col).getByText('Boom')).toBeInTheDocument()
    expect(within(col).getByText('Aborted')).toBeInTheDocument()
  })

  it('renders task count in column header', () => {
    useAppSpy.mockReturnValue({
      tasks: [
        makeTask({ id: 'a', title: 'A', status: 'pending' }),
        makeTask({ id: 'b', title: 'B', status: 'pending' }),
        makeTask({ id: 'c', title: 'C', status: 'queued' }),
      ],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    const col = screen.getByRole('row', { name: 'Queued' })
    expect(within(col).getByText('3')).toBeInTheDocument()
  })

  it('sorts by priority within column (critical first)', () => {
    useAppSpy.mockReturnValue({
      tasks: [
        makeTask({ id: 'low', title: 'Low Pri', status: 'pending', priority: 'low' }),
        makeTask({ id: 'crit', title: 'Critical', status: 'pending', priority: 'critical' }),
        makeTask({ id: 'norm', title: 'Norm', status: 'pending', priority: 'normal' }),
      ],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    const col = screen.getByRole('row', { name: 'Queued' })
    const cards = within(col).getAllByRole('button')
    expect(cards[0]).toHaveTextContent('Critical')
    expect(cards[1]).toHaveTextContent('Norm')
    expect(cards[2]).toHaveTextContent('Low Pri')
  })

  it('opens detail drawer when a card is clicked', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 't-click', title: 'Clickable', status: 'pending' })],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    switchToBoard()
    expect(screen.queryByTestId('drawer')).not.toBeInTheDocument()
    fireEvent.click(screen.getByText('Clickable'))
    expect(screen.getByTestId('drawer')).toHaveTextContent('drawer:t-click')
  })

  it('shows total task count in header subtitle', () => {
    useAppSpy.mockReturnValue({
      tasks: [
        makeTask({ id: 'a', status: 'pending' }),
        makeTask({ id: 'b', status: 'completed' }),
      ],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    expect(screen.getByText(/Aggregated view across 2 tasks/)).toBeInTheDocument()
  })

  it('uses singular "task" for one task', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'only', status: 'pending' })],
      sessions: [],
      agents: [],
      refreshTasks: vi.fn(),
    })
    render(wrap(<MissionControl />))
    expect(screen.getByText(/Aggregated view across 1 task /)).toBeInTheDocument()
  })
})

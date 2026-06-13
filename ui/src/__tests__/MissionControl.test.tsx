// Tests for G12 Mission Control kanban page.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'
import MissionControl from '@/pages/MissionControl'
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

beforeEach(() => {
  useAppSpy.mockReturnValue({ tasks: [], refreshTasks: vi.fn() })
})

describe('MissionControl', () => {
  it('renders all five status columns', () => {
    render(<MissionControl />)
    expect(screen.getByRole('row', { name: 'Queued' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'In Progress' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Blocked' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Completed' })).toBeInTheDocument()
    expect(screen.getByRole('row', { name: 'Failed' })).toBeInTheDocument()
  })

  it('shows empty-state hint for an empty column', () => {
    render(<MissionControl />)
    expect(screen.getAllByText('Nothing here.').length).toBeGreaterThan(0)
  })

  it('classifies in_progress into the In Progress column', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'a', title: 'Active Task', status: 'in_progress' })],
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
    const col = screen.getByRole('row', { name: 'In Progress' })
    expect(within(col).getByText('Active Task')).toBeInTheDocument()
  })

  it('classifies blocked into the Blocked column', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'b', title: 'Stuck Task', status: 'blocked' })],
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
    const col = screen.getByRole('row', { name: 'Blocked' })
    expect(within(col).getByText('Stuck Task')).toBeInTheDocument()
  })

  it('classifies failed and cancelled into the Failed column', () => {
    useAppSpy.mockReturnValue({
      tasks: [
        makeTask({ id: 'f1', title: 'Boom', status: 'failed' }),
        makeTask({ id: 'f2', title: 'Aborted', status: 'cancelled' }),
      ],
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
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
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
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
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
    const col = screen.getByRole('row', { name: 'Queued' })
    const cards = within(col).getAllByRole('button')
    expect(cards[0]).toHaveTextContent('Critical')
    expect(cards[1]).toHaveTextContent('Norm')
    expect(cards[2]).toHaveTextContent('Low Pri')
  })

  it('opens detail drawer when a card is clicked', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 't-click', title: 'Clickable', status: 'pending' })],
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
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
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
    expect(screen.getByText(/Aggregated view across 2 tasks/)).toBeInTheDocument()
  })

  it('uses singular "task" for one task', () => {
    useAppSpy.mockReturnValue({
      tasks: [makeTask({ id: 'only', status: 'pending' })],
      refreshTasks: vi.fn(),
    })
    render(<MissionControl />)
    expect(screen.getByText(/Aggregated view across 1 task /)).toBeInTheDocument()
  })
})

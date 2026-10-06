import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { MemoryRouter } from 'react-router-dom'
import type * as ReactRouterDom from 'react-router-dom'
import OPCKanbanBoard from '@/components/opc/OPCKanbanBoard'
import type { TaskItem } from '@/types'

const navigate = vi.hoisted(() => vi.fn())

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
  return { ...actual, useNavigate: () => navigate }
})

// P1-3: the quick-create flow crosses the bridge (start the run, mint the
// board card). Tests pin both commands.
const startBackgroundTask = vi.hoisted(() => vi.fn())
const updateTask = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const toastErrorFn = vi.hoisted(() => vi.fn())

vi.mock('sonner', () => ({
  toast: { success: toastSuccess, error: toastErrorFn },
}))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  startBackgroundTask: (...args: unknown[]) => startBackgroundTask(...args),
  updateTask: (...args: unknown[]) => updateTask(...args),
  // P2-6/F6: typing in the quick-add mounts CostEstimateHint, whose 350ms
  // debounce calls estimateTaskCost — must exist or the timer throws an
  // unhandled error whenever a test outlives the debounce.
  estimateTaskCost: vi.fn().mockResolvedValue({ hasHistory: false, runsCounted: 0, minUsd: null, maxUsd: null, avgUsd: null, lastUsd: null }),
}))

const refreshTasks = vi.fn()

function renderBoard(tasks: TaskItem[]) {
  return render(
    <MemoryRouter>
      <OPCKanbanBoard tasks={tasks} refreshTasks={refreshTasks} />
    </MemoryRouter>,
  )
}

const queuedTask: TaskItem = {
  id: 't-queued-1',
  title: 'Queued Task',
  status: 'queued',
}

beforeEach(() => {
  navigate.mockReset()
  refreshTasks.mockReset()
  toastSuccess.mockReset()
  toastErrorFn.mockReset()
  startBackgroundTask.mockReset()
  startBackgroundTask.mockResolvedValue('new-task-id')
  updateTask.mockReset()
  updateTask.mockResolvedValue({ id: 'new-task-id', title: 'x', status: 'in_progress' })
})

describe('OPCKanbanBoard navigation', () => {
  it('clicking a queued card navigates to /opc/task/:id', () => {
    renderBoard([queuedTask])
    const card = screen.getByRole('button', { name: 'Queued Task' })
    fireEvent.click(card)
    expect(navigate).toHaveBeenCalledWith('/opc/task/t-queued-1')
  })

  it('Enter key on a queued card navigates to /opc/task/:id', () => {
    renderBoard([queuedTask])
    const card = screen.getByRole('button', { name: 'Queued Task' })
    fireEvent.keyDown(card, { key: 'Enter' })
    expect(navigate).toHaveBeenCalledWith('/opc/task/t-queued-1')
  })
})

describe('OPCKanbanBoard quick create (P1-3 created-then-invisible)', () => {
  it('starts the run AND mints a board card, then refreshes', async () => {
    renderBoard([queuedTask])
    const input = screen.getByLabelText('Add task')
    fireEvent.change(input, { target: { value: 'Ship the changelog' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create task' }))
    await waitFor(() => expect(startBackgroundTask).toHaveBeenCalledWith('Ship the changelog'))
    await waitFor(() =>
      expect(updateTask).toHaveBeenCalledWith({
        id: 'new-task-id',
        title: 'Ship the changelog',
        status: 'in_progress',
      }),
    )
    await waitFor(() => expect(refreshTasks).toHaveBeenCalled())
    // Success clears the composer.
    await waitFor(() => expect((input as HTMLInputElement).value).toBe(''))
  })

  it('still reports success for the run when only the board card write fails', async () => {
    updateTask.mockRejectedValue(new Error('tasks dir unwritable'))
    renderBoard([queuedTask])
    const input = screen.getByLabelText('Add task')
    fireEvent.change(input, { target: { value: 'Run the sweep' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create task' }))
    await waitFor(() => expect(startBackgroundTask).toHaveBeenCalledWith('Run the sweep'))
    // The board-sync failure is surfaced distinctly…
    await waitFor(() =>
      expect(String(toastErrorFn.mock.calls[0]?.[0])).toContain('board card'),
    )
    // …the run success toast still fires, and the composer clears.
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Task created'))
    await waitFor(() => expect((input as HTMLInputElement).value).toBe(''))
  })

  it('reports failure and keeps the text when the run cannot start', async () => {
    startBackgroundTask.mockRejectedValue(new Error('provider down'))
    renderBoard([queuedTask])
    const input = screen.getByLabelText('Add task')
    fireEvent.change(input, { target: { value: 'Failing task' } })
    fireEvent.click(screen.getByRole('button', { name: 'Create task' }))
    await waitFor(() =>
      expect(String(toastErrorFn.mock.calls[0]?.[0])).toContain('Failed to create task'),
    )
    expect(updateTask).not.toHaveBeenCalled()
    expect(refreshTasks).not.toHaveBeenCalled()
    expect((input as HTMLInputElement).value).toBe('Failing task')
  })
})

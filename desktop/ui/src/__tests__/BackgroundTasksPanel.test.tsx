import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import BackgroundTasksPanel, {
  formatElapsed,
  runningBackgroundTasks,
  terminalBackgroundTasks,
  MAX_TERMINAL_ROWS,
} from '@/components/tasks/BackgroundTasksPanel'
import type { BackgroundTaskInfo } from '@/types'

const cancelBackgroundTask = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const toastErrorFn = vi.hoisted(() => vi.fn())

vi.mock('sonner', () => ({
  toast: { success: toastSuccess, error: toastErrorFn },
}))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  cancelBackgroundTask: (...args: unknown[]) => cancelBackgroundTask(...args),
}))

// The panel reads the catalog slice; tests pin the backgroundTasks list.
const catalogBackgroundTasks = vi.hoisted(() => vi.fn())

vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ({ backgroundTasks: catalogBackgroundTasks() }),
}))

function makeTask(overrides: Partial<BackgroundTaskInfo> = {}): BackgroundTaskInfo {
  return {
    task_id: 'bt-1',
    prompt: 'Refactor the parser module',
    status: 'running',
    started_at: Date.now() - 65_000,
    completed_at: null,
    output: '',
    ...overrides,
  }
}

beforeEach(() => {
  cancelBackgroundTask.mockReset()
  cancelBackgroundTask.mockResolvedValue(true)
  toastSuccess.mockReset()
  toastErrorFn.mockReset()
  catalogBackgroundTasks.mockReset()
  catalogBackgroundTasks.mockReturnValue([])
})

afterEach(() => {
  vi.useRealTimers()
})

describe('BackgroundTasksPanel', () => {
  it('renders nothing when there are no tasks at all', () => {
    catalogBackgroundTasks.mockReturnValue([])
    const { container } = render(<BackgroundTasksPanel />)
    expect(container).toBeEmptyDOMElement()
  })

  it('lists in-flight tasks with name, status and elapsed time', () => {
    catalogBackgroundTasks.mockReturnValue([
      makeTask({ task_id: 'bt-1', prompt: 'Refactor the parser module', started_at: Date.now() - 65_000 }),
      makeTask({ task_id: 'bt-2', prompt: 'Write release notes', started_at: Date.now() - 5_000 }),
    ])
    render(<BackgroundTasksPanel />)
    expect(screen.getByTestId('background-tasks-panel')).toBeInTheDocument()
    expect(screen.getByText('Refactor the parser module')).toBeInTheDocument()
    expect(screen.getByText('Write release notes')).toBeInTheDocument()
    // Two running rows, each with a stop button.
    expect(screen.getAllByTestId('background-task-row')).toHaveLength(2)
    expect(screen.getAllByRole('button', { name: 'Stop background task' })).toHaveLength(2)
    // Elapsed is rendered as m:ss (65s → "1:05").
    expect(screen.getByText('1:05')).toBeInTheDocument()
  })

  it('keeps recent terminal rows visible with status, duration and error summary (R2-P1-5)', () => {
    const finished = Date.now()
    catalogBackgroundTasks.mockReturnValue([
      makeTask({
        task_id: 'bt-done',
        prompt: 'Summarize the changelog',
        status: 'completed',
        started_at: finished - 95_000,
        completed_at: finished - 30_000,
        output: 'All done',
      }),
      makeTask({
        task_id: 'bt-fail',
        prompt: 'Migrate the database',
        status: 'failed',
        started_at: finished - 60_000,
        completed_at: finished - 10_000,
        output: 'engine stream broke\nfinal error: connection reset by peer',
      }),
    ])
    render(<BackgroundTasksPanel />)
    // Terminal segment exists; running segment does not.
    expect(screen.getByTestId('background-tasks-terminal')).toBeInTheDocument()
    expect(screen.queryByTestId('background-task-row')).not.toBeInTheDocument()
    // Status is a text label per row — not colour-only (a11y).
    expect(screen.getByText('Completed')).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
    // Duration from started_at → completed_at (65s → "1:05").
    expect(screen.getByText('1:05')).toBeInTheDocument()
    // Failed row carries the error headline (last non-empty output line).
    expect(screen.getByText('final error: connection reset by peer')).toBeInTheDocument()
    expect(screen.queryByText('engine stream broke')).not.toBeInTheDocument()
    // Completed rows show no error line.
    expect(screen.queryByText('All done')).not.toBeInTheDocument()
  })

  it('shows a cancelled task in the recent slice without an error line', () => {
    catalogBackgroundTasks.mockReturnValue([
      makeTask({
        task_id: 'bt-cancel',
        status: 'cancelled',
        completed_at: Date.now() - 1_000,
        output: 'Task cancelled by user',
      }),
    ])
    render(<BackgroundTasksPanel />)
    expect(screen.getByTestId('background-task-terminal-row')).toBeInTheDocument()
    expect(screen.getByText('Cancelled')).toBeInTheDocument()
    expect(screen.queryByText('Failed')).not.toBeInTheDocument()
  })

  it('caps the recent-terminal slice at the newest MAX_TERMINAL_ROWS rows', () => {
    const now = Date.now()
    const tasks: BackgroundTaskInfo[] = []
    for (let i = 0; i < MAX_TERMINAL_ROWS + 3; i++) {
      tasks.push(makeTask({
        task_id: `bt-${i}`,
        prompt: `task ${i}`,
        status: 'completed',
        started_at: now - (i + 2) * 60_000,
        completed_at: now - (i + 1) * 60_000,
      }))
    }
    const slice = terminalBackgroundTasks(tasks)
    expect(slice).toHaveLength(MAX_TERMINAL_ROWS)
    // Newest first: bt-0 finished most recently.
    expect(slice[0].task_id).toBe('bt-0')
    expect(slice[MAX_TERMINAL_ROWS - 1].task_id).toBe(`bt-${MAX_TERMINAL_ROWS - 1}`)
  })

  it('stops a task through cancel_background_task', async () => {
    catalogBackgroundTasks.mockReturnValue([makeTask()])
    render(<BackgroundTasksPanel />)
    fireEvent.click(screen.getByRole('button', { name: 'Stop background task' }))
    await waitFor(() => expect(cancelBackgroundTask).toHaveBeenCalledWith('bt-1'))
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Task cancelled'))
  })

  it('shows a failure toast when the stop call fails', async () => {
    cancelBackgroundTask.mockRejectedValue(new Error('channel closed'))
    catalogBackgroundTasks.mockReturnValue([makeTask()])
    render(<BackgroundTasksPanel />)
    fireEvent.click(screen.getByRole('button', { name: 'Stop background task' }))
    await waitFor(() =>
      expect(String(toastErrorFn.mock.calls[0]?.[0])).toContain('Failed to stop'),
    )
  })

  it('ignores a second stop click while one is in flight (anti double-click)', async () => {
    let resolveStop!: (v: boolean) => void
    cancelBackgroundTask.mockImplementation(
      () => new Promise<boolean>(res => { resolveStop = res }),
    )
    catalogBackgroundTasks.mockReturnValue([makeTask()])
    render(<BackgroundTasksPanel />)
    const stop = screen.getByRole('button', { name: 'Stop background task' })
    fireEvent.click(stop)
    await waitFor(() => expect(cancelBackgroundTask).toHaveBeenCalledTimes(1))
    expect(stop).toBeDisabled()
    fireEvent.click(stop)
    expect(cancelBackgroundTask).toHaveBeenCalledTimes(1)
    resolveStop(true)
  })

  it('formatElapsed renders m:ss and h:mm:ss', () => {
    const start = 1_700_000_000_000
    expect(formatElapsed(start, start + 5_000)).toBe('0:05')
    expect(formatElapsed(start, start + 65_000)).toBe('1:05')
    expect(formatElapsed(start, start + 3_723_000)).toBe('1:02:03')
    // Clock skew never produces negative output.
    expect(formatElapsed(start, start - 10_000)).toBe('0:00')
  })

  it('runningBackgroundTasks filters the in-flight slice', () => {
    const tasks = [
      makeTask({ status: 'running' }),
      makeTask({ status: 'completed', completed_at: 1 }),
      makeTask({ status: 'cancelled', completed_at: 2 }),
      makeTask({ status: 'failed', completed_at: 3 }),
    ]
    expect(runningBackgroundTasks(tasks)).toHaveLength(1)
    expect(terminalBackgroundTasks(tasks)).toHaveLength(3)
  })
})

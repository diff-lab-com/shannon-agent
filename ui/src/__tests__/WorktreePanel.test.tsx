import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import WorktreePanel from '@/components/tasks/WorktreePanel'

const listTaskWorktrees = vi.hoisted(() => vi.fn())
const createTaskWorktree = vi.hoisted(() => vi.fn())
const removeTaskWorktree = vi.hoisted(() => vi.fn())
const pruneTaskWorktrees = vi.hoisted(() => vi.fn())
const listScheduledTasks = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listTaskWorktrees: (...args: unknown[]) => listTaskWorktrees(...args),
  createTaskWorktree: (...args: unknown[]) => createTaskWorktree(...args),
  removeTaskWorktree: (...args: unknown[]) => removeTaskWorktree(...args),
  pruneTaskWorktrees: (...args: unknown[]) => pruneTaskWorktrees(...args),
  listScheduledTasks: (...args: unknown[]) => listScheduledTasks(...args),
}))

const sampleWorktree = {
  task_id: 'task-1',
  task_name: 'Daily Standup',
  path: '/repo/.shannon/worktrees/daily-standup',
  branch: 'routine/daily-standup',
}

const sampleRoutine = {
  id: 'task-1',
  name: 'Daily Standup',
  trigger_type: 'interval',
  enabled: true,
  prompt: 'Run standup',
  interval_seconds: 86400,
  created_at: 1717000000,
  updated_at: 1717000000,
  last_fire_at: null,
  next_fire_at: 1717000060,
  exec_count: 0,
  error_count: 0,
  last_error: null,
  depends_on: [],
  assignee: null,
  priority: null,
}

beforeEach(() => {
  listTaskWorktrees.mockReset()
  createTaskWorktree.mockReset()
  removeTaskWorktree.mockReset()
  pruneTaskWorktrees.mockReset()
  listScheduledTasks.mockReset()
  listScheduledTasks.mockResolvedValue([])
})

describe('WorktreePanel', () => {
  it('renders loading skeletons initially', async () => {
    listTaskWorktrees.mockReturnValue(new Promise(() => {}))
    render(<WorktreePanel />)
    expect(screen.queryByText('Daily Standup')).not.toBeInTheDocument()
  })

  it('renders empty state when no worktrees', async () => {
    listTaskWorktrees.mockResolvedValue([])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText(/No task worktrees yet/)).toBeInTheDocument())
  })

  it('renders error banner when fetch fails', async () => {
    listTaskWorktrees.mockRejectedValue(new Error('Disk full'))
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Disk full')).toBeInTheDocument())
  })

  it('renders worktree rows with task name and branch', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    expect(screen.getByText('routine/daily-standup')).toBeInTheDocument()
  })

  it('disables Prune button when list is empty', async () => {
    listTaskWorktrees.mockResolvedValue([])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText(/No task worktrees/)).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /Prune stale/i })).toBeDisabled()
  })

  it('disables Create button when no routine selected', async () => {
    listTaskWorktrees.mockResolvedValue([])
    listScheduledTasks.mockResolvedValue([sampleRoutine])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText(/Select a routine/)).toBeInTheDocument())
    expect(screen.getByRole('button', { name: /Create worktree/i })).toBeDisabled()
  })

  it('calls create when routine selected and Create clicked', async () => {
    listTaskWorktrees.mockResolvedValue([])
    listScheduledTasks.mockResolvedValue([sampleRoutine])
    createTaskWorktree.mockResolvedValue(sampleWorktree)
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByRole('combobox')).toBeInTheDocument())
    fireEvent.change(screen.getByRole('combobox'), { target: { value: 'task-1' } })
    fireEvent.click(screen.getByRole('button', { name: /Create worktree/i }))
    await waitFor(() => expect(createTaskWorktree).toHaveBeenCalledWith('task-1'))
  })

  it('shows inline Remove/Cancel buttons on delete click', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Remove worktree for Daily Standup/i }))
    expect(screen.getByRole('button', { name: /Confirm remove worktree/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Cancel remove/i })).toBeInTheDocument()
  })

  it('calls remove with path on confirm', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    removeTaskWorktree.mockResolvedValue(undefined)
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Remove worktree for Daily Standup/i }))
    fireEvent.click(screen.getByRole('button', { name: /Confirm remove worktree/i }))
    await waitFor(() => expect(removeTaskWorktree).toHaveBeenCalledWith(sampleWorktree.path))
  })

  it('clears confirm UI on Cancel click', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Remove worktree for Daily Standup/i }))
    fireEvent.click(screen.getByRole('button', { name: /Cancel remove/i }))
    expect(screen.queryByRole('button', { name: /Confirm remove worktree/i })).not.toBeInTheDocument()
  })

  it('calls prune when Prune stale clicked', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    pruneTaskWorktrees.mockResolvedValue([sampleWorktree.path])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText('Daily Standup')).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: /Prune stale/i }))
    await waitFor(() => expect(pruneTaskWorktrees).toHaveBeenCalled())
  })

  it('renders trigger type suffix when routine matched', async () => {
    listTaskWorktrees.mockResolvedValue([sampleWorktree])
    listScheduledTasks.mockResolvedValue([sampleRoutine])
    render(<WorktreePanel />)
    await waitFor(() => expect(screen.getByText(/interval/)).toBeInTheDocument())
  })
})

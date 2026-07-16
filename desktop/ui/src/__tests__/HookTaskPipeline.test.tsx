// P3.3 HookTaskPipeline tests.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import HookTaskPipeline from '@/components/tasks/HookTaskPipeline'
import type { TriggeredRoutineDto } from '@/types'

const listTriggeredRoutines = vi.hoisted(() => vi.fn())
const toggleTriggeredRoutine = vi.hoisted(() => vi.fn())
const createTriggeredRoutine = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  listTriggeredRoutines: (...args: unknown[]) => listTriggeredRoutines(...args),
  toggleTriggeredRoutine: (...args: unknown[]) => toggleTriggeredRoutine(...args),
  createTriggeredRoutine: (...args: unknown[]) => createTriggeredRoutine(...args),
}))

const sample: TriggeredRoutineDto[] = [
  { name: 'auto-lint', trigger: 'PostToolUse', matcher: 'Edit', command: 'just lint', enabled: true, description: 'Lint after edits' },
  { name: 'auto-test', trigger: 'preToolUse', matcher: 'Bash', command: 'cargo check', enabled: false },
  { name: 'notify-release', trigger: 'TaskCompleted', pattern: 'release-.*', command: 'slack-notify', enabled: true },
]

beforeEach(() => {
  listTriggeredRoutines.mockReset()
  toggleTriggeredRoutine.mockReset()
  createTriggeredRoutine.mockReset()
  listTriggeredRoutines.mockResolvedValue(sample)
  toggleTriggeredRoutine.mockResolvedValue(true)
  createTriggeredRoutine.mockResolvedValue(sample[0])
})

describe('HookTaskPipeline', () => {
  it('renders a row per routine on load', async () => {
    render(<HookTaskPipeline />)
    await waitFor(() => expect(listTriggeredRoutines).toHaveBeenCalledTimes(1))
    expect(await screen.findByText('auto-lint')).toBeInTheDocument()
    expect(screen.getByText('auto-test')).toBeInTheDocument()
    expect(screen.getByText('notify-release')).toBeInTheDocument()
  })

  it('shows the active/total count badge', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    expect(screen.getByText('2/3 active')).toBeInTheDocument()
  })

  it('shows the trigger badge text', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    expect(screen.getByText('PostToolUse')).toBeInTheDocument()
    expect(screen.getByText('preToolUse')).toBeInTheDocument()
  })

  it('shows matcher and pattern chips when present', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    expect(screen.getByText('Edit')).toBeInTheDocument()
    expect(screen.getByText('~/release-.*/')).toBeInTheDocument()
  })

  it('shows empty state when no routines returned', async () => {
    listTriggeredRoutines.mockResolvedValue([])
    render(<HookTaskPipeline />)
    await waitFor(() => expect(listTriggeredRoutines).toHaveBeenCalled())
    expect(await screen.findByText(/No hook-driven routines registered/i)).toBeInTheDocument()
  })

  it('shows error when load fails', async () => {
    listTriggeredRoutines.mockRejectedValue(new Error('boom'))
    render(<HookTaskPipeline />)
    await waitFor(() => expect(listTriggeredRoutines).toHaveBeenCalled())
    expect(await screen.findByText(/boom/)).toBeInTheDocument()
  })

  it('toggles a routine off via the switch', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    const toggle = screen.getByLabelText('Toggle auto-lint')
    expect(toggle).toHaveAttribute('aria-checked', 'true')
    fireEvent.click(toggle)
    await waitFor(() => expect(toggleTriggeredRoutine).toHaveBeenCalledWith('auto-lint', false))
  })

  it('toggles a routine on', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-test')
    const toggle = screen.getByLabelText('Toggle auto-test')
    expect(toggle).toHaveAttribute('aria-checked', 'false')
    fireEvent.click(toggle)
    await waitFor(() => expect(toggleTriggeredRoutine).toHaveBeenCalledWith('auto-test', true))
  })

  it('refresh button re-fetches the list', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    fireEvent.click(screen.getByText('Refresh'))
    await waitFor(() => expect(listTriggeredRoutines).toHaveBeenCalledTimes(2))
  })

  it('Add button opens the create dialog', async () => {
    render(<HookTaskPipeline />)
    await screen.findByText('auto-lint')
    fireEvent.click(screen.getByLabelText('Create new hook routine'))
    expect(await screen.findByText(/New Hook → Task Routine/i)).toBeInTheDocument()
  })
})

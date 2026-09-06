// Tests for the P0-2 Tasks-page goal run cards: rendering of status and
// counters, and action dispatch (pause/resume/stop) + objective rewrite.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import GoalRunPanel, { GoalRunCard } from '@/components/tasks/GoalRunPanel'
import * as api from '@/lib/tauri-api'
import type { GoalRunDto } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  listGoalRuns: vi.fn(),
  startGoalRun: vi.fn(),
  pauseGoalRun: vi.fn(),
  resumeGoalRun: vi.fn(),
  stopGoalRun: vi.fn(),
  updateGoalObjective: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

function makeRun(o: Partial<GoalRunDto>): GoalRunDto {
  return {
    sessionId: 'sess-goal-1',
    title: 'Harden the upload pipeline',
    objective: 'Add retry + tests to the upload pipeline',
    status: 'running',
    iterations: 3,
    maxTurns: 12,
    spentUsd: 0.42,
    budgetUsd: 5,
    stallStrikes: 0,
    lastError: null,
    startedAtMs: 1_700_000_000_000,
    updatedAtMs: 1_700_000_000_000,
    ...o,
  }
}

describe('GoalRunCard', () => {
  const noop = () => {}

  it('renders title, objective, counters and status badge', () => {
    render(
      <GoalRunCard run={makeRun({})} onPause={noop} onResume={noop} onStop={noop}
        onUpdateObjective={vi.fn()} onViewSession={noop} />,
      { wrapper },
    )
    expect(screen.getByText('Harden the upload pipeline')).toBeTruthy()
    expect(screen.getByText(/Add retry \+ tests/)).toBeTruthy()
    // iterations / maxTurns pair
    expect(screen.getByText(/3/)).toBeTruthy()
    expect(screen.getByText(/12/)).toBeTruthy()
    // running badge
    expect(screen.getByText(/running|运行中/i)).toBeTruthy()
    // running → pause + stop available, no resume
    expect(screen.getByRole('button', { name: /pause the goal run|暂停目标运行/i })).toBeTruthy()
    expect(screen.getByRole('button', { name: /stop the goal run|停止目标运行/i })).toBeTruthy()
    expect(screen.queryByRole('button', { name: /resume the goal run|继续目标运行/i })).toBeNull()
  })

  it('shows paused actions and the last error note', () => {
    render(
      <GoalRunCard run={makeRun({ status: 'paused', lastError: 'Reached stall-strike budget' })}
        onPause={noop} onResume={noop} onStop={noop}
        onUpdateObjective={vi.fn()} onViewSession={noop} />,
      { wrapper },
    )
    expect(screen.getByRole('button', { name: /resume the goal run|继续目标运行/i })).toBeTruthy()
    expect(screen.getByRole('note').textContent).toContain('stall-strike')
  })

  it('dispatches stop with the run session id', () => {
    const onStop = vi.fn()
    render(
      <GoalRunCard run={makeRun({})} onPause={noop} onResume={noop} onStop={onStop}
        onUpdateObjective={vi.fn()} onViewSession={noop} />,
      { wrapper },
    )
    fireEvent.click(screen.getByRole('button', { name: /stop the goal run|停止目标运行/i }))
    expect(onStop).toHaveBeenCalledWith('sess-goal-1')
  })

  it('rewrites the objective through the inline editor', async () => {
    const onUpdateObjective = vi.fn().mockResolvedValue(true)
    render(
      <GoalRunCard run={makeRun({})} onPause={noop} onResume={noop} onStop={noop}
        onUpdateObjective={onUpdateObjective} onViewSession={noop} />,
      { wrapper },
    )
    fireEvent.click(screen.getByRole('button', { name: /edit objective|改写目标/i }))
    const input = screen.getByRole('textbox', { name: /edit objective|改写目标/i }) as HTMLInputElement
    fireEvent.change(input, { target: { value: 'Rewritten objective' } })
    fireEvent.keyDown(input, { key: 'Enter' })
    await waitFor(() => expect(onUpdateObjective).toHaveBeenCalledWith('sess-goal-1', 'Rewritten objective'))
  })
})

describe('GoalRunPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('lists runs from the backend and refreshes', async () => {
    vi.mocked(api.listGoalRuns).mockResolvedValue([
      makeRun({}),
      makeRun({ sessionId: 'sess-goal-2', status: 'interrupted', maxTurns: null, budgetUsd: null }),
    ])
    render(<GoalRunPanel onViewSession={() => {}} />, { wrapper })
    await waitFor(() => expect(screen.getAllByTestId('goal-run-card').length).toBe(2))
    expect(screen.getByText(/interrupted|已中断/i)).toBeTruthy()
  })

  it('renders nothing without runs (empty state stays on the task list)', async () => {
    vi.mocked(api.listGoalRuns).mockResolvedValue([])
    const { container } = render(<GoalRunPanel onViewSession={() => {}} />, { wrapper })
    await waitFor(() => expect(api.listGoalRuns).toHaveBeenCalled())
    expect(container.querySelector('[data-testid="goal-run-panel"]')).toBeNull()
  })
})

// MissionCard — 缓期批 2 使命卡. Pins the honesty contract on the render
// side: absent fields render nothing (no budget → no chip, no
// budget_used_usd → no usage line), a dead link renders 「任务不存在」
// instead of invented state, a past deadline gets the muted overdue
// marker, and the editor mirrors the backend's configure('mission')
// validation before issuing the JSON payload.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import MissionCard from '@/components/opc/MissionCard'
import * as api from '@/lib/tauri-api'
import type { MissionConfig, MissionProgress, TaskItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  configure: vi.fn().mockResolvedValue(undefined),
  missionProgress: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn() },
}))

import { toast } from 'sonner'

// 2027-03-01 (safely future), 2020-01-01 (safely past).
const FUTURE_TS = new Date('2027-03-01').getTime()
const PAST_TS = new Date('2020-01-01').getTime()
const fullMission: MissionConfig = {
  name: 'Launch Shannon v2',
  budget_usd: 120,
  deadline_ts: FUTURE_TS,
  task_ids: ['t1', 't2', 'ghost'],
}

const fullProgress: MissionProgress = {
  name: 'Launch Shannon v2',
  budget_usd: 120,
  deadline_ts: FUTURE_TS,
  budget_used_usd: 30,
  tasks: [
    { task_id: 't1', found: true, title: 'Design review', status: 'in_progress', cost_usd: 12.5 },
    { task_id: 't2', found: true, title: 'Write docs', status: 'completed' },
    { task_id: 'ghost', found: false },
  ],
}

const boardTasks: TaskItem[] = [
  { id: 't1', title: 'Design review', status: 'in_progress' },
  { id: 't9', title: 'New task', status: 'pending' },
]

const refreshConfig = vi.fn().mockResolvedValue(undefined)

function renderCard(mission: MissionConfig | null = fullMission, tasks: TaskItem[] = boardTasks) {
  return render(<MissionCard mission={mission} tasks={tasks} refreshConfig={refreshConfig} />)
}

beforeEach(() => {
  vi.mocked(api.missionProgress).mockReset()
  vi.mocked(api.configure).mockReset().mockResolvedValue(undefined)
  refreshConfig.mockClear()
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.error).mockClear()
})

describe('MissionCard visibility', () => {
  it('renders nothing when no mission is configured', () => {
    renderCard(null)
    expect(screen.queryByTestId('mission-card')).not.toBeInTheDocument()
    expect(api.missionProgress).not.toHaveBeenCalled()
  })

  it('renders the mission name and queries progress when configured', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    expect(screen.getByTestId('mission-card')).toBeInTheDocument()
    expect(screen.getByText('Launch Shannon v2')).toBeInTheDocument()
    await waitFor(() => expect(api.missionProgress).toHaveBeenCalled())
    expect(screen.getByTestId('mission-task-row-t1')).toBeInTheDocument()
  })
})

describe('MissionCard honesty contract', () => {
  it('renders budget chip, deadline and budget usage when all present', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    // Budget chip — two-decimal USD like TaskCard's cost chip.
    expect(screen.getByTestId('mission-budget')).toHaveTextContent('$120.00')
    // Deadline shows a localized date (year at minimum).
    expect(screen.getByTestId('mission-deadline').textContent).toMatch(/2027/)
    // Usage line: $30.00 of $120.00 = 25%.
    await waitFor(() => expect(screen.getByTestId('mission-budget-usage')).toBeInTheDocument())
    expect(screen.getByTestId('mission-budget-usage')).toHaveTextContent('$30.00')
    expect(screen.getByTestId('mission-budget-usage')).toHaveTextContent('$120.00')
    expect(screen.getByTestId('mission-budget-usage')).toHaveTextContent('25%')
  })

  it('hides budget chip and usage line when the mission has no budget', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue({
      ...fullProgress,
      budget_usd: undefined,
      budget_used_usd: 30,
    })
    renderCard({ ...fullMission, budget_usd: undefined })
    expect(screen.queryByTestId('mission-budget')).not.toBeInTheDocument()
    await waitFor(() => expect(screen.getByTestId('mission-task-row-t1')).toBeInTheDocument())
    expect(screen.queryByTestId('mission-budget-usage')).not.toBeInTheDocument()
  })

  it('hides the usage line entirely when budget_used_usd is absent', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue({ ...fullProgress, budget_used_usd: undefined })
    renderCard()
    await waitFor(() => expect(screen.getByTestId('mission-task-row-t1')).toBeInTheDocument())
    expect(screen.queryByTestId('mission-budget-usage')).not.toBeInTheDocument()
  })

  it('marks a past deadline as overdue and never a future one', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    await waitFor(() => expect(screen.getByTestId('mission-task-row-t1')).toBeInTheDocument())
    expect(screen.queryByTestId('mission-overdue')).not.toBeInTheDocument()
  })

  it('renders the overdue marker for a deadline in the past', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue({ ...fullProgress, deadline_ts: PAST_TS })
    renderCard({ ...fullMission, deadline_ts: PAST_TS })
    await waitFor(() => expect(screen.getByTestId('mission-overdue')).toBeInTheDocument())
    expect(screen.getByTestId('mission-overdue')).toHaveTextContent('Overdue')
  })

  it('renders a found:false link as "Task not found", never invented state', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    await waitFor(() => expect(screen.getByTestId('mission-task-row-ghost')).toBeInTheDocument())
    expect(screen.getByTestId('mission-task-row-ghost')).toHaveTextContent('Task not found')
    // The dead link still shows which id it was.
    expect(screen.getByTestId('mission-task-row-ghost')).toHaveTextContent('ghost')
  })

  it('omits cost and status when the row does not carry them', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    await waitFor(() => expect(screen.getByTestId('mission-task-row-t2')).toBeInTheDocument())
    // t2 has a title + status but no cost_usd — no $ figure in its row.
    expect(screen.getByTestId('mission-task-row-t2')).toHaveTextContent('Write docs')
    expect(screen.getByTestId('mission-task-row-t2').textContent).not.toMatch(/\$/)
  })

  it('shows an honest error line when the projection query fails', async () => {
    vi.mocked(api.missionProgress).mockRejectedValue(new Error('boom'))
    renderCard()
    await waitFor(() => expect(screen.getByTestId('mission-progress-error')).toBeInTheDocument())
    expect(screen.queryByTestId('mission-task-row-t1')).not.toBeInTheDocument()
  })
})

describe('MissionCard editor', () => {
  it('opens with the current values and board checkboxes reflect links', () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    expect(screen.getByTestId('mission-edit-name')).toHaveValue('Launch Shannon v2')
    expect(screen.getByTestId('mission-edit-budget')).toHaveValue(120)
    expect(screen.getByTestId('mission-edit-task-t1')).toBeChecked()
    expect(screen.getByTestId('mission-edit-task-t9')).not.toBeChecked()
  })

  it('blocks save and shows an inline error for an empty name', () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.change(screen.getByTestId('mission-edit-name'), { target: { value: '   ' } })
    expect(screen.getByRole('alert')).toHaveTextContent('Enter a mission name')
    expect(screen.getByTestId('mission-edit-save')).toBeDisabled()
  })

  it('shows an inline error for a negative budget', () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.change(screen.getByTestId('mission-edit-budget'), { target: { value: '-5' } })
    expect(screen.getByRole('alert')).toHaveTextContent('Budget must be a number of at least 0')
    expect(screen.getByTestId('mission-edit-save')).toBeDisabled()
  })

  it('saves the mission as the configure(\'mission\') JSON payload', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.click(screen.getByTestId('mission-edit-task-t9')) // link the new task
    fireEvent.click(screen.getByTestId('mission-edit-save'))
    await waitFor(() => expect(api.configure).toHaveBeenCalled())
    expect(api.configure).toHaveBeenCalledWith({
      key: 'mission',
      value: JSON.stringify({
        name: 'Launch Shannon v2',
        budget_usd: 120,
        deadline_ts: FUTURE_TS,
        task_ids: ['t1', 't2', 'ghost', 't9'],
      }),
    })
    await waitFor(() => expect(refreshConfig).toHaveBeenCalled())
    expect(toast.success).toHaveBeenCalled()
  })

  it('unlinks a board task and a dead off-board link through the editor', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.click(screen.getByTestId('mission-edit-task-t1'))
    fireEvent.click(screen.getByTestId('mission-edit-unlink-ghost'))
    fireEvent.click(screen.getByTestId('mission-edit-save'))
    await waitFor(() => expect(api.configure).toHaveBeenCalled())
    const payload = JSON.parse(vi.mocked(api.configure).mock.calls[0][0].value) as MissionConfig
    expect(payload.task_ids).toEqual(['t2'])
  })

  it('omits optional keys from the payload when cleared', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.change(screen.getByTestId('mission-edit-budget'), { target: { value: '' } })
    fireEvent.change(screen.getByTestId('mission-edit-deadline'), { target: { value: '' } })
    fireEvent.click(screen.getByTestId('mission-edit-save'))
    await waitFor(() => expect(api.configure).toHaveBeenCalled())
    const payload = JSON.parse(vi.mocked(api.configure).mock.calls[0][0].value) as MissionConfig
    expect(payload).toEqual({ name: 'Launch Shannon v2', task_ids: ['t1', 't2', 'ghost'] })
    expect('budget_usd' in payload).toBe(false)
    expect('deadline_ts' in payload).toBe(false)
  })

  it('shows an honest error toast when configure fails', async () => {
    vi.mocked(api.missionProgress).mockResolvedValue(fullProgress)
    vi.mocked(api.configure).mockRejectedValue(new Error('rejected by backend'))
    renderCard()
    fireEvent.click(screen.getByTestId('mission-edit-toggle'))
    fireEvent.click(screen.getByTestId('mission-edit-save'))
    await waitFor(() => expect(api.configure).toHaveBeenCalled())
    // The failure surfaces through the toastError path with the cause.
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(screen.getByTestId('mission-edit-form')).toBeInTheDocument()
  })
})

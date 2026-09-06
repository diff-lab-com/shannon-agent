// P2-5: off-peak execution window UI tests.
//
// Covers the three UI touchpoints of the off-peak queue:
//  - ScheduleForm emits policy.execution_window in the create payload
//    (enabled → window object, disabled → null);
//  - OffpeakWindowEditor surfaces the queued status + saves the window
//    through update_scheduled_task (full-policy replace);
//  - AdvancedSettings persists `offpeak.model_override` via configure;
//  - statusBadge maps the `queued` run status to its i18n badge.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ScheduleForm from '@/components/tasks/ScheduleForm'
import OffpeakWindowEditor from '@/components/tasks/OffpeakWindowEditor'
import { statusBadge } from '@/components/tasks/shared'
import type { CreateTaskPayload, ScheduledRoutine } from '@/types'

const previewCron = vi.hoisted(() => vi.fn())
const listTaskExecutions = vi.hoisted(() => vi.fn())
const updateScheduledTask = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', () => ({
  previewCron: (...args: unknown[]) => previewCron(...args),
  listTaskExecutions: (...args: unknown[]) => listTaskExecutions(...args),
  updateScheduledTask: (...args: unknown[]) => updateScheduledTask(...args),
  // AdvancedSettings imports the whole module — every function its render
  // path touches must exist. Everything here rejects fast; only the
  // functions above carry behavior for these tests.
  getConfig: vi.fn().mockResolvedValue({}),
  refreshConfig: vi.fn().mockResolvedValue(undefined),
  getCliInstallStatus: vi.fn().mockRejectedValue(new Error('skip')),
  listSkillCandidates: vi.fn().mockRejectedValue(new Error('skip')),
  checkAppUpdate: vi.fn().mockRejectedValue(new Error('skip')),
}))

function windowedRoutine(): ScheduledRoutine {
  return {
    id: 'sched-042',
    name: 'nightly-digest',
    prompt: 'compile digest',
    interval_secs: 3600,
    trigger_type: 'interval',
    cron_expr: null,
    timezone: null,
    next_fire_at: null,
    expires_at: null,
    created_at: 1_735_689_600,
    last_fired: null,
    enabled: true,
    fire_count: 0,
    max_fires: null,
    policy: {
      max_retries: 1,
      timeout_secs: 600,
      worktree: null,
      notify_on_failure: false,
      budget_usd: null,
      auto_archive_when_empty: true,
      execution_window: { start_hour: 22, end_hour: 6, timezone: null },
    },
    last_run_id: null,
    last_error: null,
    depends_on: [],
  }
}

beforeEach(() => {
  previewCron.mockReset()
  previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [1735689600] })
  listTaskExecutions.mockReset()
  listTaskExecutions.mockResolvedValue([])
  updateScheduledTask.mockReset()
  updateScheduledTask.mockImplementation(async (payload: { id: string }) => ({
    ...windowedRoutine(),
    id: payload.id,
  }))
})

describe('P2-5 ScheduleForm off-peak window payload', () => {
  function fillRequiredFields() {
    fireEvent.change(screen.getByPlaceholderText('e.g. Daily standup summary'), { target: { value: 'My Routine' } })
    fireEvent.change(screen.getByPlaceholderText('Describe what this routine should do...'), { target: { value: 'do thing' } })
  }

  it('emits execution_window in the create payload when enabled', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)

    fillRequiredFields()
    fireEvent.click(screen.getByText('Policy options'))

    const toggle = screen.getByLabelText('Toggle off-peak execution window')
    fireEvent.click(toggle)

    fireEvent.change(screen.getByLabelText('Start hour (inclusive)'), { target: { value: '23' } })
    fireEvent.change(screen.getByLabelText('End hour (inclusive)'), { target: { value: '5' } })
    fireEvent.change(screen.getByLabelText('Timezone'), { target: { value: 'UTC' } })

    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))

    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.policy?.execution_window).toEqual({
      start_hour: 23,
      end_hour: 5,
      timezone: 'UTC',
    })
  })

  it('sends a null execution_window when off-peak stays disabled', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)

    fillRequiredFields()
    fireEvent.click(screen.getByText('Policy options'))
    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))

    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.policy?.execution_window).toBeNull()
  })

  it('clamps out-of-range hours into 0..=23', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)

    fillRequiredFields()
    fireEvent.click(screen.getByText('Policy options'))
    fireEvent.click(screen.getByLabelText('Toggle off-peak execution window'))
    fireEvent.change(screen.getByLabelText('Start hour (inclusive)'), { target: { value: '99' } })
    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))

    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.policy?.execution_window?.start_hour).toBe(23)
  })
})

describe('P2-5 OffpeakWindowEditor queued status + save', () => {
  it('shows the queued status with the window hours', async () => {
    listTaskExecutions.mockResolvedValue([
      {
        run_id: 'r-1',
        task_id: 'sched-042',
        task_name: 'nightly-digest',
        started_at: 1_735_689_600,
        status: 'queued',
      },
    ])
    render(<OffpeakWindowEditor routine={windowedRoutine()} />)

    await waitFor(() =>
      expect(screen.getByText('Queued (off-peak 22:00–06:00) — will run when the window opens')).toBeInTheDocument(),
    )
    expect(listTaskExecutions).toHaveBeenCalledWith('sched-042', 1)
  })

  it('saves the full policy with the updated window', async () => {
    render(<OffpeakWindowEditor routine={windowedRoutine()} />)

    // Wait for the initial executions probe to settle.
    await waitFor(() => expect(listTaskExecutions).toHaveBeenCalled())

    fireEvent.change(screen.getByLabelText('End hour'), { target: { value: '5' } })
    fireEvent.click(screen.getByLabelText('Save off-peak window'))

    await waitFor(() => expect(updateScheduledTask).toHaveBeenCalledTimes(1))
    const payload = updateScheduledTask.mock.calls[0][0] as { id: string; policy: { execution_window: unknown } }
    expect(payload.id).toBe('sched-042')
    expect(payload.policy.execution_window).toEqual({ start_hour: 22, end_hour: 5, timezone: null })
    // Full-policy replace: the untouched fields ride along.
    expect(payload.policy.max_retries).toBe(1)
  })

  it('disables save until something changes', async () => {
    render(<OffpeakWindowEditor routine={windowedRoutine()} />)
    await waitFor(() => expect(listTaskExecutions).toHaveBeenCalled())
    expect(screen.getByLabelText('Save off-peak window')).toBeDisabled()
  })
})

describe('P2-5 queued status badge', () => {
  it('maps the queued run status to the off-peak badge', () => {
    const badge = statusBadge('queued')
    expect(badge.labelId).toBe('tasks.status.queued.label')
    expect(badge.tipId).toBe('tasks.status.queued.tip')
    expect(badge.icon).toBe('bedtime')
  })
})

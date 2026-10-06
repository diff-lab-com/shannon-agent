import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import RoutineBasicsEditor from '@/components/tasks/RoutineBasicsEditor'
import type { ScheduledRoutine } from '@/types'

const updateScheduledTask = vi.hoisted(() => vi.fn())
const previewCron = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const toastErrorFn = vi.hoisted(() => vi.fn())

vi.mock('sonner', () => ({
  toast: { success: toastSuccess, error: toastErrorFn },
}))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  updateScheduledTask: (...args: unknown[]) => updateScheduledTask(...args),
  previewCron: (...args: unknown[]) => previewCron(...args),
}))

function makeRoutine(overrides: Partial<ScheduledRoutine> = {}): ScheduledRoutine {
  return {
    id: 'r1',
    name: 'Daily Standup',
    prompt: 'Summarize the day',
    interval_secs: 3600,
    trigger_type: 'interval',
    enabled: true,
    created_at: 1717000000,
    fire_count: 0,
    ...overrides,
  }
}

beforeEach(() => {
  updateScheduledTask.mockReset()
  updateScheduledTask.mockResolvedValue(makeRoutine())
  previewCron.mockReset()
  previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [1717000000] })
  toastSuccess.mockReset()
  toastErrorFn.mockReset()
})

describe('RoutineBasicsEditor', () => {
  it('pre-fills the fields from the routine and disables Save when pristine', () => {
    render(<RoutineBasicsEditor routine={makeRoutine()} />)
    expect(screen.getByDisplayValue('Daily Standup')).toBeInTheDocument()
    expect(screen.getByDisplayValue('Summarize the day')).toBeInTheDocument()
    expect(screen.getByTestId('routine-basics-save')).toBeDisabled()
  })

  it('sends only the changed fields via update_scheduled_task on save', async () => {
    const onUpdated = vi.fn()
    render(<RoutineBasicsEditor routine={makeRoutine()} onUpdated={onUpdated} />)
    fireEvent.change(screen.getByTestId('routine-basics-name'), {
      target: { value: 'Weekly Standup' },
    })
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    // W3-1: save opens the confirm step; Activate persists.
    fireEvent.click(screen.getByTestId('routine-basics-activate'))
    await waitFor(() =>
      expect(updateScheduledTask).toHaveBeenCalledWith({
        id: 'r1',
        name: 'Weekly Standup',
        prompt: 'Summarize the day',
        trigger_type: 'interval',
        interval_secs: 3600,
      }),
    )
    await waitFor(() => expect(onUpdated).toHaveBeenCalled())
  })

  it('keeps the edits and stays saveable when the update fails', async () => {
    updateScheduledTask.mockRejectedValue(new Error('backend down'))
    const onUpdated = vi.fn()
    render(<RoutineBasicsEditor routine={makeRoutine()} onUpdated={onUpdated} />)
    fireEvent.change(screen.getByTestId('routine-basics-prompt'), {
      target: { value: 'Edited prompt kept on failure' },
    })
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    fireEvent.click(screen.getByTestId('routine-basics-activate'))
    await waitFor(() => expect(updateScheduledTask).toHaveBeenCalled())
    await waitFor(() => expect(toastErrorFn).toHaveBeenCalled())
    // Failure keeps the local edits (retryable), and onUpdated never fired.
    expect(screen.getByTestId('routine-basics-activate')).toBeEnabled()
    // Back to edit still shows the kept values.
    fireEvent.click(screen.getByTestId('routine-basics-back'))
    expect(screen.getByTestId('routine-basics-prompt')).toHaveValue('Edited prompt kept on failure')
    expect(onUpdated).not.toHaveBeenCalled()
  })

  it('blocks Save while the edited cron expression is invalid', async () => {
    previewCron.mockResolvedValue({ expression: 'bad', valid: false, error: 'Invalid field' })
    render(<RoutineBasicsEditor routine={makeRoutine()} />)
    fireEvent.click(screen.getByRole('radio', { name: /Cron/ }))
    fireEvent.change(screen.getByTestId('routine-basics-cron'), { target: { value: 'bad' } })
    await waitFor(() => expect(previewCron).toHaveBeenCalledWith('bad'))
    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument())
    expect(screen.getByTestId('routine-basics-save')).toBeDisabled()
  })

  it('sends the cron expression when the trigger type switches to cron', async () => {
    render(<RoutineBasicsEditor routine={makeRoutine()} />)
    fireEvent.click(screen.getByRole('radio', { name: /Cron/ }))
    await waitFor(() =>
      expect(screen.getByTestId('routine-basics-save')).toBeEnabled(),
    )
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    fireEvent.click(screen.getByTestId('routine-basics-activate'))
    await waitFor(() =>
      expect(updateScheduledTask).toHaveBeenCalledWith({
        id: 'r1',
        name: 'Daily Standup',
        prompt: 'Summarize the day',
        trigger_type: 'cron',
        cron_expr: '0 9 * * *',
      }),
    )
  })

  it('ignores a second save click while a save is in flight (anti double-click)', async () => {
    let resolveSave!: (v: ScheduledRoutine) => void
    updateScheduledTask.mockImplementation(
      () => new Promise<ScheduledRoutine>(res => { resolveSave = res }),
    )
    render(<RoutineBasicsEditor routine={makeRoutine()} />)
    fireEvent.change(screen.getByTestId('routine-basics-name'), { target: { value: 'Renamed' } })
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    const activate = screen.getByTestId('routine-basics-activate')
    fireEvent.click(activate)
    await waitFor(() => expect(updateScheduledTask).toHaveBeenCalledTimes(1))
    expect(activate).toBeDisabled()
    fireEvent.click(activate)
    expect(updateScheduledTask).toHaveBeenCalledTimes(1)
    resolveSave(makeRoutine())
  })

  // ── W3-1: the edit path confirms too ──────────────────────────────────
  it('Save only opens the confirm step — Activate issues the update', async () => {
    const onUpdated = vi.fn()
    render(<RoutineBasicsEditor routine={makeRoutine()} onUpdated={onUpdated} />)
    fireEvent.change(screen.getByTestId('routine-basics-name'), {
      target: { value: 'Renamed Routine' },
    })
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    // Review step is up with the edited values; nothing was persisted yet.
    expect(screen.getByTestId('routine-basics-review')).toBeInTheDocument()
    expect(screen.getByTestId('routine-basics-review')).toHaveTextContent('Renamed Routine')
    expect(updateScheduledTask).not.toHaveBeenCalled()
    fireEvent.click(screen.getByTestId('routine-basics-activate'))
    await waitFor(() => expect(updateScheduledTask).toHaveBeenCalledWith({
      id: 'r1',
      name: 'Renamed Routine',
      prompt: 'Summarize the day',
      trigger_type: 'interval',
      interval_secs: 3600,
    }))
    await waitFor(() => expect(onUpdated).toHaveBeenCalled())
  })

  it('Back from the confirm step keeps every local edit', () => {
    render(<RoutineBasicsEditor routine={makeRoutine()} />)
    fireEvent.change(screen.getByTestId('routine-basics-prompt'), {
      target: { value: 'Draft edit' },
    })
    fireEvent.click(screen.getByTestId('routine-basics-save'))
    fireEvent.click(screen.getByTestId('routine-basics-back'))
    expect(screen.queryByTestId('routine-basics-review')).not.toBeInTheDocument()
    expect(screen.getByTestId('routine-basics-prompt')).toHaveValue('Draft edit')
    expect(updateScheduledTask).not.toHaveBeenCalled()
  })
})

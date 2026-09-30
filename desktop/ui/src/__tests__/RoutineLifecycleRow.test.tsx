import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import RoutineLifecycleRow from '@/components/tasks/RoutineLifecycleRow'
import type { ScheduledRoutine } from '@/types'

const toggleScheduledTask = vi.hoisted(() => vi.fn())
const deleteScheduledTask = vi.hoisted(() => vi.fn())
const toastSuccess = vi.hoisted(() => vi.fn())
const toastErrorFn = vi.hoisted(() => vi.fn())

vi.mock('sonner', () => ({
  toast: { success: toastSuccess, error: toastErrorFn },
}))

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  toggleScheduledTask: (...args: unknown[]) => toggleScheduledTask(...args),
  deleteScheduledTask: (...args: unknown[]) => deleteScheduledTask(...args),
}))

function makeRoutine(overrides: Partial<ScheduledRoutine> = {}): ScheduledRoutine {
  return {
    id: 'r1',
    name: 'Daily Standup',
    prompt: 'Summarize',
    interval_secs: 3600,
    trigger_type: 'interval',
    enabled: true,
    created_at: 1717000000,
    fire_count: 0,
    ...overrides,
  }
}

beforeEach(() => {
  toggleScheduledTask.mockReset()
  deleteScheduledTask.mockReset()
  toastSuccess.mockReset()
  toastErrorFn.mockReset()
})

describe('RoutineLifecycleRow', () => {
  it('renders the switch reflecting the persisted enabled state', () => {
    render(<RoutineLifecycleRow routine={makeRoutine()} />)
    const toggle = screen.getByTestId('routine-lifecycle-toggle') as HTMLInputElement
    expect(toggle.checked).toBe(true)
    expect(screen.getByText('Enabled')).toBeInTheDocument()
  })

  it('sends the EXPLICIT target state to toggle and fires onUpdated', async () => {
    toggleScheduledTask.mockResolvedValue(false)
    const onUpdated = vi.fn()
    render(<RoutineLifecycleRow routine={makeRoutine()} onUpdated={onUpdated} />)
    fireEvent.click(screen.getByTestId('routine-lifecycle-toggle'))
    await waitFor(() =>
      expect(toggleScheduledTask).toHaveBeenCalledWith('r1', false),
    )
    await waitFor(() => expect(onUpdated).toHaveBeenCalled())
  })

  it('keys the toast off the persisted bool, not the requested value', async () => {
    // User asked to disable, but the backend reports the routine is still
    // enabled (flip raced a concurrent change) — the toast must say enabled.
    toggleScheduledTask.mockResolvedValue(true)
    render(<RoutineLifecycleRow routine={makeRoutine()} />)
    fireEvent.click(screen.getByTestId('routine-lifecycle-toggle'))
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Task enabled'))
  })

  it('keeps the control usable and does not call onUpdated when toggle fails', async () => {
    toggleScheduledTask.mockRejectedValue(new Error('backend down'))
    const onUpdated = vi.fn()
    render(<RoutineLifecycleRow routine={makeRoutine()} onUpdated={onUpdated} />)
    fireEvent.click(screen.getByTestId('routine-lifecycle-toggle'))
    await waitFor(() => expect(toastErrorFn).toHaveBeenCalled())
    expect(String(toastErrorFn.mock.calls[0][0])).toContain('Failed to toggle')
    expect(onUpdated).not.toHaveBeenCalled()
  })

  it('ignores a second click while a toggle is in flight (anti double-click)', async () => {
    let resolveToggle!: (v: boolean) => void
    toggleScheduledTask.mockImplementation(
      () => new Promise<boolean>(res => { resolveToggle = res }),
    )
    render(<RoutineLifecycleRow routine={makeRoutine()} />)
    const toggle = screen.getByTestId('routine-lifecycle-toggle')
    fireEvent.click(toggle)
    await waitFor(() => expect(toggleScheduledTask).toHaveBeenCalledTimes(1))
    // Still in flight → the control is disabled and the extra click is a no-op.
    expect((screen.getByTestId('routine-lifecycle-toggle') as HTMLInputElement).disabled).toBe(true)
    fireEvent.click(toggle)
    expect(toggleScheduledTask).toHaveBeenCalledTimes(1)
    resolveToggle(false)
    await waitFor(() =>
      expect((screen.getByTestId('routine-lifecycle-toggle') as HTMLInputElement).disabled).toBe(false),
    )
  })

  it('deletes only after the destructive confirm, then closes', async () => {
    deleteScheduledTask.mockResolvedValue(true)
    const onUpdated = vi.fn()
    const onDeleted = vi.fn()
    render(
      <RoutineLifecycleRow routine={makeRoutine()} onUpdated={onUpdated} onDeleted={onDeleted} />,
    )
    // No confirm dialog content before the button is clicked.
    expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('routine-lifecycle-delete'))
    expect(await screen.findByRole('alertdialog')).toBeInTheDocument()
    expect(deleteScheduledTask).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Delete routine' }))
    await waitFor(() => expect(deleteScheduledTask).toHaveBeenCalledWith('r1'))
    await waitFor(() => expect(toastSuccess).toHaveBeenCalledWith('Task deleted'))
    await waitFor(() => expect(onUpdated).toHaveBeenCalled())
    await waitFor(() => expect(onDeleted).toHaveBeenCalled())
    // Dialog closed after success.
    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
  })

  it('keeps the routine (no close) when delete fails', async () => {
    deleteScheduledTask.mockRejectedValue(new Error('disk full'))
    const onUpdated = vi.fn()
    const onDeleted = vi.fn()
    render(
      <RoutineLifecycleRow routine={makeRoutine()} onUpdated={onUpdated} onDeleted={onDeleted} />,
    )
    fireEvent.click(screen.getByTestId('routine-lifecycle-delete'))
    fireEvent.click(await screen.findByRole('button', { name: 'Delete routine' }))
    await waitFor(() =>
      expect(String(toastErrorFn.mock.calls[0]?.[0])).toContain('Failed to delete'),
    )
    expect(onUpdated).not.toHaveBeenCalled()
    expect(onDeleted).not.toHaveBeenCalled()
  })

  it('ignores a second confirm while the delete is in flight (anti double-click)', async () => {
    let resolveDelete!: (v: boolean) => void
    deleteScheduledTask.mockImplementation(
      () => new Promise<boolean>(res => { resolveDelete = res }),
    )
    render(<RoutineLifecycleRow routine={makeRoutine()} />)
    fireEvent.click(screen.getByTestId('routine-lifecycle-delete'))
    const confirm = await screen.findByRole('button', { name: 'Delete routine' })
    fireEvent.click(confirm)
    await waitFor(() => expect(deleteScheduledTask).toHaveBeenCalledTimes(1))
    // In-flight: confirm shows the busy label and is disabled.
    const busyConfirm = screen.getByRole('button', { name: 'Deleting…' })
    expect(busyConfirm).toBeDisabled()
    fireEvent.click(busyConfirm)
    expect(deleteScheduledTask).toHaveBeenCalledTimes(1)
    resolveDelete(true)
  })
})

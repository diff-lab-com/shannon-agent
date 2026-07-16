import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ScheduleForm from '@/components/tasks/ScheduleForm'

const previewCron = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  previewCron: (...args: unknown[]) => previewCron(...args),
}))

beforeEach(() => {
  previewCron.mockReset()
  previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [1717000000] })
})

describe('ScheduleForm', () => {
  it('renders required name and prompt fields', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByPlaceholderText(/Daily standup/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText(/Describe what this routine/)).toBeInTheDocument()
  })

  it('disables Create Routine when required fields missing', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByRole('button', { name: /^Create Routine$/ })).toBeDisabled()
  })

  it('enables Create Routine when name + prompt + interval filled', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Standup' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run summary' } })
    expect(screen.getByRole('button', { name: /^Create Routine$/ })).toBeEnabled()
  })

  it('shows cron input when cron trigger selected', async () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('radio', { name: /Cron/ }))
    expect(screen.getByPlaceholderText('0 9 * * *')).toBeInTheDocument()
  })

  it('validates cron via previewCron API', async () => {
    previewCron.mockResolvedValue({ expression: 'bad', valid: false, error: 'Invalid field' })
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('radio', { name: /Cron/ }))
    fireEvent.change(screen.getByPlaceholderText('0 9 * * *'), { target: { value: 'bad' } })
    await waitFor(() => expect(previewCron).toHaveBeenCalledWith('bad'))
    await waitFor(() => expect(screen.getByText(/Invalid field/)).toBeInTheDocument())
  })

  it('shows webhook info banner when webhook selected', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('radio', { name: /Webhook/ }))
    expect(screen.getByText(/signing secret/i)).toBeInTheDocument()
  })

  it('shows event info banner when event selected', () => {
    render(<ScheduleForm onSubmit={() => {}} />)
    fireEvent.click(screen.getByRole('radio', { name: /Event/ }))
    expect(screen.getByText(/Event-driven triggers/i)).toBeInTheDocument()
  })

  it('reveals policy fields on toggle with max_retries default 2', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Policy options/ }))
    expect(screen.getByText(/Max retries/)).toBeInTheDocument()
    expect(screen.getByDisplayValue(2)).toBeInTheDocument()
    expect(screen.getByText(/Timeout/)).toBeInTheDocument()
    expect(screen.getByText(/Budget/)).toBeInTheDocument()
  })

  it('submit passes through payload with trigger_type interval', () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Daily' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run' } })
    fireEvent.click(screen.getByRole('button', { name: /^Create Routine$/ }))
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      name: 'Daily',
      prompt: 'Run',
      trigger_type: 'interval',
      interval_secs: 3600,
      policy: expect.objectContaining({ max_retries: 2, timeout_secs: 600 }),
    }))
  })

  it('submit with cron trigger includes cron_expr', async () => {
    const onSubmit = vi.fn()
    previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [] })
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Cron Task' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Go' } })
    fireEvent.click(screen.getByRole('radio', { name: /Cron/ }))
    fireEvent.change(screen.getByPlaceholderText('0 9 * * *'), { target: { value: '0 9 * * *' } })
    await waitFor(() => expect(previewCron).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: /^Create Routine$/ }))
    expect(onSubmit).toHaveBeenCalledWith(expect.objectContaining({
      trigger_type: 'cron',
      cron_expr: '0 9 * * *',
    }))
  })

  it('Cancel button resets and calls onCancel', () => {
    const onCancel = vi.fn()
    render(<ScheduleForm onSubmit={() => {}} onCancel={onCancel} />)
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }))
    expect(onCancel).toHaveBeenCalled()
  })

  it('toggles policy panel closed on second click', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Policy options/ }))
    expect(screen.getByText(/Max retries/)).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Hide policy/ }))
    expect(screen.queryByText(/Max retries/)).not.toBeInTheDocument()
  })

  it('updates max_retries when policy field changed', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByRole('button', { name: /Policy options/ }))
    const retryInput = screen.getByDisplayValue(2)
    fireEvent.change(retryInput, { target: { value: '5' } })
    expect(retryInput).toHaveValue(5)
  })
})

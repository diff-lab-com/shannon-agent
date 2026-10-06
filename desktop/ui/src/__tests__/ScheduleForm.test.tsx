import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ScheduleForm from '@/components/tasks/ScheduleForm'

const previewCron = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  previewCron: (...args: unknown[]) => previewCron(...args),
  // office B6'-ui: ScheduleForm probes the webhook config on mount.
  getWebhookConfig: vi.fn().mockResolvedValue(null),
  // P2-6/F6: filling the required fields mounts CostEstimateHint, whose
  // 350ms debounce calls estimateTaskCost — must exist or the timer throws
  // an unhandled error whenever a test outlives the debounce.
  estimateTaskCost: vi.fn().mockResolvedValue({ hasHistory: false, runsCounted: 0, minUsd: null, maxUsd: null, avgUsd: null, lastUsd: null }),
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

  it('disables Review when required fields missing', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    expect(screen.getByRole('button', { name: /^Review$/ })).toBeDisabled()
  })

  it('enables Review when name + prompt + interval filled', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Standup' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run summary' } })
    expect(screen.getByRole('button', { name: /^Review$/ })).toBeEnabled()
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
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Activate routine$/ }))
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
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Activate routine$/ }))
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

// ── W3-1: two-step review → activate ───────────────────────────────────────
describe('ScheduleForm review step (W3-1)', () => {
  it('does not create anything until Activate is pressed', () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Daily' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run' } })
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    // The structured preview is up, but no routine exists yet.
    expect(screen.getByTestId('schedule-review')).toBeInTheDocument()
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('Activate submits the exact configuration shown in the preview', () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Previewed' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run it' } })
    fireEvent.click(screen.getByRole('button', { name: /Policy options/ }))
    const retryInput = screen.getByDisplayValue(2)
    fireEvent.change(retryInput, { target: { value: '4' } })
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    // The preview carries the same name/prompt the payload will.
    expect(screen.getByTestId('schedule-review')).toHaveTextContent('Previewed')
    expect(screen.getByTestId('schedule-review')).toHaveTextContent('Run it')
    fireEvent.click(screen.getByRole('button', { name: /^Activate routine$/ }))
    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(onSubmit.mock.calls[0][0]).toMatchObject({
      name: 'Previewed',
      prompt: 'Run it',
      policy: expect.objectContaining({ max_retries: 4 }),
    })
  })

  it('Back to edit keeps every filled field', () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Kept' } })
    fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Still here' } })
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Back to edit$/ }))
    expect(screen.queryByTestId('schedule-review')).not.toBeInTheDocument()
    expect(screen.getByPlaceholderText(/Daily standup/)).toHaveValue('Kept')
    expect(screen.getByPlaceholderText(/Describe what this routine/)).toHaveValue('Still here')
    // And the kept state still flows through Review → Activate unchanged.
    fireEvent.click(screen.getByRole('button', { name: /^Review$/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Activate routine$/ }))
    expect(onSubmit.mock.calls[0][0]).toMatchObject({ name: 'Kept', prompt: 'Still here' })
  })
})

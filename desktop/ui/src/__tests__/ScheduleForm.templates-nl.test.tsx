// P3 ScheduleForm integration: templates pre-fill + NL cron parse.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ScheduleForm from '@/components/tasks/ScheduleForm'
import type { CreateTaskPayload } from '@/types'

const previewCron = vi.hoisted(() => vi.fn())
vi.mock('@/lib/tauri-api', () => ({
  previewCron: (...args: unknown[]) => previewCron(...args),
}))

beforeEach(() => {
  previewCron.mockReset()
  previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [1735689600] })
})

describe('ScheduleForm templates (P3.2)', () => {
  it('pre-fills form when template clicked', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('Daily Standup Summary'))
    expect(screen.getByPlaceholderText('e.g. Daily standup summary')).toHaveValue('Daily Standup')
    // Trigger switches to cron
    expect(screen.getByPlaceholderText('0 9 * * *')).toHaveValue('0 9 * * *')
  })

  it('template submit emits the cron payload', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('Nightly Test Suite'))
    // Wait for the async cron preview to load before submit (validity gate).
    await waitFor(() => expect(screen.getByText('Create Routine')).not.toBeDisabled())
    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.trigger_type).toBe('cron')
    expect(payload.cron_expr).toBe('0 2 * * *')
  })

  it('PR Auto-Review template sets interval', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('PR Auto-Review'))
    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.trigger_type).toBe('interval')
    expect(payload.interval_secs).toBe(6 * 3600)
  })
})

describe('ScheduleForm NL cron (P3.1)', () => {
  it('parses a natural-language phrase and fills cron field', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    // Switch to cron trigger
    fireEvent.click(screen.getByText('Cron'))
    const nl = screen.getByLabelText('Natural language cron input')
    fireEvent.change(nl, { target: { value: 'weekdays at 9am' } })
    fireEvent.click(screen.getByText('Parse'))
    await waitFor(() => {
      expect(screen.getByPlaceholderText('0 9 * * *')).toHaveValue('0 9 * * 1-5')
    })
    expect(screen.getByText(/Parsed:/)).toBeInTheDocument()
  })

  it('shows error for unparseable phrase', async () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('Cron'))
    fireEvent.change(screen.getByLabelText('Natural language cron input'), { target: { value: 'do the thing' } })
    fireEvent.click(screen.getByText('Parse'))
    expect(await screen.findByText(/Could not parse/i)).toBeInTheDocument()
  })

  it('Parse button is disabled until input is non-empty', () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('Cron'))
    expect(screen.getByText('Parse')).toBeDisabled()
  })

  it('Enter key triggers parse', async () => {
    render(<ScheduleForm onSubmit={() => {}} onCancel={() => {}} />)
    fireEvent.click(screen.getByText('Cron'))
    const nl = screen.getByLabelText('Natural language cron input')
    fireEvent.change(nl, { target: { value: 'midnight' } })
    fireEvent.keyDown(nl, { key: 'Enter' })
    await waitFor(() => {
      expect(screen.getByPlaceholderText('0 9 * * *')).toHaveValue('0 0 * * *')
    })
  })
})

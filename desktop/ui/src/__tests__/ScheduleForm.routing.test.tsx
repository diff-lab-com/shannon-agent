// P2.3 ScheduleForm: result_routing payload integration test.

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

describe('ScheduleForm result_routing (P2.3)', () => {
  it('adds an email channel and includes it in the submit payload', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)

    // Fill required fields
    fireEvent.change(screen.getByPlaceholderText('e.g. Daily standup summary'), { target: { value: 'My Routine' } })
    fireEvent.change(screen.getByPlaceholderText('Describe what this routine should do...'), { target: { value: 'do thing' } })

    // Open policy section (button toggles)
    fireEvent.click(screen.getByText('Policy options'))

    // Switch to email channel and add target
    await waitFor(() => expect(screen.getByLabelText('Channel type')).toBeInTheDocument())
    fireEvent.change(screen.getByLabelText('Channel type'), { target: { value: 'email' } })
    fireEvent.change(screen.getByLabelText('email target'), { target: { value: 'a@release.com' } })
    fireEvent.click(screen.getByText('Add'))

    // Submit
    fireEvent.click(screen.getByText('Create Routine'))

    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.policy?.result_routing).toEqual(['email:a@release.com'])
  })

  it('defaults result_routing to empty when no channels added', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.change(screen.getByPlaceholderText('e.g. Daily standup summary'), { target: { value: 'R' } })
    fireEvent.change(screen.getByPlaceholderText('Describe what this routine should do...'), { target: { value: 'p' } })
    fireEvent.click(screen.getByText('Create Routine'))
    await waitFor(() => expect(onSubmit).toHaveBeenCalled())
    const payload: CreateTaskPayload = onSubmit.mock.calls[0][0]
    expect(payload.policy?.result_routing).toEqual([])
  })
})

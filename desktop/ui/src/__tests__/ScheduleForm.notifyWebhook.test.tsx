// office Wave 2 B6'-ui — the notify_webhook checkbox: default off in the
// payload, explicit true when checked, and the "no webhook configured" hint
// gating on the getWebhookConfig probe.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import ScheduleForm from '@/components/tasks/ScheduleForm'
import type { CreateTaskPayload } from '@/types'

const previewCron = vi.hoisted(() => vi.fn())
const getWebhookConfig = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  previewCron: (...args: unknown[]) => previewCron(...args),
  getWebhookConfig: (...args: unknown[]) => getWebhookConfig(...args),
}))

beforeEach(() => {
  previewCron.mockReset()
  previewCron.mockResolvedValue({ expression: '0 9 * * *', valid: true, next_fires: [1717000000] })
  getWebhookConfig.mockReset().mockResolvedValue(null)
})

function fillRequired() {
  fireEvent.change(screen.getByPlaceholderText(/Daily standup/), { target: { value: 'Nightly' } })
  fireEvent.change(screen.getByPlaceholderText(/Describe what this routine/), { target: { value: 'Run' } })
}

async function submit(onSubmit: ReturnType<typeof vi.fn>) {
  fillRequired()
  fireEvent.click(screen.getByRole('button', { name: /^Create Routine$/ }))
  await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
  return onSubmit.mock.calls[0][0] as CreateTaskPayload
}

describe('ScheduleForm notify_webhook (B6\'-ui)', () => {
  it('defaults to notify_webhook: false in the create payload', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    const payload = await submit(onSubmit)
    expect(payload.notify_webhook).toBe(false)
  })

  it('sends notify_webhook: true when the checkbox is checked', async () => {
    const onSubmit = vi.fn()
    render(<ScheduleForm onSubmit={onSubmit} onCancel={() => {}} />)
    fireEvent.click(screen.getByTestId('notify-webhook-checkbox'))
    const payload = await submit(onSubmit)
    expect(payload.notify_webhook).toBe(true)
  })

  it('shows the "no webhook configured" hint only on a confirmed negative probe', async () => {
    render(<ScheduleForm onSubmit={vi.fn()} onCancel={() => {}} />)
    // While the probe is in flight nothing is claimed…
    expect(screen.queryByText(/No webhook configured/)).not.toBeInTheDocument()
    // …then the confirmed negative (getWebhookConfig → null) reveals it.
    await waitFor(() => expect(screen.getByText(/No webhook configured/)).toBeInTheDocument())
  })

  it('hides the hint when a webhook IS configured', async () => {
    getWebhookConfig.mockResolvedValue({ url: 'https://example.com/hook', template: 'custom', secret: null, timeout_ms: 5000, include_body: false })
    render(<ScheduleForm onSubmit={vi.fn()} onCancel={() => {}} />)
    await waitFor(() => expect(getWebhookConfig).toHaveBeenCalled())
    await waitFor(() => expect(screen.queryByText(/No webhook configured/)).not.toBeInTheDocument())
  })
})

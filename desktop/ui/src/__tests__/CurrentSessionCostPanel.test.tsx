// B2 / P1-9 — session budget save correctness in CurrentSessionCostPanel:
//   - the save is acknowledged (success toast) or explicitly failed (error
//     toast) — never a silent spinner;
//   - a session without a budget clears the input instead of carrying the
//     previous session's cap over;
//   - a non-positive / non-numeric entry is rejected client-side.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import CurrentSessionCostPanel from '@/components/usage/CurrentSessionCostPanel'
import { useSessions } from '@/context/SessionContext'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown } from '@/types'

vi.mock('@/context/SessionContext', () => ({
  useSessions: vi.fn(),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn(), info: vi.fn(), message: vi.fn() },
}))

const mockedUseSessions = vi.mocked(useSessions)
const mockedBreakdown: ContextBreakdown = {
  totalTokens: 1000,
  contextWindow: 200_000,
  categories: [
    { key: 'system', tokens: 200 },
    { key: 'conversation', tokens: 800 },
  ],
}

function renderPanel() {
  return render(<CurrentSessionCostPanel />)
}

beforeEach(() => {
  vi.clearAllMocks()
  mockedUseSessions.mockReturnValue({ currentSessionId: 's1' } as never)
  vi.mocked(api.getSessionContextBreakdown).mockResolvedValue(mockedBreakdown)
  vi.mocked(api.getSessionBudget).mockResolvedValue(7.5)
  vi.mocked(api.setSessionBudget).mockResolvedValue(undefined)
})

function budgetInput(): HTMLInputElement {
  return screen.getByPlaceholderText('No cap') as HTMLInputElement
}

async function renderLoaded() {
  renderPanel()
  await waitFor(() => expect(budgetInput()).toBeInTheDocument())
}

describe('CurrentSessionCostPanel — budget cap', () => {
  it('seeds the input from the session budget and toasts on save', async () => {
    await renderLoaded()
    expect(budgetInput().value).toBe('7.5')
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(api.setSessionBudget).toHaveBeenCalledWith('s1', 7.5))
    const { toast } = await import('sonner')
    expect(toast.success).toHaveBeenCalled()
  })

  it('clears the input when the new session has no budget (P1-9)', async () => {
    vi.mocked(api.getSessionBudget).mockResolvedValue(null)
    await renderLoaded()
    // The stale value from session A must not linger — one click would
    // otherwise write A's cap into B.
    expect(budgetInput().value).toBe('')
  })

  it('toasts a failure instead of dying in the finally block (P1-9)', async () => {
    const { toast } = await import('sonner')
    vi.mocked(api.setSessionBudget).mockRejectedValue('storage unavailable')
    await renderLoaded()
    fireEvent.change(budgetInput(), { target: { value: '5' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('rejects a non-positive amount client-side without an IPC write', async () => {
    const { toast } = await import('sonner')
    await renderLoaded()
    fireEvent.change(budgetInput(), { target: { value: '-2' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    expect(api.setSessionBudget).not.toHaveBeenCalled()
    expect(toast.error).toHaveBeenCalled()
  })

  it('clears the stored cap when saving an empty input', async () => {
    await renderLoaded()
    fireEvent.change(budgetInput(), { target: { value: '' } })
    fireEvent.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(api.setSessionBudget).toHaveBeenCalledWith('s1', null))
    expect(budgetInput().value).toBe('')
  })
})

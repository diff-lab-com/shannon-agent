// Tests for the P0-2 /goal composer entry: the slash registration resolves
// only a bare `/goal`, and the rendered start form submits the right
// start_goal_run payload (session passthrough, optional caps dropped when
// empty/invalid).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import GoalStartForm from '@/components/chat/GoalStartForm'
import { parseSlashInput, filterSlashCommands } from '@/lib/slash/commands'
import * as api from '@/lib/tauri-api'

vi.mock('@/lib/tauri-api', () => ({
  startGoalRun: vi.fn(),
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

describe('/goal slash registration', () => {
  it('resolves a bare /goal and the /goal autocomplete filter', () => {
    expect(parseSlashInput('/goal')?.name).toBe('goal')
    expect(parseSlashInput('/goal ship it')).toBeNull() // args go to the model
    expect(filterSlashCommands('goa').map(c => c.name)).toContain('goal')
  })
})

describe('GoalStartForm', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.startGoalRun).mockResolvedValue({ sessionId: 'sess-1' })
  })

  it('submits objective with the current session and drops empty caps', async () => {
    render(
      <GoalStartForm sessionId="sess-current" onDismiss={() => {}} />,
      { wrapper },
    )
    fireEvent.change(screen.getByPlaceholderText(/completion condition|描述完成条件/i), {
      target: { value: 'make CI green' },
    })
    fireEvent.change(screen.getByPlaceholderText(/flaky upload tests|不稳定的上传测试/i), {
      target: { value: 'CI hunter' },
    })
    fireEvent.click(screen.getByRole('button', { name: /start goal|启动目标/i }))

    await waitFor(() => expect(api.startGoalRun).toHaveBeenCalledTimes(1))
    expect(api.startGoalRun).toHaveBeenCalledWith({
      sessionId: 'sess-current',
      title: 'CI hunter',
      objective: 'make CI green',
      maxTurns: undefined,
      budgetUsd: undefined,
    })
    await waitFor(() => expect(screen.getByTestId('goal-start-success')).toBeTruthy())
  })

  it('passes numeric caps and falls back to null session', async () => {
    vi.mocked(api.startGoalRun).mockResolvedValue({ sessionId: 'new-sess' })
    render(<GoalStartForm sessionId={null} onDismiss={() => {}} />, { wrapper })

    const objective = screen.getByPlaceholderText(/completion condition|描述完成条件/i) as HTMLInputElement
    fireEvent.change(objective, { target: { value: 'write the changelog' } })
    const numericInputs = screen.getAllByRole('textbox') as HTMLInputElement[]
    // title + objective + two numeric inputs (inputMode numeric/decimal still render as textboxes)
    fireEvent.change(numericInputs[2], { target: { value: '8' } })
    fireEvent.change(numericInputs[3], { target: { value: '2.5' } })
    fireEvent.click(screen.getByRole('button', { name: /start goal|启动目标/i }))

    await waitFor(() => expect(api.startGoalRun).toHaveBeenCalledTimes(1))
    expect(api.startGoalRun).toHaveBeenCalledWith({
      sessionId: null,
      title: 'write the changelog',
      objective: 'write the changelog',
      maxTurns: 8,
      budgetUsd: 2.5,
    })
  })

  it('surfaces backend errors instead of starting', async () => {
    vi.mocked(api.startGoalRun).mockRejectedValue(new Error('a goal run is already active'))
    render(<GoalStartForm sessionId="sess-current" onDismiss={() => {}} />, { wrapper })

    const objective = screen.getByPlaceholderText(/completion condition|描述完成条件/i) as HTMLInputElement
    fireEvent.change(objective, { target: { value: 'another goal' } })
    fireEvent.click(screen.getByRole('button', { name: /start goal|启动目标/i }))

    await waitFor(() =>
      expect(screen.getByRole('alert').textContent).toContain('a goal run is already active'),
    )
    expect(screen.queryByTestId('goal-start-success')).toBeNull()
  })
})

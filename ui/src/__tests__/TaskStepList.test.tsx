import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, act } from '@testing-library/react'
import TaskStepList from '@/components/tasks/TaskStepList'
import * as tauriEvent from '@tauri-apps/api/event'

interface RegisteredHandler {
  event: string
  handler: (e: { payload: unknown }) => void
}

describe('TaskStepList', () => {
  let registered: RegisteredHandler[]
  let unlistenFns: Array<() => void>

  beforeEach(() => {
    registered = []
    unlistenFns = []
    vi.spyOn(tauriEvent, 'listen').mockImplementation(
      async <T,>(event: string, handler: (e: { payload: T }) => void) => {
        registered.push({
          event,
          handler: handler as (e: { payload: unknown }) => void,
        })
        const un = vi.fn()
        unlistenFns.push(un)
        return un as unknown as () => void
      },
    )
  })

  afterEach(() => {
    vi.restoreAllMocks()
    cleanup()
  })

  function emit(event: string, payload: unknown) {
    for (const r of registered) {
      if (r.event === event) {
        act(() => r.handler({ payload }))
      }
    }
  }

  it('renders empty state before any events arrive', async () => {
    render(<TaskStepList taskId="t1" />)
    // Wait for the async listen() to register
    await screen.findByText('No steps streamed yet.')
  })

  it('renders a step when task-step event fires', async () => {
    render(<TaskStepList taskId="t1" />)
    await screen.findByText('No steps streamed yet.')
    emit('task-step', {
      task_id: 't1',
      run_id: 'r1',
      step_index: 1,
      step_total: 3,
      step_label: 'Running query',
      status: 'started',
      error: null,
      timestamp_ms: 1700000000000,
    })
    expect(screen.getByText('Running query')).toBeInTheDocument()
    expect(screen.getByText('1/3')).toBeInTheDocument()
  })

  it('renders retry banner when task-retry event fires', async () => {
    render(<TaskStepList taskId="t1" />)
    await screen.findByText('No steps streamed yet.')
    emit('task-retry', {
      task_id: 't1',
      run_id: 'r1',
      attempt: 2,
      max_attempts: 4,
      delay_ms: 500,
      last_error: 'rate limited',
      timestamp_ms: 1700000000000,
    })
    expect(screen.getByText(/Retrying.*2.*4/)).toBeInTheDocument()
    expect(screen.getByText('rate limited')).toBeInTheDocument()
  })

  it('ignores events for other task ids', async () => {
    render(<TaskStepList taskId="t1" />)
    await screen.findByText('No steps streamed yet.')
    emit('task-step', {
      task_id: 't2',
      run_id: 'r1',
      step_index: 1,
      step_total: 1,
      step_label: 'Other task step',
      status: 'started',
      error: null,
      timestamp_ms: 1700000000000,
    })
    expect(screen.queryByText('Other task step')).not.toBeInTheDocument()
  })
})

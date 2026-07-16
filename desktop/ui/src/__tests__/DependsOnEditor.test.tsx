import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import DependsOnEditor from '@/components/tasks/DependsOnEditor'
import type { ScheduledRoutine } from '@/types'

const updateScheduledTask = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  updateScheduledTask: (...args: unknown[]) => updateScheduledTask(...args),
}))

function makeRoutine(overrides: Partial<ScheduledRoutine> = {}): ScheduledRoutine {
  return {
    id: 'r1',
    name: 'Routine 1',
    prompt: 'Do thing 1',
    interval_secs: 60,
    trigger_type: 'interval',
    enabled: true,
    created_at: 1717000000,
    fire_count: 0,
    ...overrides,
  }
}

const baseProps = (overrides: Partial<Parameters<typeof DependsOnEditor>[0]> = {}) => ({
  routine: makeRoutine(),
  routines: [
    makeRoutine(),
    makeRoutine({ id: 'r2', name: 'Routine 2' }),
    makeRoutine({ id: 'r3', name: 'Routine 3' }),
  ],
  ...overrides,
})

beforeEach(() => {
  updateScheduledTask.mockReset()
})

describe('DependsOnEditor', () => {
  it('shows empty hint when no other routines exist', () => {
    render(<DependsOnEditor routine={makeRoutine()} routines={[makeRoutine()]} />)
    expect(screen.getByText(/Create at least one other routine/)).toBeInTheDocument()
  })

  it('excludes self from candidate list', () => {
    render(<DependsOnEditor {...baseProps()} />)
    expect(screen.getByText('Routine 2')).toBeInTheDocument()
    expect(screen.getByText('Routine 3')).toBeInTheDocument()
    expect(screen.queryByText('Routine 1')).not.toBeInTheDocument()
  })

  it('disables Save when nothing changed', () => {
    render(<DependsOnEditor {...baseProps()} />)
    expect(screen.getByRole('button', { name: /Save dependencies/i })).toBeDisabled()
    expect(screen.getByRole('button', { name: /Discard dependency changes/i })).toBeDisabled()
  })

  it('enables Save when a dependency is toggled on', () => {
    render(<DependsOnEditor {...baseProps()} />)
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    expect(screen.getByRole('button', { name: /Save dependencies/i })).toBeEnabled()
  })

  it('marks existing deps as checked on mount', () => {
    render(<DependsOnEditor {...baseProps({ routine: makeRoutine({ depends_on: ['r2'] }) })} />)
    const cb = screen.getByLabelText('Depends on Routine 2') as HTMLInputElement
    expect(cb.checked).toBe(true)
  })

  it('sends full new list to updateScheduledTask on save', async () => {
    updateScheduledTask.mockResolvedValue(makeRoutine({ depends_on: ['r2', 'r3'] }))
    const onUpdated = vi.fn()
    render(<DependsOnEditor {...baseProps({ onUpdated })} />)
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    fireEvent.click(screen.getByLabelText('Depends on Routine 3'))
    fireEvent.click(screen.getByRole('button', { name: /Save dependencies/i }))
    await waitFor(() =>
      expect(updateScheduledTask).toHaveBeenCalledWith({
        id: 'r1',
        depends_on: ['r2', 'r3'],
      }),
    )
    expect(onUpdated).toHaveBeenCalled()
  })

  it('sends empty list when all toggles cleared', async () => {
    updateScheduledTask.mockResolvedValue(makeRoutine())
    render(
      <DependsOnEditor
        {...baseProps({ routine: makeRoutine({ depends_on: ['r2', 'r3'] }) })}
      />,
    )
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    fireEvent.click(screen.getByLabelText('Depends on Routine 3'))
    fireEvent.click(screen.getByRole('button', { name: /Save dependencies/i }))
    await waitFor(() =>
      expect(updateScheduledTask).toHaveBeenCalledWith({
        id: 'r1',
        depends_on: [],
      }),
    )
  })

  it('shows toast error when update fails', async () => {
    updateScheduledTask.mockRejectedValue(new Error('Network down'))
    render(<DependsOnEditor {...baseProps()} />)
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    fireEvent.click(screen.getByRole('button', { name: /Save dependencies/i }))
    await waitFor(() => expect(updateScheduledTask).toHaveBeenCalled())
    // After failure, saving=false and the change is still pending — Save stays enabled.
    await waitFor(() =>
      expect(screen.getByRole('button', { name: /Save dependencies/i })).toBeEnabled(),
    )
  })

  it('Reset button reverts toggle to initial state', () => {
    render(<DependsOnEditor {...baseProps()} />)
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    fireEvent.click(screen.getByRole('button', { name: /Discard dependency changes/i }))
    const cb = screen.getByLabelText('Depends on Routine 2') as HTMLInputElement
    expect(cb.checked).toBe(false)
    expect(screen.getByRole('button', { name: /Save dependencies/i })).toBeDisabled()
  })

  it('shows selected count after toggles', () => {
    render(<DependsOnEditor {...baseProps()} />)
    fireEvent.click(screen.getByLabelText('Depends on Routine 2'))
    fireEvent.click(screen.getByLabelText('Depends on Routine 3'))
    expect(screen.getByText('2 selected')).toBeInTheDocument()
  })
})

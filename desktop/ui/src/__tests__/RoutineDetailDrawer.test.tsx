import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import RoutineDetailDrawer from '@/components/tasks/RoutineDetailDrawer'
import type { ScheduledRoutine } from '@/types'

vi.mock('@/components/tasks/DependsOnEditor', () => ({
  default: () => <div data-testid="deps-editor">DependsOnEditor</div>,
}))

function makeRoutine(overrides: Partial<ScheduledRoutine> = {}): ScheduledRoutine {
  return {
    id: 'r1',
    name: 'Daily Standup',
    prompt: 'Summarize today',
    interval_secs: 86400,
    trigger_type: 'interval',
    enabled: true,
    created_at: 1717000000,
    next_fire_at: 1717000060,
    fire_count: 5,
    ...overrides,
  }
}

describe('RoutineDetailDrawer', () => {
  it('renders nothing when routine is null', () => {
    const { container } = render(
      <RoutineDetailDrawer routine={null} routines={[]} onClose={() => {}} />,
    )
    expect(container).toBeEmptyDOMElement()
  })

  it('renders routine name and prompt', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('Daily Standup')).toBeInTheDocument()
    expect(screen.getByText('Summarize today')).toBeInTheDocument()
  })

  it('shows trigger type capitalized', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('Interval')).toBeInTheDocument()
  })

  it('shows Enabled as Yes when enabled true', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('Yes')).toBeInTheDocument()
  })

  it('shows last_error block when routine has error', () => {
    render(
      <RoutineDetailDrawer
        routine={makeRoutine({ last_error: 'Boom' })}
        routines={[]}
        onClose={() => {}}
      />,
    )
    expect(screen.getByText('Boom')).toBeInTheDocument()
  })

  it('renders DependsOnEditor', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByTestId('deps-editor')).toBeInTheDocument()
  })

  it('shows resolved dep names instead of IDs when deps present', () => {
    render(
      <RoutineDetailDrawer
        routine={makeRoutine({ depends_on: ['r2'] })}
        routines={[makeRoutine(), makeRoutine({ id: 'r2', name: 'Deploy' })]}
        onClose={() => {}}
      />,
    )
    expect(screen.getByText('Deploy')).toBeInTheDocument()
  })

  it('shows None when no deps', () => {
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={() => {}} />)
    expect(screen.getByText('None')).toBeInTheDocument()
  })

  it('calls onClose when close button clicked', () => {
    const onClose = vi.fn()
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={onClose} />)
    fireEvent.click(screen.getByRole('button', { name: /Close drawer/i }))
    expect(onClose).toHaveBeenCalled()
  })

  it('calls onClose when backdrop clicked', () => {
    const onClose = vi.fn()
    const { container } = render(
      <RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={onClose} />,
    )
    fireEvent.click(container.firstChild as Element)
    expect(onClose).toHaveBeenCalled()
  })

  it('does not call onClose when inner panel clicked', () => {
    const onClose = vi.fn()
    render(<RoutineDetailDrawer routine={makeRoutine()} routines={[]} onClose={onClose} />)
    fireEvent.click(screen.getByText('Daily Standup'))
    expect(onClose).not.toHaveBeenCalled()
  })
})

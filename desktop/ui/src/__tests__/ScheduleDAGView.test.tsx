// P3.5 ScheduleDAGView tests.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import ScheduleDAGView from '@/components/tasks/ScheduleDAGView'
import type { ScheduledRoutine } from '@/types'

function mk(over: Partial<ScheduledRoutine>): ScheduledRoutine {
  return {
    id: 'r-' + Math.random().toString(36).slice(2),
    name: 'Routine',
    prompt: '',
    interval_secs: 3600,
    trigger_type: 'cron',
    cron_expr: '0 9 * * *',
    enabled: true,
    fire_count: 0,
    created_at: 0,
    ...over,
  }
}

describe('ScheduleDAGView', () => {
  it('shows the empty state when no routines', () => {
    render(<ScheduleDAGView routines={[]} />)
    expect(screen.getByText(/No scheduled routines yet/i)).toBeInTheDocument()
  })

  it('renders each routine name and counts', () => {
    const routines = [mk({ id: 'a', name: 'Alpha' }), mk({ id: 'b', name: 'Beta' })]
    render(<ScheduleDAGView routines={routines} />)
    expect(screen.getByText('Alpha')).toBeInTheDocument()
    expect(screen.getByText('Beta')).toBeInTheDocument()
    expect(screen.getByText('2 routines')).toBeInTheDocument()
  })

  it('renders edges for depends_on', () => {
    const routines = [
      mk({ id: 'a', name: 'Alpha' }),
      mk({ id: 'b', name: 'Beta', depends_on: ['a'] }),
    ]
    const { container } = render(<ScheduleDAGView routines={routines} />)
    const svg = container.querySelector('svg')
    expect(svg).not.toBeNull()
    const paths = svg!.querySelectorAll('path')
    // 1 dependency edge (+arrow marker is a separate path)
    expect(paths.length).toBeGreaterThanOrEqual(1)
  })

  it('renders singular "routine" when there is one', () => {
    render(<ScheduleDAGView routines={[mk({ id: 'a', name: 'Solo' })]} />)
    expect(screen.getByText('1 routine')).toBeInTheDocument()
  })

  it('invokes onSelectRoutine with id when node is clicked', () => {
    const onSelect = vi.fn()
    render(<ScheduleDAGView routines={[mk({ id: 'r-1', name: 'ClickMe' })]} onSelectRoutine={onSelect} />)
    fireEvent.click(screen.getByLabelText('Routine ClickMe'))
    expect(onSelect).toHaveBeenCalledWith('r-1')
  })

  it('shows fire count when present', () => {
    render(<ScheduleDAGView routines={[mk({ id: 'a', name: 'Fired', fire_count: 7 })]} />)
    expect(screen.getByText(/7× fired/)).toBeInTheDocument()
  })

  it('shows disabled marker for disabled routines', () => {
    render(<ScheduleDAGView routines={[mk({ id: 'a', name: 'Off', enabled: false })]} />)
    expect(screen.getByText(/○ disabled/)).toBeInTheDocument()
  })

  it('topology: chains deeper dependencies across columns', () => {
    const routines = [
      mk({ id: 'a', name: 'A' }),
      mk({ id: 'b', name: 'B', depends_on: ['a'] }),
      mk({ id: 'c', name: 'C', depends_on: ['b'] }),
    ]
    const { container } = render(<ScheduleDAGView routines={routines} />)
    // Three nodes + at least two edges (one per dep)
    const svg = container.querySelector('svg')!
    const edges = svg.querySelectorAll('path[marker-end]')
    expect(edges.length).toBeGreaterThanOrEqual(2)
  })
})

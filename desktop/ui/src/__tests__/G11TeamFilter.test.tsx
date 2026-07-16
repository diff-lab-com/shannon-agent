// G11 tests: TasksHeader team filter + TaskCard team badge.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import TasksHeader from '@/components/tasks/TasksHeader'
import TaskCard from '@/components/tasks/TaskCard'
import type { TaskItem } from '@/types'

describe('G11 TasksHeader team filter', () => {
  it('hides team filter when teams is empty', () => {
    render(<TasksHeader teams={[]} teamFilter="all" onTeamFilterChange={() => {}} onToggleFilters={() => {}} onToggleCalendar={() => {}} onToggleNewTask={() => {}} onToggleSchedule={() => {}} />)
    expect(screen.queryByLabelText('Filter by team')).not.toBeInTheDocument()
  })

  it('hides team filter when no teams prop is passed', () => {
    render(<TasksHeader onToggleFilters={() => {}} onToggleCalendar={() => {}} onToggleNewTask={() => {}} onToggleSchedule={() => {}} />)
    expect(screen.queryByLabelText('Filter by team')).not.toBeInTheDocument()
  })

  it('renders team filter with team names when provided', () => {
    render(<TasksHeader teams={['red', 'blue']} teamFilter="all" onTeamFilterChange={() => {}} onToggleFilters={() => {}} onToggleCalendar={() => {}} onToggleNewTask={() => {}} onToggleSchedule={() => {}} />)
    const sel = screen.getByLabelText('Filter by team') as HTMLSelectElement
    expect(sel).toBeInTheDocument()
    const opts = Array.from(sel.options).map(o => o.value)
    expect(opts).toEqual(['all', 'red', 'blue'])
  })

  it('emits selected team on change', () => {
    const onChange = vi.fn()
    render(<TasksHeader teams={['red', 'blue']} teamFilter="all" onTeamFilterChange={onChange} onToggleFilters={() => {}} onToggleCalendar={() => {}} onToggleNewTask={() => {}} onToggleSchedule={() => {}} />)
    fireEvent.change(screen.getByLabelText('Filter by team'), { target: { value: 'blue' } })
    expect(onChange).toHaveBeenCalledWith('blue')
  })

  it('reflects current teamFilter value', () => {
    render(<TasksHeader teams={['red', 'blue']} teamFilter="red" onTeamFilterChange={() => {}} onToggleFilters={() => {}} onToggleCalendar={() => {}} onToggleNewTask={() => {}} onToggleSchedule={() => {}} />)
    const sel = screen.getByLabelText('Filter by team') as HTMLSelectElement
    expect(sel.value).toBe('red')
  })
})

describe('G11 TaskCard team badge', () => {
  const noop = () => {}

  it('shows team badge when task.team is set', () => {
    const task: TaskItem = { id: 't1', title: 'Title', status: 'pending', team: 'ops' }
    render(<TaskCard task={task} isRunning={false} onSelect={noop} onRunNow={noop} onCancel={noop} />)
    expect(screen.getByText('ops')).toBeInTheDocument()
  })

  it('omits team badge when task.team is undefined', () => {
    const task: TaskItem = { id: 't1', title: 'Title', status: 'pending' }
    render(<TaskCard task={task} isRunning={false} onSelect={noop} onRunNow={noop} onCancel={noop} />)
    expect(screen.queryByText('ops')).not.toBeInTheDocument()
  })

  it('shows team badge with title attribute for accessibility', () => {
    const task: TaskItem = { id: 't1', title: 'Title', status: 'pending', team: 'data' }
    render(<TaskCard task={task} isRunning={false} onSelect={noop} onRunNow={noop} onCancel={noop} />)
    const badge = screen.getByText('data').closest('[title]')
    expect(badge?.getAttribute('title')).toBe('Team: data')
  })
})

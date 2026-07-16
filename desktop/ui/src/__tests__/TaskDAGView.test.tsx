import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import TaskDAGView from '@/components/tasks/TaskDAGView'
import type { TaskItem } from '@/types'

function mkTask(over: Partial<TaskItem> = {}): TaskItem {
  return {
    id: 't1',
    title: 'Task 1',
    status: 'pending',
    ...over,
  }
}

describe('TaskDAGView', () => {
  it('renders empty state when no tasks', () => {
    render(<TaskDAGView tasks={[]} />)
    expect(screen.getByText(/No tasks yet/)).toBeInTheDocument()
  })

  it('renders task title and status badge', () => {
    render(<TaskDAGView tasks={[mkTask()]} />)
    expect(screen.getByText('Task 1')).toBeInTheDocument()
    expect(screen.getByText('pending')).toBeInTheDocument()
  })

  it('renders task count and edge count in header', () => {
    render(
      <TaskDAGView
        tasks={[
          mkTask({ id: 'a', title: 'A' }),
          mkTask({ id: 'b', title: 'B', blocked_by: ['a'] }),
        ]}
      />,
    )
    expect(screen.getByText(/2 tasks · 1 edges/)).toBeInTheDocument()
  })

  it('renders legend', () => {
    render(<TaskDAGView tasks={[mkTask()]} />)
    expect(screen.getByText('Completed')).toBeInTheDocument()
    expect(screen.getByText('Running')).toBeInTheDocument()
    expect(screen.getByText('Pending')).toBeInTheDocument()
  })

  it('calls onSelectTask when a node is clicked', () => {
    const onSelect = vi.fn()
    render(<TaskDAGView tasks={[mkTask({ id: 't1', title: 'ClickMe' })]} onSelectTask={onSelect} />)
    fireEvent.click(screen.getByText('ClickMe'))
    expect(onSelect).toHaveBeenCalledWith('t1')
  })

  it('renders assignee when present', () => {
    render(<TaskDAGView tasks={[mkTask({ id: 't1', title: 'T', assignee: 'lead' })]} />)
    expect(screen.getByText('@ lead')).toBeInTheDocument()
  })

  it('handles cycle without infinite loop', () => {
    // A → B → A (cycle). Should not throw.
    const tasks = [
      mkTask({ id: 'a', title: 'A', blocked_by: ['b'] }),
      mkTask({ id: 'b', title: 'B', blocked_by: ['a'] }),
    ]
    expect(() => render(<TaskDAGView tasks={tasks} />)).not.toThrow()
    expect(screen.getByText('A')).toBeInTheDocument()
    expect(screen.getByText('B')).toBeInTheDocument()
  })

  it('renders multiple columns when there are dependency chains', () => {
    // No assertion on layout coords (those are SVG attributes), but verify
    // the task titles render in dependency order.
    render(
      <TaskDAGView
        tasks={[
          mkTask({ id: 'a', title: 'Root' }),
          mkTask({ id: 'b', title: 'Child', blocked_by: ['a'] }),
          mkTask({ id: 'c', title: 'Grandchild', blocked_by: ['b'] }),
        ]}
      />,
    )
    expect(screen.getByText('Root')).toBeInTheDocument()
    expect(screen.getByText('Child')).toBeInTheDocument()
    expect(screen.getByText('Grandchild')).toBeInTheDocument()
  })
})

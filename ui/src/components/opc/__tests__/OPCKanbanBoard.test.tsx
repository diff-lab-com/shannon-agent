import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCKanbanBoard, { bucketFor } from '@/components/opc/OPCKanbanBoard'
import type { TaskItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  startBackgroundTask: vi.fn().mockResolvedValue('ok'),
  configure: vi.fn().mockResolvedValue(undefined),
}))

function renderBoard(tasks: TaskItem[] = []) {
  return render(
    <MemoryRouter>
      <OPCKanbanBoard tasks={tasks} refreshTasks={vi.fn()} />
    </MemoryRouter>
  )
}

describe('bucketFor', () => {
  it('maps pending → todo bucket', () => {
    expect(bucketFor('pending')).toBe('todo')
  })

  it('maps review → pending bucket', () => {
    expect(bucketFor('review')).toBe('pending')
  })

  it('maps in_progress → doing bucket', () => {
    expect(bucketFor('in_progress')).toBe('doing')
  })

  it('maps completed → done bucket', () => {
    expect(bucketFor('completed')).toBe('done')
  })

  it('maps deprecated → deprecated bucket', () => {
    expect(bucketFor('deprecated')).toBe('deprecated')
  })

  it('falls back to todo for unknown status', () => {
    expect(bucketFor('nonexistent')).toBe('todo')
  })
})

describe('OPCKanbanBoard', () => {
  it('renders KANBAN header', () => {
    renderBoard()
    expect(screen.getByText('KANBAN')).toBeInTheDocument()
  })

  it('renders all 5 column titles', () => {
    renderBoard()
    expect(screen.getByText('To Do')).toBeInTheDocument()
    expect(screen.getByText('Pending')).toBeInTheDocument()
    expect(screen.getByText('Doing')).toBeInTheDocument()
    expect(screen.getByText('Done')).toBeInTheDocument()
    expect(screen.getByText('Deprecated')).toBeInTheDocument()
  })

  it('renders empty state for empty Deprecated column', () => {
    renderBoard()
    expect(screen.getByText('No deprecated tasks.')).toBeInTheDocument()
  })

  it('renders quick task input', () => {
    renderBoard()
    expect(screen.getByPlaceholderText('Quick inject task...')).toBeInTheDocument()
  })

  it('shows empty placeholders for empty non-deprecated columns', () => {
    renderBoard()
    const empties = screen.getAllByText('Empty')
    expect(empties.length).toBe(4) // To Do, Pending, Doing, Done
  })

  it('renders todo task in To Do column', () => {
    const tasks: TaskItem[] = [{ id: 'task-todo-1', title: 'My Todo', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('My Todo')).toBeInTheDocument()
  })

  it('renders high-priority badge as priority text', () => {
    const tasks: TaskItem[] = [{ id: 'p1', title: 'Important', status: 'todo', priority: 'high' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('high')).toBeInTheDocument()
  })

  it('renders blocked task in Pending column with Critical badge', () => {
    const tasks: TaskItem[] = [{ id: 'p2', title: 'Stuck Task', status: 'blocked', priority: 'high' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Stuck Task')).toBeInTheDocument()
    expect(screen.getByText('Critical')).toBeInTheDocument()
  })

  it('renders in_progress task in Doing column', () => {
    const tasks: TaskItem[] = [{ id: 'p3', title: 'Active Work', status: 'in_progress' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Active Work')).toBeInTheDocument()
  })

  it('renders done task in Done column', () => {
    const tasks: TaskItem[] = [{ id: 'p4', title: 'Shipped', status: 'completed' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Shipped')).toBeInTheDocument()
  })

  it('updates quick task input on type', () => {
    renderBoard()
    const input = screen.getByPlaceholderText('Quick inject task...')
    fireEvent.change(input, { target: { value: 'new task' } })
    expect(input).toHaveValue('new task')
  })

  it('handles optimistic drop on Doing column without error', () => {
    const tasks: TaskItem[] = [{ id: 'drag-1', title: 'Drag Me', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    const doingColumn = screen.getByText('Doing').closest('div[class*="shrink-0"]') as HTMLElement
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'drag-1' : '') }
    expect(() => fireEvent.drop(doingColumn!, { dataTransfer: dt as unknown as DataTransfer })).not.toThrow()
    expect(screen.getByText('Drag Me')).toBeInTheDocument()
  })

  it('handles dragover on column without error', () => {
    renderBoard()
    const todoColumn = screen.getByText('To Do').closest('div[class*="shrink-0"]') as HTMLElement
    expect(() => fireEvent.dragOver(todoColumn, { dataTransfer: {} as DataTransfer })).not.toThrow()
  })

  it('shows reset link after a drop creates overrides', () => {
    const tasks: TaskItem[] = [{ id: 'r1', title: 'Move Me', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    const doingColumn = screen.getByText('Doing').closest('div[class*="shrink-0"]') as HTMLElement
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'r1' : '') }
    fireEvent.drop(doingColumn!, { dataTransfer: dt as unknown as DataTransfer })
    expect(screen.getByText('Reset local overrides')).toBeInTheDocument()
  })
})

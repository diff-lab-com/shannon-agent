// Tests for OPCKanbanBoard after the kanban unification refactor.
//
// The board now uses the unified status taxonomy (queued/active/blocked/done/
// failed) via lib/task-status.ts. bucketFor() is preserved as a thin alias
// over classifyStatus() for backwards compatibility.

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

describe('bucketFor (unified taxonomy alias)', () => {
  it('maps pending → queued (legacy "to-do" bucket)', () => {
    expect(bucketFor('pending')).toBe('queued')
  })

  it('maps review → blocked', () => {
    expect(bucketFor('review')).toBe('blocked')
  })

  it('maps blocked → blocked', () => {
    expect(bucketFor('blocked')).toBe('blocked')
  })

  it('maps in_progress → active', () => {
    expect(bucketFor('in_progress')).toBe('active')
  })

  it('maps completed → done', () => {
    expect(bucketFor('completed')).toBe('done')
  })

  it('maps deprecated → failed (unified terminal column)', () => {
    expect(bucketFor('deprecated')).toBe('failed')
  })

  it('falls back to queued for unknown status', () => {
    expect(bucketFor('nonexistent')).toBe('queued')
  })
})

describe('OPCKanbanBoard', () => {
  it('renders KANBAN header', () => {
    renderBoard()
    expect(screen.getByText('KANBAN')).toBeInTheDocument()
  })

  it('renders all 5 unified column titles', () => {
    renderBoard()
    expect(screen.getByText('Queued')).toBeInTheDocument()
    expect(screen.getByText('In Progress')).toBeInTheDocument()
    expect(screen.getByText('Blocked')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
  })

  it('renders empty state for empty Failed column', () => {
    renderBoard()
    expect(screen.getByText('No deprecated tasks.')).toBeInTheDocument()
  })

  it('renders quick task input', () => {
    renderBoard()
    expect(screen.getByPlaceholderText('Add task...')).toBeInTheDocument()
  })

  it('renders queued task in Queued column', () => {
    const tasks: TaskItem[] = [{ id: 'task-todo-1', title: 'My Todo', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('My Todo')).toBeInTheDocument()
  })

  it('renders high-priority badge as priority text', () => {
    const tasks: TaskItem[] = [{ id: 'p1', title: 'Important', status: 'todo', priority: 'high' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('high')).toBeInTheDocument()
  })

  it('renders blocked task in Blocked column with Critical badge', () => {
    const tasks: TaskItem[] = [{ id: 'p2', title: 'Stuck Task', status: 'blocked', priority: 'high' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Stuck Task')).toBeInTheDocument()
    expect(screen.getByText('Critical')).toBeInTheDocument()
  })

  it('renders in_progress task in In Progress column', () => {
    const tasks: TaskItem[] = [{ id: 'p3', title: 'Active Work', status: 'in_progress' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Active Work')).toBeInTheDocument()
  })

  it('renders completed task in Completed column', () => {
    const tasks: TaskItem[] = [{ id: 'p4', title: 'Shipped', status: 'completed' } as TaskItem]
    renderBoard(tasks)
    expect(screen.getByText('Shipped')).toBeInTheDocument()
  })

  it('updates quick task input on type', () => {
    renderBoard()
    const input = screen.getByPlaceholderText('Add task...')
    fireEvent.change(input, { target: { value: 'new task' } })
    expect(input).toHaveValue('new task')
  })

  it('handles optimistic drop on In Progress column without error', () => {
    const tasks: TaskItem[] = [{ id: 'drag-1', title: 'Drag Me', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    // In unified mode, columns are <section> elements with aria-label
    const inProgressSection = document.querySelector('section[aria-label="In Progress"]') as HTMLElement
    expect(inProgressSection).not.toBeNull()
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'drag-1' : '') }
    expect(() => fireEvent.drop(inProgressSection!, { dataTransfer: dt as unknown as DataTransfer })).not.toThrow()
    expect(screen.getByText('Drag Me')).toBeInTheDocument()
  })

  it('handles dragover on column without error', () => {
    renderBoard()
    const queuedSection = document.querySelector('section[aria-label="Queued"]') as HTMLElement
    expect(queuedSection).not.toBeNull()
    expect(() => fireEvent.dragOver(queuedSection, { dataTransfer: {} as DataTransfer })).not.toThrow()
  })

  it('shows reset link after a drop creates overrides', () => {
    const tasks: TaskItem[] = [{ id: 'r1', title: 'Move Me', status: 'todo' } as TaskItem]
    renderBoard(tasks)
    const inProgressSection = document.querySelector('section[aria-label="In Progress"]') as HTMLElement
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'r1' : '') }
    fireEvent.drop(inProgressSection!, { dataTransfer: dt as unknown as DataTransfer })
    expect(screen.getByText('Reset local overrides')).toBeInTheDocument()
  })
})

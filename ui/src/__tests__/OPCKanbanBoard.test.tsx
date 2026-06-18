import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCKanbanBoard from '@/components/opc/OPCKanbanBoard'
import type { TaskItem } from '@/types'

const navigate = vi.hoisted(() => vi.fn())

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return { ...actual, useNavigate: () => navigate }
})

const refreshTasks = vi.fn()

function renderBoard(tasks: TaskItem[]) {
  return render(
    <MemoryRouter>
      <OPCKanbanBoard tasks={tasks} refreshTasks={refreshTasks} />
    </MemoryRouter>,
  )
}

const queuedTask: TaskItem = {
  id: 't-queued-1',
  title: 'Queued Task',
  status: 'queued',
}

beforeEach(() => {
  navigate.mockReset()
  refreshTasks.mockReset()
})

describe('OPCKanbanBoard navigation', () => {
  it('clicking a queued card navigates to /opc/task/:id', () => {
    renderBoard([queuedTask])
    const card = screen.getByRole('button', { name: 'Queued Task' })
    fireEvent.click(card)
    expect(navigate).toHaveBeenCalledWith('/opc/task/t-queued-1')
  })

  it('Enter key on a queued card navigates to /opc/task/:id', () => {
    renderBoard([queuedTask])
    const card = screen.getByRole('button', { name: 'Queued Task' })
    fireEvent.keyDown(card, { key: 'Enter' })
    expect(navigate).toHaveBeenCalledWith('/opc/task/t-queued-1')
  })
})

// Tests for G7 (execution_mode radio) + G8 (assignee datalist) in TaskDetailDrawer.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import type { TaskItem, AgentInfo } from '@/types'
import * as React from 'react'

const updateTask = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  updateTask: (...args: unknown[]) => updateTask(...args),
}))

// Spy on useCatalog to inject agents.
const useCatalogSpy = vi.hoisted(() => vi.fn())
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: (...args: unknown[]) => useCatalogSpy(...args),
}))

const agents: AgentInfo[] = [
  { id: 'a1', name: 'alpha', model: 'm', status: 'idle' },
  { id: 'a2', name: 'beta', model: 'm', status: 'idle' },
  { id: 'a3', name: 'gamma', model: 'm', status: 'idle' },
]

const baseTask: TaskItem = {
  id: 't1',
  title: 'Task One',
  status: 'pending',
  assignee: 'alpha',
  priority: 'normal',
  execution_mode: 'serial',
}

beforeEach(() => {
  updateTask.mockReset()
  updateTask.mockResolvedValue(baseTask)
  useCatalogSpy.mockReturnValue({ agents })
})

describe('TaskDetailDrawer G7+G8', () => {
  it('G8: populates assignee datalist with known agent names', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    const opts = document.querySelectorAll('#assignee-options option')
    expect(opts.length).toBe(3)
    expect(Array.from(opts).map(o => o.getAttribute('value'))).toEqual(['alpha', 'beta', 'gamma'])
  })

  it('G8: assignee input has list attribute pointing to options', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    const input = screen.getByLabelText('Assignee') as HTMLInputElement
    expect(input.getAttribute('list')).toBe('assignee-options')
  })

  it('G7: renders execution radiogroup with serial + parallel options', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    const rg = screen.getByRole('radiogroup', { name: 'Execution mode' })
    expect(rg).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: /serial/i, checked: true })).toBeInTheDocument()
    expect(screen.getByRole('radio', { name: /parallel/i, checked: false })).toBeInTheDocument()
  })

  it('G7: switching to parallel sends execution_mode in payload', async () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    fireEvent.click(screen.getByRole('radio', { name: /parallel/i }))
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(updateTask).toHaveBeenCalledTimes(1))
    const payload = updateTask.mock.calls[0][0]
    expect(payload.execution_mode).toBe('parallel')
  })

  it('G7: preserves default serial when task.execution_mode is null', async () => {
    const taskWithoutMode: TaskItem = { ...baseTask, execution_mode: null }
    render(<TaskDetailDrawer task={taskWithoutMode} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(updateTask).toHaveBeenCalled())
    const payload = updateTask.mock.calls[0][0]
    expect(payload.execution_mode).toBe('serial')
  })

  it('G7: Cancel restores previous execution_mode', () => {
    const parallelTask: TaskItem = { ...baseTask, execution_mode: 'parallel' }
    render(<TaskDetailDrawer task={parallelTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    // Switch to serial then cancel
    fireEvent.click(screen.getByRole('radio', { name: /^serial$/i }))
    expect(screen.getByRole('radio', { name: /^serial$/i, checked: true })).toBeInTheDocument()
    fireEvent.click(screen.getByText('Cancel'))
    // Re-enter edit mode and verify parallel is restored
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByRole('radio', { name: /parallel/i, checked: true })).toBeInTheDocument()
  })

  it('G7: read-only mode shows execution_mode text', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    // Default serial is shown as text
    expect(screen.getByText('serial')).toBeInTheDocument()
  })
})

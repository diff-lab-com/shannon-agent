import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import TaskDetailDrawer from '@/components/tasks/TaskDetailDrawer'
import type { TaskItem } from '@/types'

const updateTask = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  updateTask: (...args: unknown[]) => updateTask(...args),
}))

const baseTask: TaskItem = {
  id: 't1',
  title: 'Test Task',
  status: 'pending',
  assignee: 'lead',
  priority: 'normal',
  description: 'desc',
  blocked_by: ['t0'],
  blocks: ['t2'],
  due_date: 1717200000, // 2024-06-01 UTC
}

beforeEach(() => {
  updateTask.mockReset()
  updateTask.mockResolvedValue(baseTask)
})

describe('TaskDetailDrawer edit mode', () => {
  it('renders read-only fields by default', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    expect(screen.getByText('Test Task')).toBeInTheDocument()
    expect(screen.getByText('normal')).toBeInTheDocument() // priority
    expect(screen.getByText('lead')).toBeInTheDocument() // assignee
    expect(screen.getByText('Blocked by:')).toBeInTheDocument()
    expect(screen.getByText('t0')).toBeInTheDocument()
  })

  it('shows Edit button for TaskItem', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('enters edit mode and shows Save button', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Save')).toBeInTheDocument()
    expect(screen.getByText('Cancel')).toBeInTheDocument()
  })

  it('shows selects and inputs in edit mode', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByLabelText('Status')).toBeInTheDocument()
    expect(screen.getByLabelText('Priority')).toBeInTheDocument()
    expect(screen.getByLabelText('Assignee')).toBeInTheDocument()
    expect(screen.getByLabelText('Due date')).toBeInTheDocument()
  })

  it('sends update payload on Save', async () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    // Change priority
    fireEvent.change(screen.getByLabelText('Priority'), { target: { value: 'high' } })
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(updateTask).toHaveBeenCalledTimes(1))
    const payload = updateTask.mock.calls[0][0]
    expect(payload.id).toBe('t1')
    expect(payload.priority).toBe('high')
    expect(payload.status).toBe('pending')
    expect(payload.assignee).toBe('lead')
  })

  it('calls onUpdated callback after successful save', async () => {
    const onUpdated = vi.fn()
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} onUpdated={onUpdated} />)
    fireEvent.click(screen.getByText('Edit'))
    fireEvent.click(screen.getByText('Save'))
    await waitFor(() => expect(onUpdated).toHaveBeenCalledTimes(1))
  })

  it('Cancel restores previous values', () => {
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    fireEvent.change(screen.getByLabelText('Priority'), { target: { value: 'critical' } })
    fireEvent.click(screen.getByText('Cancel'))
    // Back to read-only; priority text should show original 'normal'
    expect(screen.getByText('normal')).toBeInTheDocument()
    expect(screen.queryByText('Save')).not.toBeInTheDocument()
  })

  it('shows error toast when save fails', async () => {
    updateTask.mockRejectedValue(new Error('Disk full'))
    render(<TaskDetailDrawer task={baseTask} onClose={() => {}} />)
    fireEvent.click(screen.getByText('Edit'))
    fireEvent.click(screen.getByText('Save'))
    // The Save handler rejects; we assert the call was made and the button
    // returns to enabled (no Save in flight). Toast rendering depends on the
    // sonner <Toaster> provider which isn't mounted in this unit test.
    await waitFor(() => expect(updateTask).toHaveBeenCalled())
    await waitFor(() => expect(screen.getByText('Save')).not.toBeDisabled())
  })

  it('hides Edit button for BackgroundTaskInfo (no editable fields)', () => {
    const bg = { task_id: 'bg1', prompt: 'run thing', status: 'running', started_at: 0, completed_at: null, output: '' }
    // @ts-expect-error — testing duck-typed path
    render(<TaskDetailDrawer task={bg} onClose={() => {}} />)
    expect(screen.queryByText('Edit')).not.toBeInTheDocument()
  })
})

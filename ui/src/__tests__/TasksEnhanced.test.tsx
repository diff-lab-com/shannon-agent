import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Tasks from '@/pages/Tasks'

let mockContext: any = {
  tasks: [],
  backgroundTasks: [],
  agents: [],
  refreshTasks: vi.fn(),
}

vi.mock('@/context/AppContext', () => ({
  useApp: () => mockContext,
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual('@/lib/tauri-api')
  return {
    ...actual,
    startBackgroundTask: vi.fn().mockResolvedValue('task-1'),
    cancelBackgroundTask: vi.fn().mockResolvedValue(true),
  }
})

function renderTasks() {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={['/tasks']}>
        <Routes>
          <Route path="/tasks" element={<Tasks />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

function setContext(ctx: any) {
  mockContext = { ...mockContext, ...ctx }
}

describe('Tasks Enhanced', () => {
  it('shows Scheduled Tasks heading', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Scheduled Tasks')).toBeInTheDocument()
  })

  it('shows empty state when no tasks', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('No tasks yet.')).toBeInTheDocument()
  })

  it('has Filters button', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Filters')).toBeInTheDocument()
  })

  it('has Month View toggle', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Month View')).toBeInTheDocument()
  })

  it('has New Background Task button', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('New Background Task')).toBeInTheDocument()
  })

  it('toggles filters on click', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    fireEvent.click(screen.getByText('Filters'))
    expect(screen.getByText('All')).toBeInTheDocument()
  })

  it('toggles calendar view on click', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    fireEvent.click(screen.getByText('Month View'))
    expect(screen.getByText('List View')).toBeInTheDocument()
  })

  it('renders calendar with day names', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    // Day headers come from Intl weekday 'short' (locale-aware); en → "Mon".
    expect(screen.getByText('Mon')).toBeInTheDocument()
  })

  it('renders Task Completion card', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Task Completion')).toBeInTheDocument()
  })

  it('shows 0% efficiency when no tasks', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('0%')).toBeInTheDocument()
  })

  it('renders Schedule heading', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Schedule')).toBeInTheDocument()
  })

  it('shows no active tasks message', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('No active tasks')).toBeInTheDocument()
  })

  it('renders tasks with status badges', () => {
    setContext({ tasks: [{ id: '1', title: 'Test Task', status: 'completed', assignee: 'Bot', priority: 'high' }], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Test Task')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })

  it('renders running task with stop button', () => {
    setContext({ tasks: [{ id: '2', title: 'Running Task', status: 'running' }], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Running')).toBeInTheDocument()
    expect(screen.getByLabelText('Cancel task')).toBeInTheDocument()
  })

  it('renders Run Now button', () => {
    setContext({ tasks: [{ id: '1', title: 'Task', status: 'pending' }], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Run Now')).toBeInTheDocument()
  })

  it('renders background tasks in execution log', () => {
    setContext({ tasks: [], backgroundTasks: [{ task_id: 'bt1', prompt: 'test prompt', status: 'completed', started_at: Date.now(), completed_at: Date.now(), output: 'done' }], agents: [] })
    renderTasks()
    expect(screen.getByText('Task Execution Log')).toBeInTheDocument()
    expect(screen.getByText('test prompt')).toBeInTheDocument()
  })

  it('opens task detail drawer on click', () => {
    setContext({ tasks: [{ id: '1', title: 'DrawerTask', status: 'completed', description: 'Desc', priority: 'high', assignee: 'Agent' }], backgroundTasks: [], agents: [] })
    renderTasks()
    fireEvent.click(screen.getByText('DrawerTask'))
    expect(screen.getByText('Task Detail')).toBeInTheDocument()
  })

  it('closes drawer on close button click', () => {
    setContext({ tasks: [{ id: '1', title: 'CloseTask', status: 'completed' }], backgroundTasks: [], agents: [] })
    renderTasks()
    fireEvent.click(screen.getByText('CloseTask'))
    expect(screen.getByText('Task Detail')).toBeInTheDocument()
    fireEvent.click(screen.getByLabelText('Close drawer'))
    expect(screen.queryByText('Task Detail')).not.toBeInTheDocument()
  })

  it('shows agent allocation when agents present', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [{ id: 'a1', name: 'Agent A', model: 'gpt-4', status: 'running' }] })
    renderTasks()
    expect(screen.getByText('Agent Allocation')).toBeInTheDocument()
  })

  it('navigates calendar months', () => {
    setContext({ tasks: [], backgroundTasks: [], agents: [] })
    renderTasks()
    fireEvent.click(screen.getByLabelText('Previous month'))
    fireEvent.click(screen.getByLabelText('Next month'))
  })

  it('shows failed status badge', () => {
    setContext({ tasks: [{ id: '3', title: 'Failed Task', status: 'failed' }], backgroundTasks: [], agents: [] })
    renderTasks()
    expect(screen.getByText('Failed')).toBeInTheDocument()
  })

  it('shows running background task with cancel button', () => {
    setContext({ tasks: [], backgroundTasks: [{ task_id: 'bt2', prompt: 'running bg', status: 'running', started_at: Date.now(), completed_at: null, output: '' }], agents: [] })
    renderTasks()
    expect(screen.getByLabelText('Cancel background task')).toBeInTheDocument()
  })

  // US-TASK-04: Filter Tasks by status
  describe('Filter functionality', () => {
    // Use tasks without 'running' status to avoid sidebar "Active Now" duplication
    const mixedTasks = [
      { id: '1', title: 'Build API', status: 'completed' },
      { id: '2', title: 'Write Tests', status: 'pending' },
      { id: '3', title: 'Code Review', status: 'completed' },
      { id: '4', title: 'Fix Bug', status: 'pending' },
    ]

    it('shows all tasks by default', () => {
      setContext({ tasks: mixedTasks, backgroundTasks: [], agents: [] })
      renderTasks()
      expect(screen.getByText('Build API')).toBeInTheDocument()
      expect(screen.getByText('Write Tests')).toBeInTheDocument()
      expect(screen.getByText('Code Review')).toBeInTheDocument()
      expect(screen.getByText('Fix Bug')).toBeInTheDocument()
    })

    it('filters to completed tasks only', () => {
      setContext({ tasks: mixedTasks, backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      fireEvent.click(screen.getByRole('button', { name: 'Completed' }))
      expect(screen.getByText('Build API')).toBeInTheDocument()
      expect(screen.getByText('Code Review')).toBeInTheDocument()
      expect(screen.queryByText('Write Tests')).not.toBeInTheDocument()
      expect(screen.queryByText('Fix Bug')).not.toBeInTheDocument()
    })

    it('filters to pending tasks only', () => {
      setContext({ tasks: mixedTasks, backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      fireEvent.click(screen.getByRole('button', { name: 'Pending' }))
      expect(screen.getByText('Write Tests')).toBeInTheDocument()
      expect(screen.getByText('Fix Bug')).toBeInTheDocument()
      expect(screen.queryByText('Build API')).not.toBeInTheDocument()
      expect(screen.queryByText('Code Review')).not.toBeInTheDocument()
    })

    it('filters to running tasks only', () => {
      const withRunning = [
        { id: '1', title: 'Build API', status: 'completed' },
        { id: '2', title: 'Deploy App', status: 'running' },
      ]
      setContext({ tasks: withRunning, backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      fireEvent.click(screen.getByRole('button', { name: 'Running' }))
      // Running tasks appear in both task list and "Active Now" sidebar
      expect(screen.queryAllByText('Deploy App').length).toBeGreaterThanOrEqual(1)
      expect(screen.queryByText('Build API')).not.toBeInTheDocument()
    })

    it('resets to all tasks when All is clicked', () => {
      setContext({ tasks: mixedTasks, backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      fireEvent.click(screen.getByRole('button', { name: 'Completed' }))
      expect(screen.queryByText('Write Tests')).not.toBeInTheDocument()
      fireEvent.click(screen.getByRole('button', { name: 'All' }))
      expect(screen.getByText('Write Tests')).toBeInTheDocument()
      expect(screen.getByText('Fix Bug')).toBeInTheDocument()
      expect(screen.getByText('Build API')).toBeInTheDocument()
    })

    it('highlights active filter button', () => {
      setContext({ tasks: mixedTasks, backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      const completedBtn = screen.getByRole('button', { name: 'Completed' })
      fireEvent.click(completedBtn)
      expect(completedBtn.className).toContain('primary')
    })

    it('shows empty state when filter matches no tasks', () => {
      setContext({ tasks: [{ id: '1', title: 'Done', status: 'completed' }], backgroundTasks: [], agents: [] })
      renderTasks()
      fireEvent.click(screen.getByText('Filters'))
      fireEvent.click(screen.getByRole('button', { name: 'Running' }))
      expect(screen.getByText('No tasks yet.')).toBeInTheDocument()
    })
  })
})

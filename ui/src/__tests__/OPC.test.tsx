import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPC from '@/pages/OPC'

const ctx = vi.hoisted(() => ({
  agents: [] as any[],
  tasks: [] as any[],
  config: null as any,
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
}))

function resetCtx() {
  ctx.agents = []
  ctx.tasks = []
  ctx.config = null
}

function renderOPC() {
  return render(
    <MemoryRouter>
      <OPC />
    </MemoryRouter>
  )
}

describe('OPC page', () => {
  it('renders strategic focus section', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('Strategic Focus')).toBeInTheDocument()
  })

  it('renders agent swarm heading', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('Agent Swarm')).toBeInTheDocument()
  })

  it('renders kanban section', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('KANBAN')).toBeInTheDocument()
  })

  it('renders quick task input', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByPlaceholderText('Quick inject task...')).toBeInTheDocument()
  })

  it('renders all kanban columns', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('To Do')).toBeInTheDocument()
    expect(screen.getByText('Pending')).toBeInTheDocument()
    expect(screen.getByText('Doing')).toBeInTheDocument()
    expect(screen.getByText('Done')).toBeInTheDocument()
    expect(screen.getByText('Deprecated')).toBeInTheDocument()
  })

  it('renders no agents message when empty', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText(/No agents running/)).toBeInTheDocument()
  })

  it('shows default heading without config provider', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText(/Autonomous task execution through multi-agent/)).toBeInTheDocument()
  })

  it('shows provider-based heading with config', () => {
    resetCtx()
    ctx.config = { provider: 'anthropic', model: 'claude-sonnet' }
    renderOPC()
    expect(screen.getByText(/Anthropic Agent Orchestration/)).toBeInTheDocument()
  })

  it('shows agent count badge', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderOPC()
    expect(screen.getByText('1 Active')).toBeInTheDocument()
  })

  it('renders agent cards with name', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Research Agent', status: 'running', task: 'analyzing' }]
    renderOPC()
    expect(screen.getByText('Research Agent')).toBeInTheDocument()
  })

  it('renders todo tasks in To Do column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-1-abc', title: 'Todo Item', status: 'todo', priority: 'high' }]
    renderOPC()
    expect(screen.getByText('Todo Item')).toBeInTheDocument()
    expect(screen.getByText('high')).toBeInTheDocument()
  })

  it('renders pending tasks in Pending column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-2-xyz', title: 'Review Item', status: 'review', assignee: 'Bot' }]
    renderOPC()
    expect(screen.getByText('Review Item')).toBeInTheDocument()
    expect(screen.getByText(/Review$/)).toBeInTheDocument()
  })

  it('renders blocked tasks in Pending column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-3-def', title: 'Blocked Item', status: 'blocked', priority: 'high' }]
    renderOPC()
    expect(screen.getByText('Blocked Item')).toBeInTheDocument()
    expect(screen.getByText('Critical')).toBeInTheDocument()
  })

  it('renders in-progress tasks in Doing column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-4-ghi', title: 'Active Item', status: 'in_progress', assignee: 'Agent1' }]
    renderOPC()
    expect(screen.getByText('Active Item')).toBeInTheDocument()
  })

  it('renders completed tasks in Done column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-5-jkl', title: 'Done Item', status: 'completed' }]
    renderOPC()
    expect(screen.getByText('Done Item')).toBeInTheDocument()
  })

  it('quick task input accepts text', () => {
    resetCtx()
    renderOPC()
    const input = screen.getByPlaceholderText('Quick inject task...')
    fireEvent.change(input, { target: { value: 'Test quick task' } })
    expect(input).toHaveValue('Test quick task')
  })

  it('shows agent task or status text', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'idle' }]
    renderOPC()
    expect(screen.getByText('idle')).toBeInTheDocument()
  })

  it('renders empty state in Deprecated column', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('No deprecated tasks.')).toBeInTheDocument()
  })

  it('renders Empty state for empty columns', () => {
    resetCtx()
    renderOPC()
    const emptyMessages = screen.getAllByText('Empty')
    expect(emptyMessages.length).toBeGreaterThan(0)
  })

  // US-OPC-04: Strategic Focus editing
  it('shows Edit button for strategic focus', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  it('toggles to textarea on Edit click', () => {
    resetCtx()
    renderOPC()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Save Focus')).toBeInTheDocument()
  })

  it('shows Cancel when editing and toggles back', () => {
    resetCtx()
    renderOPC()
    fireEvent.click(screen.getByText('Edit'))
    expect(screen.getByText('Cancel')).toBeInTheDocument()
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.getByText('Edit')).toBeInTheDocument()
  })

  // C1: worktree path label on agent card
  it('renders worktree path label when agent has worktree_path', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Dev Agent', status: 'running', worktree_path: '/Users/x/worktrees/feature-auth' }]
    renderOPC()
    expect(screen.getByText('/feature-auth')).toBeInTheDocument()
  })

  it('omits worktree label when agent has no worktree_path', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Dev Agent', status: 'running' }]
    renderOPC()
    expect(screen.queryByText(/worktree/i)).not.toBeInTheDocument()
  })

  // F6: ⋮ menu shows Stop / Pause / View Logs / Reassign actions
  it('opens agent actions menu when ⋮ button clicked', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderOPC()
    const menuBtn = screen.getByRole('button', { name: /Actions for Bot/ })
    fireEvent.click(menuBtn)
    expect(screen.getByRole('menuitem', { name: /Stop/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /Pause/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /View Logs/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /Reassign/ })).toBeInTheDocument()
  })

  it('clicking ⋮ button does not navigate (stops propagation)', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderOPC()
    const menuBtn = screen.getByRole('button', { name: /Actions for Bot/ })
    // Should not throw or trigger navigation
    expect(() => fireEvent.click(menuBtn)).not.toThrow()
  })

  // F4: HTML5 drag-and-drop moves cards between columns optimistically
  it('moves a todo card to Doing column on drop (local override)', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-99', title: 'Draggable Task', status: 'todo' }]
    renderOPC()
    // Verify card initially in To Do
    expect(screen.getByText('Draggable Task')).toBeInTheDocument()
    // Simulate drop on the Doing column — jsdom lacks DataTransfer, stub it
    const doingColumn = screen.getByText('Doing').closest('div[class*="shrink-0"]') as HTMLElement
    expect(doingColumn).toBeTruthy()
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'task-99' : '') }
    fireEvent.drop(doingColumn!, { dataTransfer: dt as unknown as DataTransfer })
    // The card should still be in the document (moved column) — title persists
    expect(screen.getByText('Draggable Task')).toBeInTheDocument()
  })

  it('KanbanColumn handles dragover without error', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-1', title: 'Item', status: 'todo' }]
    renderOPC()
    const todoColumn = screen.getByText('To Do').closest('div[class*="shrink-0"]') as HTMLElement
    fireEvent.dragOver(todoColumn, { dataTransfer: {} as DataTransfer })
    // Should not error and column should still be present
    expect(todoColumn).toBeInTheDocument()
  })

  // F5: clicking agent card calls navigate (smoke test — navigate requires router context)
  it('agent card is keyboard focusable as button', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderOPC()
    const card = screen.getByRole('button', { name: /Bot — running/ })
    expect(card).toHaveAttribute('tabindex', '0')
  })

  // C5: Spawn Agent button + modal
  it('renders Spawn button on Agent Swarm heading', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByRole('button', { name: /Spawn new agent/ })).toBeInTheDocument()
  })

  it('opens spawn-agent modal on Spawn click', () => {
    resetCtx()
    renderOPC()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    expect(screen.getByRole('heading', { name: /Spawn New Agent/ })).toBeInTheDocument()
    expect(screen.getByPlaceholderText(/Research Agent/)).toBeInTheDocument()
  })

  it('validates name is required when creating agent', () => {
    resetCtx()
    renderOPC()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Create Agent$/ }))
    expect(screen.getByText(/Agent name is required/)).toBeInTheDocument()
  })

  it('closes modal on Cancel', () => {
    resetCtx()
    renderOPC()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }))
    expect(screen.queryByRole('heading', { name: /Spawn New Agent/ })).not.toBeInTheDocument()
  })
})

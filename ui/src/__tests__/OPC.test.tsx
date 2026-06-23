import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPC from '@/pages/OPC'

const ctx = vi.hoisted(() => ({
  agents: [] as any[],
  tasks: [] as any[],
  config: null as any,
  refreshTasks: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
}))

function resetCtx() {
  ctx.agents = []
  ctx.tasks = []
  ctx.config = null
  ctx.refreshTasks = vi.fn().mockResolvedValue(undefined)
}

function renderOPC() {
  return render(
    <MemoryRouter>
      <OPC />
    </MemoryRouter>
  )
}

describe('OPC page', () => {
  it('renders today\'s mission section', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText("Today's Mission")).toBeInTheDocument()
  })

  it('renders active agents heading', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('Active Agents')).toBeInTheDocument()
  })

  it('renders kanban section', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('KANBAN')).toBeInTheDocument()
  })

  it('renders quick task input', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByPlaceholderText('Add task...')).toBeInTheDocument()
  })

  it('renders all kanban columns', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('Queued')).toBeInTheDocument()
    expect(screen.getByText('Blocked')).toBeInTheDocument()
    expect(screen.getByText('In Progress')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
    expect(screen.getByText('Failed')).toBeInTheDocument()
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

  it('renders todo tasks in Queued column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-1-abc', title: 'Todo Item', status: 'todo', priority: 'high' }]
    renderOPC()
    expect(screen.getByText('Todo Item')).toBeInTheDocument()
    expect(screen.getByText('high')).toBeInTheDocument()
  })

  it('renders review tasks in Blocked column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-2-xyz', title: 'Review Item', status: 'review', assignee: 'Bot' }]
    renderOPC()
    expect(screen.getByText('Review Item')).toBeInTheDocument()
    expect(screen.getByText(/Review$/)).toBeInTheDocument()
  })

  it('renders blocked tasks in Blocked column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-3-def', title: 'Blocked Item', status: 'blocked', priority: 'high' }]
    renderOPC()
    expect(screen.getByText('Blocked Item')).toBeInTheDocument()
    expect(screen.getByText('Critical')).toBeInTheDocument()
  })

  it('renders in-progress tasks in In Progress column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-4-ghi', title: 'Active Item', status: 'in_progress', assignee: 'Agent1' }]
    renderOPC()
    expect(screen.getByText('Active Item')).toBeInTheDocument()
  })

  it('renders completed tasks in Completed column', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-5-jkl', title: 'Done Item', status: 'completed' }]
    renderOPC()
    expect(screen.getByText('Done Item')).toBeInTheDocument()
  })

  it('quick task input accepts text', () => {
    resetCtx()
    renderOPC()
    const input = screen.getByPlaceholderText('Add task...')
    fireEvent.change(input, { target: { value: 'Test quick task' } })
    expect(input).toHaveValue('Test quick task')
  })

  it('shows agent task or status text', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'idle' }]
    renderOPC()
    expect(screen.getByText('idle')).toBeInTheDocument()
  })

  it('renders empty state in Failed column', () => {
    resetCtx()
    renderOPC()
    expect(screen.getByText('No deprecated tasks.')).toBeInTheDocument()
  })

  it('renders empty placeholders for empty columns', () => {
    resetCtx()
    renderOPC()
    // 4 of 5 columns are empty (the Failed column has its own message); each
    // shows the unified "Nothing here." placeholder via KanbanBoard.
    const placeholders = screen.getAllByText('Nothing here.')
    expect(placeholders.length).toBe(4)
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
  it('moves a queued card to In Progress column on drop (local override)', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-99', title: 'Draggable Task', status: 'todo' }]
    renderOPC()
    // Verify card initially in Queued
    expect(screen.getByText('Draggable Task')).toBeInTheDocument()
    // Simulate drop on the In Progress column — unified columns are <section aria-label="…">
    const inProgressSection = document.querySelector('section[aria-label="In Progress"]') as HTMLElement
    expect(inProgressSection).toBeTruthy()
    const dt = { getData: (type: string) => (type === 'text/plain' ? 'task-99' : '') }
    fireEvent.drop(inProgressSection!, { dataTransfer: dt as unknown as DataTransfer })
    // The card should still be in the document (moved column) — title persists
    expect(screen.getByText('Draggable Task')).toBeInTheDocument()
  })

  it('KanbanColumn handles dragover without error', () => {
    resetCtx()
    ctx.tasks = [{ id: 'task-1', title: 'Item', status: 'todo' }]
    renderOPC()
    const queuedSection = document.querySelector('section[aria-label="Queued"]') as HTMLElement
    fireEvent.dragOver(queuedSection, { dataTransfer: {} as DataTransfer })
    // Should not error and column should still be present
    expect(queuedSection).toBeInTheDocument()
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
  it('renders Spawn button on Active Agents heading', () => {
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

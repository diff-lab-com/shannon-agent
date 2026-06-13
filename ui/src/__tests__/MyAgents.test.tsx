import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import MyAgents from '@/components/extensions/MyAgents'

const ctx = vi.hoisted(() => ({
  agents: [] as any[],
  backgroundTasks: [] as any[],
  models: [] as any[],
  sendMessage: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
}))

function resetCtx() {
  ctx.agents = []
  ctx.backgroundTasks = []
}

function renderMyAgents() {
  return render(<MemoryRouter><MyAgents /></MemoryRouter>)
}

describe('MyAgents', () => {
  it('renders page heading', () => {
    resetCtx()
    renderMyAgents()
    expect(screen.getByText('My Agents')).toBeInTheDocument()
  })

  it('shows no agents message when empty', () => {
    resetCtx()
    renderMyAgents()
    expect(screen.getByText('No agents running.')).toBeInTheDocument()
  })

  it('shows agent count as 0 when empty', () => {
    resetCtx()
    renderMyAgents()
    expect(screen.getByText('0 agents')).toBeInTheDocument()
  })

  it('shows agent cards when present', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Research Agent', model: 'gpt-4', status: 'running', task: 'analyze data' }]
    renderMyAgents()
    expect(screen.getByText('Research Agent')).toBeInTheDocument()
    expect(screen.getByText('analyze data')).toBeInTheDocument()
    expect(screen.getByText('1 agent')).toBeInTheDocument()
  })

  it('shows Active badge for running agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('Active')).toBeInTheDocument()
  })

  it('shows Idle badge for idle agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'idle' }]
    renderMyAgents()
    expect(screen.getByText('Idle')).toBeInTheDocument()
  })

  it('shows Error badge for error agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'error' }]
    renderMyAgents()
    expect(screen.getByText('Error')).toBeInTheDocument()
  })

  it('shows custom status label for unknown status', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'sleeping' }]
    renderMyAgents()
    expect(screen.getByText('sleeping')).toBeInTheDocument()
  })

  it('shows progress bar when agent has progress', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running', progress: 75 }]
    renderMyAgents()
    expect(screen.getByText('75%')).toBeInTheDocument()
  })

  it('shows tools used count', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running', tools_used: 5 }]
    renderMyAgents()
    expect(screen.getByText('5')).toBeInTheDocument()
  })

  it('shows duration in seconds when under 60s', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running', duration: 45000 }]
    renderMyAgents()
    expect(screen.getByText('45s')).toBeInTheDocument()
  })

  it('shows duration in minutes when over 60s', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running', duration: 120000 }]
    renderMyAgents()
    expect(screen.getByText('2.0m')).toBeInTheDocument()
  })

  it('shows New Specialization card', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('New Specialization')).toBeInTheDocument()
  })

  it('shows Agent Performance heading', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('Agent Performance')).toBeInTheDocument()
  })

  it('shows Task Completion section', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('Task Completion')).toBeInTheDocument()
  })

  it('shows 0% completion when no background tasks', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('0%')).toBeInTheDocument()
  })

  it('shows completion percentage with background tasks', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    ctx.backgroundTasks = [
      { task_id: 't1', prompt: 'Do something', status: 'completed' },
      { task_id: 't2', prompt: 'Do more', status: 'completed' },
      { task_id: 't3', prompt: 'Running task', status: 'running' },
    ]
    renderMyAgents()
    expect(screen.getByText('67%')).toBeInTheDocument()
    expect(screen.getByText('2 of 3 tasks completed')).toBeInTheDocument()
  })

  it('shows no task data message when no background tasks', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('No task execution data yet.')).toBeInTheDocument()
  })

  it('shows background task status bars', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    ctx.backgroundTasks = [
      { task_id: 't1', prompt: 'Completed task here', status: 'completed' },
    ]
    renderMyAgents()
    expect(screen.getByText('Done')).toBeInTheDocument()
  })

  it('renders code icon for coding agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'CodeGenerator', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('code')).toBeInTheDocument()
  })

  it('renders research icon for research agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Researcher', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('query_stats')).toBeInTheDocument()
  })

  it('renders test icon for testing agent', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Tester', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('bug_report')).toBeInTheDocument()
  })

  it('renders Default Model when no model specified', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText(/Default Model/)).toBeInTheDocument()
  })

  it('renders model name when specified', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'TestBot', model: 'claude-3', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText(/claude-3/)).toBeInTheDocument()
  })

  // US-EXT-06: Agent management actions
  it('shows Configure button on agent card', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'idle' }]
    renderMyAgents()
    expect(screen.getByText('Configure')).toBeInTheDocument()
  })

  it('toggles config panel on Configure click', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'ConfigBot', model: 'gpt-4', status: 'idle' }]
    renderMyAgents()
    fireEvent.click(screen.getByText('Configure'))
    expect(screen.getByText('Close')).toBeInTheDocument()
    expect(screen.getByText(/Configuration for/)).toBeInTheDocument()
  })

  // US-EXT-07: Create new agent
  it('shows New Specialization card when agents exist', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderMyAgents()
    expect(screen.getByText('New Specialization')).toBeInTheDocument()
  })

  it('shows create form on New Specialization click', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderMyAgents()
    fireEvent.click(screen.getByText('New Specialization'))
    expect(screen.getByText('Create New Agent')).toBeInTheDocument()
    expect(screen.getByPlaceholderText(/Describe the agent/)).toBeInTheDocument()
    expect(screen.getByText('Create Agent')).toBeInTheDocument()
  })

  it('dismisses create form on Cancel click', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Bot', status: 'running' }]
    renderMyAgents()
    fireEvent.click(screen.getByText('New Specialization'))
    fireEvent.click(screen.getByText('Cancel'))
    expect(screen.getByText('New Specialization')).toBeInTheDocument()
  })
})

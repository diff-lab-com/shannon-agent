import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCAgentSwarm from '@/components/opc/OPCAgentSwarm'
import type { AgentInfo, TaskItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  cancelBackgroundTask: vi.fn().mockResolvedValue(undefined),
  createAgentDefinition: vi.fn().mockResolvedValue(undefined),
  updateTask: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() },
}))

const { useNavigate: _useNavigate } = vi.hoisted(() => ({ useNavigate: vi.fn() }))
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return { ...actual, useNavigate: () => _useNavigate }
})

function renderSwarm(agents: AgentInfo[] = [], tasks: TaskItem[] = []) {
  return render(
    <MemoryRouter>
      <OPCAgentSwarm agents={agents} tasks={tasks} />
    </MemoryRouter>
  )
}

describe('OPCAgentSwarm', () => {
  it('renders Agent Swarm heading', () => {
    renderSwarm()
    expect(screen.getByText('Agent Swarm')).toBeInTheDocument()
  })

  it('shows 0 Active when no agents', () => {
    renderSwarm()
    expect(screen.getByText('0 Active')).toBeInTheDocument()
  })

  it('shows empty state when no agents', () => {
    renderSwarm()
    expect(screen.getByText(/No agents running/)).toBeInTheDocument()
  })

  it('renders Spawn button', () => {
    renderSwarm()
    expect(screen.getByRole('button', { name: /Spawn new agent/ })).toBeInTheDocument()
  })

  it('shows agent count badge', () => {
    renderSwarm([{ id: 'a1', name: 'Bot', status: 'running' } as AgentInfo])
    expect(screen.getByText('1 Active')).toBeInTheDocument()
  })

  it('renders agent name and status', () => {
    renderSwarm([{ id: 'a1', name: 'Research Agent', status: 'running', task: 'analyzing' } as AgentInfo])
    expect(screen.getByText('Research Agent')).toBeInTheDocument()
    expect(screen.getByText('analyzing')).toBeInTheDocument()
  })

  it('shows idle status when not running', () => {
    renderSwarm([{ id: 'a1', name: 'Bot', status: 'idle' } as AgentInfo])
    expect(screen.getByText('idle')).toBeInTheDocument()
  })

  it('renders worktree tail when present', () => {
    renderSwarm([{ id: 'a1', name: 'Dev Agent', status: 'running', worktree_path: '/Users/x/worktrees/feature-auth' } as AgentInfo])
    expect(screen.getByText('/feature-auth')).toBeInTheDocument()
  })

  it('omits worktree label when path absent', () => {
    renderSwarm([{ id: 'a1', name: 'Dev Agent', status: 'running' } as AgentInfo])
    expect(screen.queryByText(/worktree/i)).not.toBeInTheDocument()
  })

  it('opens action menu on ⋮ click', () => {
    renderSwarm([{ id: 'a1', name: 'Bot', status: 'running' } as AgentInfo])
    fireEvent.click(screen.getByRole('button', { name: /Actions for Bot/ }))
    expect(screen.getByRole('menuitem', { name: /Stop/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /Pause/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /View Logs/ })).toBeInTheDocument()
    expect(screen.getByRole('menuitem', { name: /Reassign/ })).toBeInTheDocument()
  })

  it('opens Spawn modal on click', () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    expect(screen.getByRole('heading', { name: /Spawn New Agent/ })).toBeInTheDocument()
  })

  it('validates name required on Spawn submit', () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Create Agent$/ }))
    expect(screen.getByText(/Agent name is required/)).toBeInTheDocument()
  })

  it('closes Spawn modal on Cancel', () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Spawn new agent/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }))
    expect(screen.queryByRole('heading', { name: /Spawn New Agent/ })).not.toBeInTheDocument()
  })

  it('agent card is keyboard focusable as button', () => {
    renderSwarm([{ id: 'a1', name: 'Bot', status: 'running' } as AgentInfo])
    const card = screen.getByRole('button', { name: /Bot — running/ })
    expect(card).toHaveAttribute('tabindex', '0')
  })

  it('uses research icon for "research" agent', () => {
    renderSwarm([{ id: 'a1', name: 'Research Agent', status: 'running' } as AgentInfo])
    // Agent card is the role=button — the icon inside its header (first .material-symbols-outlined within card)
    const card = screen.getByRole('button', { name: /Research Agent — running/ })
    const icon = card.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('query_stats')
  })

  it('uses smart_toy icon for unknown agent', () => {
    renderSwarm([{ id: 'a1', name: 'Mystery Agent', status: 'running' } as AgentInfo])
    const card = screen.getByRole('button', { name: /Mystery Agent — running/ })
    const icon = card.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('smart_toy')
  })
})

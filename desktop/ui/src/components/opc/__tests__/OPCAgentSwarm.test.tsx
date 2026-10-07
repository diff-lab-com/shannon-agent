import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { MemoryRouter } from 'react-router-dom'
import type * as ReactRouterDom from 'react-router-dom'
import OPCAgentSwarm from '@/components/opc/OPCAgentSwarm'
import type { AgentInfo, TaskItem } from '@/types'

vi.mock('@/lib/tauri-api', () => ({
  cancelBackgroundTask: vi.fn().mockResolvedValue(undefined),
  createAgentDefinition: vi.fn().mockResolvedValue(undefined),
  updateTask: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ switchSession: vi.fn().mockResolvedValue(undefined) }),
}))

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), info: vi.fn(), error: vi.fn() },
}))

const { useNavigate: _useNavigate } = vi.hoisted(() => ({ useNavigate: vi.fn() }))
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof ReactRouterDom>('react-router-dom')
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
  it('renders Active Agents heading', () => {
    renderSwarm()
    expect(screen.getByText('Active Agents')).toBeInTheDocument()
  })

  it('shows 0 Active when no agents', () => {
    renderSwarm()
    expect(screen.getByText('0 Active')).toBeInTheDocument()
  })

  it('shows empty state when no agents', () => {
    renderSwarm()
    expect(screen.getByText(/No agents running/)).toBeInTheDocument()
  })

  it('renders the register-template button', () => {
    renderSwarm()
    expect(screen.getByRole('button', { name: /Register a new agent template/ })).toBeInTheDocument()
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

  it('opens the register-template modal on click', () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Register a new agent template/ }))
    expect(screen.getByRole('heading', { name: /Register agent template/ })).toBeInTheDocument()
  })

  it('validates name required on register submit', () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Register a new agent template/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Register$/ }))
    expect(screen.getByText(/Agent name is required/)).toBeInTheDocument()
  })

  it('closes the register-template modal on Cancel', async () => {
    renderSwarm()
    fireEvent.click(screen.getByRole('button', { name: /Register a new agent template/ }))
    fireEvent.click(screen.getByRole('button', { name: /^Cancel$/ }))
    // Base UI Dialog stays mounted during the close animation; wait for
    // the heading to detach instead of asserting immediate removal.
    await waitFor(() => {
      expect(screen.queryByRole('heading', { name: /Register agent template/ })).not.toBeInTheDocument()
    })
  })

  it('agent card is keyboard focusable as button', () => {
    renderSwarm([{ id: 'a1', name: 'Bot', status: 'running' } as AgentInfo])
    const card = screen.getByRole('button', { name: /Bot — Running/ })
    expect(card).toHaveAttribute('tabindex', '0')
  })

  it('uses research icon for "research" agent', () => {
    renderSwarm([{ id: 'a1', name: 'Research Agent', status: 'running' } as AgentInfo])
    // Agent card is the role=button — the icon inside its header (first .material-symbols-outlined within card)
    const card = screen.getByRole('button', { name: /Research Agent — Running/ })
    const icon = card.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('query_stats')
  })

  it('uses smart_toy icon for unknown agent', () => {
    renderSwarm([{ id: 'a1', name: 'Mystery Agent', status: 'running' } as AgentInfo])
    const card = screen.getByRole('button', { name: /Mystery Agent — Running/ })
    const icon = card.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('smart_toy')
  })

  // F-5 (ui-redesign 09): a blocked agent consumes no compute — its load
  // reads "—" with the explanatory tooltip, never a number or progress bar.
  it('shows an em-dash load with blocked tooltip (no percent) for blocked agents', () => {
    renderSwarm([{ id: 'b1', name: 'Ops', status: 'blocked', progress: 55 } as AgentInfo])
    const load = screen.getByTestId('opc-agent-load-b1')
    expect(load).toHaveAttribute('title', 'Blocked — no compute being consumed')
    expect(load).toHaveTextContent('Blocked')
    expect(load).toHaveTextContent('—')
    expect(screen.queryByText('55%')).not.toBeInTheDocument()
    // No progress bar either.
    expect(load.querySelector('.h-1')).toBeNull()
  })

  it('shows the numeric progress meter for running agents', () => {
    renderSwarm([{ id: 'r1', name: 'Scaler', status: 'running', progress: 72 } as AgentInfo])
    expect(screen.getByText('72%')).toBeInTheDocument()
  })

  it('omits the load row when the agent has no progress data', () => {
    renderSwarm([{ id: 'n1', name: 'Idle Bot', status: 'idle' } as AgentInfo])
    expect(screen.queryByTestId('opc-agent-load-n1')).not.toBeInTheDocument()
  })
})

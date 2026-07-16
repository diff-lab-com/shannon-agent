// F6 OPC agent menu tests: View Logs, Reassign, Pause, Stop.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPC from '@/pages/OPC'
import type { AgentInfo, TaskItem } from '@/types'

const cancelBackgroundTask = vi.hoisted(() => vi.fn())
const updateTask = vi.hoisted(() => vi.fn())
const startBackgroundTask = vi.hoisted(() => vi.fn())
const navigate = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  cancelBackgroundTask: (...args: unknown[]) => cancelBackgroundTask(...args),
  updateTask: (...args: unknown[]) => updateTask(...args),
  startBackgroundTask: (...args: unknown[]) => startBackgroundTask(...args),
}))

vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<typeof import('react-router-dom')>('react-router-dom')
  return { ...actual, useNavigate: () => navigate }
})

const useCatalogSpy = vi.hoisted(() => vi.fn())
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: (...args: unknown[]) => useCatalogSpy(...args),
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: (...args: unknown[]) => useCatalogSpy(...args),
}))

const agentAlpha: AgentInfo = { id: 'a1', name: 'alpha', model: 'm', status: 'running', task: 'shipping' }
const agentBeta: AgentInfo = { id: 'a2', name: 'beta', model: 'm', status: 'idle' }

const taskInProgress: TaskItem = {
  id: 't-active',
  title: 'Active Task',
  status: 'in_progress',
  assignee: 'alpha',
}

function renderOPC(overrides: { agents?: AgentInfo[]; tasks?: TaskItem[] } = {}) {
  useCatalogSpy.mockReturnValue({
    agents: overrides.agents ?? [agentAlpha, agentBeta],
    tasks: overrides.tasks ?? [taskInProgress],
    config: { provider: 'anthropic' },
    loading: false,
    refreshTasks: vi.fn(),
  })
  return render(
    <MemoryRouter>
      <OPC />
    </MemoryRouter>,
  )
}

beforeEach(() => {
  cancelBackgroundTask.mockReset()
  cancelBackgroundTask.mockResolvedValue(true)
  updateTask.mockReset()
  updateTask.mockResolvedValue(taskInProgress)
  startBackgroundTask.mockReset()
  startBackgroundTask.mockResolvedValue('bg-1')
  navigate.mockReset()
})

describe('OPC F6 agent menu', () => {
  it('Stop calls cancelBackgroundTask', async () => {
    renderOPC()
    fireEvent.click(screen.getByLabelText('Actions for alpha'))
    fireEvent.click(screen.getByText('Stop'))
    await waitFor(() => expect(cancelBackgroundTask).toHaveBeenCalledWith('a1'))
  })

  it('View Logs navigates to /opc/task with agent query', () => {
    renderOPC()
    fireEvent.click(screen.getByLabelText('Actions for alpha'))
    fireEvent.click(screen.getByText('View Logs'))
    expect(navigate).toHaveBeenCalledWith('/opc/task?agent=a1')
  })

  it('View Logs includes session id when present', () => {
    renderOPC({ agents: [{ ...agentAlpha, session_id: 'sess-9' }] })
    fireEvent.click(screen.getByLabelText('Actions for alpha'))
    fireEvent.click(screen.getByText('View Logs'))
    expect(navigate).toHaveBeenCalledWith('/opc/task?agent=a1&session=sess-9')
  })

  it('Reassign opens modal and submits updateTask', async () => {
    renderOPC()
    fireEvent.click(screen.getByLabelText('Actions for alpha'))
    fireEvent.click(screen.getByText('Reassign'))
    expect(await screen.findByText('Reassign Task')).toBeInTheDocument()
    fireEvent.change(screen.getByPlaceholderText('Pick or type an agent name'), { target: { value: 'beta' } })
    fireEvent.click(screen.getByText('Reassign'))
    await waitFor(() => expect(updateTask).toHaveBeenCalled())
    const payload = updateTask.mock.calls[0][0]
    expect(payload.id).toBe('t-active')
    expect(payload.assignee).toBe('beta')
  })

  it('Reassign skips opening modal when no active task owned by agent', () => {
    renderOPC({ tasks: [{ id: 't-other', title: 'Other', status: 'pending', assignee: 'someoneelse' }] })
    fireEvent.click(screen.getByLabelText('Actions for alpha'))
    fireEvent.click(screen.getByText('Reassign'))
    expect(screen.queryByText('Reassign Task')).not.toBeInTheDocument()
  })
})

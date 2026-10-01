// P2-8 — the OPC cross-agent run table.
//
// Covers the four states that matter: empty, populated columns, the session
// jump (switchSession + navigate to /chat), and row-click detail expansion
// (including the succeeded→completed badge mapping and the failed-run error
// block).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import OPCRunsTable from '@/components/opc/OPCRunsTable'
import * as api from '@/lib/tauri-api'
import type { AgentRunRow } from '@/types'

const navigate = vi.fn()
vi.mock('react-router-dom', async () => {
  const actual = await vi.importActual<object>('react-router-dom')
  return { ...actual, useNavigate: () => navigate }
})

const switchSession = vi.fn().mockResolvedValue(undefined)
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: 's1', switchSession }),
}))

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    listAgentRuns: vi.fn(),
    getExecutionDetail: vi.fn(),
  }
})

const mockedRuns = vi.mocked(api.listAgentRuns)
const mockedDetail = vi.mocked(api.getExecutionDetail)

function row(over: Partial<AgentRunRow> = {}): AgentRunRow {
  return {
    run_id: 'run-1',
    task_id: 'task-a',
    task_name: 'Security Scanner',
    started_at: 1_760_000_000,
    finished_at: 1_760_000_090,
    status: 'succeeded',
    cost_usd: 0.1234,
    token_usage: 12_345,
    session_id: 'sess-9',
    model: 'glm-4.7',
    ...over,
  }
}

function renderTable() {
  return render(
    <MemoryRouter>
      <OPCRunsTable />
    </MemoryRouter>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  mockedRuns.mockResolvedValue([])
  mockedDetail.mockResolvedValue({
    run_id: 'run-1', task_id: 'task-a', task_name: 'Security Scanner',
    started_at: 1, status: 'succeeded', prompt: 'Scan the repo', cron_expr: '0 9 * * *',
  })
})

describe('OPCRunsTable', () => {
  it('renders the empty state when there are no runs', async () => {
    mockedRuns.mockResolvedValue([])
    renderTable()
    await waitFor(() => expect(screen.getByText('No runs yet')).toBeInTheDocument())
  })

  it('renders the six columns with the joined session/model data', async () => {
    mockedRuns.mockResolvedValue([
      row(),
      row({ run_id: 'run-2', task_name: 'Legacy run', status: 'failed', error_message: 'boom', session_id: undefined, model: undefined, cost_usd: 0.5, token_usage: 100 }),
    ])
    renderTable()
    await waitFor(() => expect(screen.getByText('Security Scanner')).toBeInTheDocument())
    expect(screen.getByText('Time')).toBeInTheDocument()
    expect(screen.getByText('Task / agent')).toBeInTheDocument()
    expect(screen.getByText('Status')).toBeInTheDocument()
    expect(screen.getByText('Cost')).toBeInTheDocument()
    expect(screen.getByText('Tokens')).toBeInTheDocument()
    expect(screen.getByText('Model')).toBeInTheDocument()
    expect(screen.getByText('$0.1234')).toBeInTheDocument()
    expect(screen.getByText('12,345 tok')).toBeInTheDocument()
    expect(screen.getByText('glm-4.7')).toBeInTheDocument()
    // succeeded → reads as the completed badge family.
    expect(screen.getByText('Completed')).toBeInTheDocument()
    // Session jump affordance only where a session exists.
    expect(screen.getByRole('button', { name: /Open session: Security Scanner/ })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: /Open session: Legacy run/ })).not.toBeInTheDocument()
  })

  it('jumps into the run session (switchSession + /chat) on click', async () => {
    mockedRuns.mockResolvedValue([row()])
    renderTable()
    fireEvent.click(await screen.findByRole('button', { name: /Open session: Security Scanner/ }))
    await waitFor(() => {
      expect(switchSession).toHaveBeenCalledWith('sess-9')
      expect(navigate).toHaveBeenCalledWith('/chat')
    })
  })

  it('expands the run detail on row click (prompt + cron + error)', async () => {
    mockedRuns.mockResolvedValue([
      row({ status: 'failed', error_message: 'boom' }),
    ])
    renderTable()
    fireEvent.click(await screen.findByText('Security Scanner'))
    await waitFor(() => expect(mockedDetail).toHaveBeenCalledWith('run-1'))
    expect(await screen.findByText('Scan the repo')).toBeInTheDocument()
    expect(screen.getByText(/0 9 \* \* \*/)).toBeInTheDocument()
    expect(screen.getByText('boom')).toBeInTheDocument()
  })

  it('collapses an expanded row on a second click', async () => {
    mockedRuns.mockResolvedValue([row()])
    renderTable()
    const name = await screen.findByText('Security Scanner')
    fireEvent.click(name)
    await screen.findByText('Scan the repo')
    fireEvent.click(name)
    await waitFor(() => expect(screen.queryByText('Scan the repo')).not.toBeInTheDocument())
  })
})

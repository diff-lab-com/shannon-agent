import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, within } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import OPCTask from '@/pages/OPCTask'

const ctx = vi.hoisted(() => ({
  tasks: [] as any[],
  agents: [] as any[],
  usage: { input_tokens: 100, output_tokens: 50, cost_usd: 0.05 },
  permissionRequest: null as any,
  respondPermission: vi.fn(),
  sessions: [] as any[],
  goalRunsBySession: {} as Record<string, any>,
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ctx,
}))

function renderOPCTask(path = '/opc/task') {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/opc/task" element={<OPCTask />} />
          <Route path="/opc/task/:id" element={<OPCTask />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

function resetCtx() {
  ctx.tasks = []
  ctx.agents = []
  ctx.permissionRequest = null
  ctx.respondPermission = vi.fn()
  ctx.sessions = []
  ctx.goalRunsBySession = {}
}

/** A pending request owned by an OPC agent-run session — approvable here. */
function agentRunPermission() {
  ctx.sessions = [{ id: 's1', title: 'Agent run session', is_agent_run: true }]
  ctx.permissionRequest = { request_id: 'r1', tool: 'bash', input: {}, risk: 'medium', session_id: 's1' }
}

describe('OPCTask', () => {
  it('renders Agent Workflow heading', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText('Agent Workflow')).toBeInTheDocument()
  })

  it('shows no agents message when empty', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText(/No agents in this workflow/)).toBeInTheDocument()
  })

  it('shows no task selected when no tasks', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText(/No task selected/)).toBeInTheDocument()
  })

  it('renders Execution Log heading', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText('Execution Log')).toBeInTheDocument()
  })

  it('shows no execution events when no agents', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText(/No execution events yet/)).toBeInTheDocument()
  })

  it('renders Efficiency Metrics', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText('Efficiency Metrics')).toBeInTheDocument()
  })

  it('shows session cost from usage', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText('$0.0500')).toBeInTheDocument()
  })

  it('shows agent count', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.getByText('0 Agents')).toBeInTheDocument()
  })

  it('does not show Human-in-the-Loop when no permission request pending', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    renderOPCTask()
    expect(screen.queryByText(/Human-in-the-Loop/)).not.toBeInTheDocument()
  })

  it('shows Human-in-the-Loop when a permission request is pending', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.permissionRequest = { request_id: 'r1', tool: 'bash', input: {}, risk: 'medium' }
    renderOPCTask()
    expect(screen.getByText(/Human-in-the-Loop Review/)).toBeInTheDocument()
  })

  it('shows agents in workflow when present', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Agent A', model: 'gpt-4', status: 'running', task: 'do stuff' }]
    renderOPCTask()
    expect(screen.getAllByText('Agent A').length).toBeGreaterThan(0)
    expect(screen.getByText('do stuff')).toBeInTheDocument()
  })

  it('shows task description when task found', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'My Task', status: 'running', description: 'A test task', assignee: 'Bot', priority: 'high' }]
    renderOPCTask()
    expect(screen.getAllByText('My Task').length).toBeGreaterThan(0)
    expect(screen.getByText('A test task')).toBeInTheDocument()
    expect(screen.getByText(/Assigned to: Bot/)).toBeInTheDocument()
    expect(screen.getByText(/Priority: high/)).toBeInTheDocument()
  })

  it('sends the pending request_id (not the task id) on Approve click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    agentRunPermission()
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    const dialog = screen.getByRole('alertdialog', { name: /Approve final merge\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Approve merge$/ }))
    expect(ctx.respondPermission).toHaveBeenCalledWith('r1', true, undefined)
  })

  it('sends the pending request_id (not the task id) on Rollback click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    agentRunPermission()
    renderOPCTask()
    fireEvent.click(screen.getByText('Rollback'))
    const dialog = screen.getByRole('alertdialog', { name: /Rollback task\?/i })
    fireEvent.click(within(dialog).getByRole('button', { name: /^Rollback$/ }))
    expect(ctx.respondPermission).toHaveBeenCalledWith('r1', false, undefined)
  })

  it('shows the owning session title in the confirm dialog', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    agentRunPermission()
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    const dialog = screen.getByRole('alertdialog', { name: /Approve final merge\?/i })
    expect(within(dialog).getByText(/Agent run session/)).toBeInTheDocument()
  })

  it('disables approval when the request belongs to a plain chat session', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.sessions = [{ id: 'chat1', title: 'My chat', is_agent_run: false }]
    ctx.permissionRequest = { request_id: 'r2', tool: 'bash', input: {}, risk: 'high', session_id: 'chat1' }
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    const dialog = screen.getByRole('alertdialog', { name: /Approve final merge\?/i })
    const confirm = within(dialog).getByRole('button', { name: /^Approve merge$/ })
    expect(confirm).toBeDisabled()
    fireEvent.click(confirm)
    expect(ctx.respondPermission).not.toHaveBeenCalled()
  })

  it('disables approval when the pending request has no owner session', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.permissionRequest = { request_id: 'r3', tool: 'bash', input: {}, risk: 'medium' }
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    const dialog = screen.getByRole('alertdialog', { name: /Approve final merge\?/i })
    expect(within(dialog).getByRole('button', { name: /^Approve merge$/ })).toBeDisabled()
  })

  it('allows approval when the request belongs to a goal-run session', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.goalRunsBySession = { goal1: { id: 'g1' } }
    ctx.permissionRequest = { request_id: 'r4', tool: 'bash', input: {}, risk: 'medium', session_id: 'goal1' }
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    const dialog = screen.getByRole('alertdialog', { name: /Approve final merge\?/i })
    expect(within(dialog).getByRole('button', { name: /^Approve merge$/ })).not.toBeDisabled()
  })

  it('shows revision input on Request Revision click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.permissionRequest = { request_id: 'r1', tool: 'bash', input: {}, risk: 'medium' }
    renderOPCTask()
    fireEvent.click(screen.getByText('Request Revision'))
    expect(screen.getByPlaceholderText(/Describe what needs to change/)).toBeInTheDocument()
  })

  it('disables Submit revision until the note has non-empty text', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    ctx.permissionRequest = { request_id: 'r1', tool: 'bash', input: {}, risk: 'medium' }
    renderOPCTask()
    fireEvent.click(screen.getByText('Request Revision'))
    const submit = screen.getByText('Submit revision')
    expect(submit).toBeDisabled()
    fireEvent.change(screen.getByPlaceholderText(/Describe what needs to change/), { target: { value: 'fix the bug' } })
    expect(submit).not.toBeDisabled()
  })

  it('shows Related Tasks in sidebar', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Related', status: 'completed' }]
    renderOPCTask()
    expect(screen.getByText('Related Tasks')).toBeInTheDocument()
  })
})

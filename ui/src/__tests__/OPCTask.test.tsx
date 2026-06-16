import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import OPCTask from '@/pages/OPCTask'

const ctx = vi.hoisted(() => ({
  tasks: [] as any[],
  agents: [] as any[],
  usage: { input_tokens: 100, output_tokens: 50, cost_usd: 0.05 },
  respondPermission: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
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
  ctx.respondPermission = vi.fn()
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

  it('does not show Human-in-the-Loop when no running tasks', () => {
    resetCtx()
    renderOPCTask()
    expect(screen.queryByText(/Human-in-the-Loop/)).not.toBeInTheDocument()
  })

  it('shows Human-in-the-Loop when running tasks exist', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
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

  it('calls respondPermission on Approve click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    renderOPCTask()
    fireEvent.click(screen.getByText('Approve Final Merge'))
    expect(ctx.respondPermission).toHaveBeenCalledWith('1', true)
  })

  it('calls respondPermission on Rollback click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    renderOPCTask()
    fireEvent.click(screen.getByText('Rollback'))
    expect(ctx.respondPermission).toHaveBeenCalledWith('1', false)
  })

  it('shows revision input on Request Revision click', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Test', status: 'running' }]
    renderOPCTask()
    fireEvent.click(screen.getByText('Request Revision'))
    expect(screen.getByPlaceholderText(/Describe what needs to change/)).toBeInTheDocument()
  })

  it('shows Related Tasks in sidebar', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Related', status: 'completed' }]
    renderOPCTask()
    expect(screen.getByText('Related Tasks')).toBeInTheDocument()
  })
})

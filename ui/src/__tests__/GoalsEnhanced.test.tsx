import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Goals from '@/pages/Goals'

const ctx = vi.hoisted(() => ({
  tasks: [] as any[],
  agents: [] as any[],
  respondPermission: vi.fn(),
  sendMessage: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
}))

function renderGoals() {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={['/goals']}>
        <Routes>
          <Route path="/goals" element={<Goals />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

function resetCtx() {
  ctx.tasks = []
  ctx.agents = []
  ctx.respondPermission = vi.fn()
  ctx.sendMessage = vi.fn()
}

describe('Goals Enhanced', () => {
  it('renders Task Management heading', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText('Task Management')).toBeInTheDocument()
  })

  it('shows Tasks Overview badge', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText('Tasks Overview')).toBeInTheDocument()
  })

  it('shows total task count', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText('0 total tasks')).toBeInTheDocument()
  })

  it('shows search input', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByPlaceholderText('Search tasks...')).toBeInTheDocument()
  })

  it('shows no tasks message when empty', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText(/No tasks yet/)).toBeInTheDocument()
  })

  it('shows Active Agents heading', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText(/Active Agents/)).toBeInTheDocument()
  })

  it('shows no agents active message', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText('No agents active')).toBeInTheDocument()
  })

  it('renders agents when present', () => {
    resetCtx()
    ctx.agents = [{ id: 'a1', name: 'Agent A', model: 'gpt-4', status: 'running' }]
    renderGoals()
    expect(screen.getByText('Agent A')).toBeInTheDocument()
  })

  it('renders tasks in tree view', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Tree Task', status: 'running', assignee: 'Bot' }]
    renderGoals()
    expect(screen.getAllByText('Tree Task').length).toBeGreaterThan(0)
    expect(screen.getAllByText(/Assigned to: Bot/).length).toBeGreaterThan(0)
  })

  it('shows Agent Reasoning when active tasks exist', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Active Task', status: 'in_progress' }]
    renderGoals()
    expect(screen.getByText('Agent Reasoning')).toBeInTheDocument()
    expect(screen.getByText('Approve')).toBeInTheDocument()
    expect(screen.getByText('Adjust')).toBeInTheDocument()
  })

  it('calls respondPermission on Approve', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Active', status: 'in_progress' }]
    renderGoals()
    fireEvent.click(screen.getByText('Approve'))
    expect(ctx.respondPermission).toHaveBeenCalledWith('1', true)
  })

  it('calls respondPermission on Adjust', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Active', status: 'in_progress' }]
    renderGoals()
    fireEvent.click(screen.getByText('Adjust'))
    expect(ctx.respondPermission).toHaveBeenCalledWith('1', false)
  })

  it('shows Task Summary in sidebar', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByText('Task Summary')).toBeInTheDocument()
  })

  it('renders completed tasks in sidebar', () => {
    resetCtx()
    ctx.tasks = [{ id: '1', title: 'Done Task', status: 'completed' }]
    renderGoals()
    expect(screen.getAllByText('Done Task').length).toBeGreaterThan(0)
  })

  // US-GOAL-06: Goal input field
  it('has goal text input', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByPlaceholderText('Ask about this goal...')).toBeInTheDocument()
  })

  // US-GOAL-07: AI assistant button
  it('has AI assistant button', () => {
    resetCtx()
    renderGoals()
    expect(screen.getByLabelText('AI assistant')).toBeInTheDocument()
  })

  it('sends suggestion message on AI assistant click', () => {
    resetCtx()
    ctx.sendMessage = vi.fn()
    renderGoals()
    fireEvent.click(screen.getByLabelText('AI assistant'))
    fireEvent.click(screen.getByText('Suggest Next Steps'))
    expect(ctx.sendMessage).toHaveBeenCalledWith(expect.stringContaining('Suggest next steps'))
  })
})

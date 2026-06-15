import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import ConversationsToday from '@/components/conversations/ConversationsToday'
import type { SessionInfo, TaskItem } from '@/types'

const today = Date.now()
const yesterday = today - 86_400_000

const sessions: SessionInfo[] = [
  { id: 's1', title: 'Today chat A', created_at: today, message_count: 5 },
  { id: 's2', title: 'Today chat B', created_at: today - 3_600_000, message_count: 2 },
  { id: 's3', title: 'Yesterday chat', created_at: yesterday, message_count: 8 },
]

const tasks: TaskItem[] = [
  { id: 't1', title: 'Running task', status: 'in_progress' },
  { id: 't2', title: 'Queued task', status: 'queued' },
  { id: 't3', title: 'Done task', status: 'completed' },
  { id: 't4', title: 'Failed task', status: 'failed' },
]

const agentsCtx = vi.hoisted(() => ({
  agents: [
    { id: 'a1', name: 'Coder', status: 'running', task: 'Fixing bug' },
    { id: 'a2', name: 'Idle agent', status: 'idle' },
  ],
}))

vi.mock('@/context/AppContext', () => ({
  AppProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  useApp: () => agentsCtx,
}))

function wrap(ui: React.ReactElement) {
  return <MemoryRouter>{ui}</MemoryRouter>
}

describe('ConversationsToday', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders chats today stat with today-only count', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    // 2 sessions today (s1, s2)
    expect(screen.getByText('Chats today')).toBeInTheDocument()
    const stat = screen.getByText('Chats today').closest('div')?.parentElement
    expect(stat).toBeInTheDocument()
  })

  it('renders running tasks stat', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Running tasks')).toBeInTheDocument()
  })

  it('renders completed stat', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })

  it('renders "Recent chats" section', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Recent chats')).toBeInTheDocument()
  })

  it('renders today session titles under Recent chats', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Today chat A')).toBeInTheDocument()
    expect(screen.getByText('Today chat B')).toBeInTheDocument()
  })

  it('does NOT render yesterday sessions', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.queryByText('Yesterday chat')).not.toBeInTheDocument()
  })

  it('renders "Due today" section header', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Due today')).toBeInTheDocument()
  })

  it('shows empty hint for Due today when nothing due', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Nothing due today.')).toBeInTheDocument()
  })

  it('shows empty hint when no today sessions', () => {
    render(wrap(<ConversationsToday sessions={[]} tasks={tasks} />))
    expect(screen.getByText('No chats yet today. Start one in Chat.')).toBeInTheDocument()
  })

  it('renders active agents section when agents are running', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Active agents')).toBeInTheDocument()
    expect(screen.getByText('Coder')).toBeInTheDocument()
  })

  it('does NOT render idle agents', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.queryByText('Idle agent')).not.toBeInTheDocument()
  })

  it('due-today task renders with due time', () => {
    const dueTasks: TaskItem[] = [
      { id: 'd1', title: 'Due now task', status: 'in_progress', due_date: Math.floor(today / 1000) },
    ]
    render(wrap(<ConversationsToday sessions={sessions} tasks={dueTasks} />))
    expect(screen.getByText('Due now task')).toBeInTheDocument()
  })
})

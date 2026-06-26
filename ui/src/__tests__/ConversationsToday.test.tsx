import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import ConversationsToday from '@/components/conversations/ConversationsToday'
import type { SessionInfo, TaskItem } from '@/types'

// Pin to noon today so subtracting a few hours never crosses midnight
// (tests are time-of-day sensitive otherwise).
const NOW = new Date()
const today = new Date(NOW.getFullYear(), NOW.getMonth(), NOW.getDate(), 12, 0, 0, 0).getTime()
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
  useApp: () => ({ ...agentsCtx, switchSession: vi.fn() }),
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

  it('renders Weekly Active Conversations hero card', () => {
    render(wrap(<ConversationsToday sessions={sessions} tasks={tasks} />))
    expect(screen.getByText('Weekly Active Conversations')).toBeInTheDocument()
    expect(screen.getByText(/chats in the last 7 days/i)).toBeInTheDocument()
  })

  it('WAC counts sessions touched in last 7 days', () => {
    const old = today - 8 * 86_400_000 // 8 days ago — outside WAC window
    const mixedSessions: SessionInfo[] = [
      ...sessions,
      { id: 'sold', title: 'Old chat', created_at: old, message_count: 1 },
    ]
    render(wrap(<ConversationsToday sessions={mixedSessions} tasks={tasks} />))
    // sessions has 3 (today, today-1h, yesterday); old is 8d so excluded → WAC = 3
    const wacSection = screen.getByLabelText('Weekly Active Conversations')
    expect(wacSection).toHaveTextContent('3')
  })

  it('WAC is 0 when no sessions in last 7 days', () => {
    render(wrap(<ConversationsToday sessions={[]} tasks={tasks} />))
    const wacSection = screen.getByLabelText('Weekly Active Conversations')
    expect(wacSection).toHaveTextContent('0')
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

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import AgentMessagesPanel from '@/components/tasks/AgentMessagesPanel'

const listMessages = vi.hoisted(() => vi.fn())
const listTeams = vi.hoisted(() => vi.fn())
const recordMessage = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  default: {},
  listAgentMessages: (...args: unknown[]) => listMessages(...args),
  listAgentMessageTeams: (...args: unknown[]) => listTeams(...args),
  recordAgentMessage: (...args: unknown[]) => recordMessage(...args),
}))

const sampleMessage = {
  message_id: 'msg-1',
  team: '<adhoc>',
  from: 'lead',
  to: 'worker',
  content_preview: 'Please review PR #42',
  content_kind: 'text' as const,
  priority: 'high' as const,
  timestamp: 1717000000,
}

beforeEach(() => {
  listMessages.mockReset()
  listTeams.mockReset()
  recordMessage.mockReset()
  listTeams.mockResolvedValue([])
})

describe('AgentMessagesPanel', () => {
  it('renders loading state initially', () => {
    listMessages.mockReturnValue(new Promise(() => {}))
    render(<AgentMessagesPanel />)
    // Header always rendered
    expect(screen.getByText('Agent Messages')).toBeInTheDocument()
  })

  it('renders empty state when no messages', async () => {
    listMessages.mockResolvedValue([])
    render(<AgentMessagesPanel />)
    await waitFor(() =>
      expect(screen.getByText(/No inter-agent messages recorded yet/)).toBeInTheDocument(),
    )
  })

  it('renders team-scoped empty state', async () => {
    listMessages.mockResolvedValue([])
    render(<AgentMessagesPanel team="alpha" />)
    await waitFor(() =>
      expect(screen.getByText(/No messages recorded for team "alpha"/)).toBeInTheDocument(),
    )
  })

  it('renders error state when fetch fails', async () => {
    listMessages.mockRejectedValue(new Error('Permission denied'))
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText('Permission denied')).toBeInTheDocument())
  })

  it('renders messages with from→to and content preview', async () => {
    listMessages.mockResolvedValue([sampleMessage])
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText('lead')).toBeInTheDocument())
    expect(screen.getByText('worker')).toBeInTheDocument()
    expect(screen.getByText('Please review PR #42')).toBeInTheDocument()
  })

  it('renders HIGH priority badge', async () => {
    listMessages.mockResolvedValue([sampleMessage])
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText('HIGH')).toBeInTheDocument())
  })

  it('renders broadcast recipient with campaign icon', async () => {
    listMessages.mockResolvedValue([{ ...sampleMessage, to: '*' }])
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText('lead')).toBeInTheDocument())
    expect(screen.getByText('*')).toBeInTheDocument()
  })

  it('shows aggregated teams count when no team filter', async () => {
    listMessages.mockResolvedValue([sampleMessage])
    listTeams.mockResolvedValue(['<adhoc>', 'alpha', 'beta'])
    render(<AgentMessagesPanel />)
    await waitFor(() =>
      expect(screen.getByText(/Aggregated from 3 teams/)).toBeInTheDocument(),
    )
  })

  it('hides aggregated count when team filter is set', async () => {
    listMessages.mockResolvedValue([sampleMessage])
    listTeams.mockResolvedValue(['alpha'])
    render(<AgentMessagesPanel team="alpha" />)
    await waitFor(() => expect(screen.getByText('lead')).toBeInTheDocument())
    expect(screen.queryByText(/Aggregated from/)).not.toBeInTheDocument()
  })

  it('sends a test message via the inject form', async () => {
    listMessages.mockResolvedValue([])
    recordMessage.mockResolvedValue('new-msg-id')
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText(/Record test message/)).toBeInTheDocument())

    fireEvent.click(screen.getByText('Record test message'))
    const contentInput = await screen.findByPlaceholderText(/Message content/)
    fireEvent.change(contentInput, { target: { value: 'Hello from test' } })
    fireEvent.click(screen.getByText('Send'))

    await waitFor(() => expect(recordMessage).toHaveBeenCalled())
    expect(recordMessage).toHaveBeenCalledWith(
      '<adhoc>',
      'lead',
      '*',
      'Hello from test',
      'normal',
    )
  })

  it('disables Send button when content is empty', async () => {
    listMessages.mockResolvedValue([])
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText(/Record test message/)).toBeInTheDocument())
    fireEvent.click(screen.getByText('Record test message'))
    const sendBtn = await screen.findByText('Send')
    expect(sendBtn).toBeDisabled()
  })

  it('toggles auto-refresh', async () => {
    listMessages.mockResolvedValue([])
    render(<AgentMessagesPanel />)
    await waitFor(() => expect(screen.getByText('Agent Messages')).toBeInTheDocument())
    const checkbox = screen.getByLabelText('Auto-refresh') as HTMLInputElement
    expect(checkbox.checked).toBe(true)
    fireEvent.click(checkbox)
    expect(checkbox.checked).toBe(false)
  })
})

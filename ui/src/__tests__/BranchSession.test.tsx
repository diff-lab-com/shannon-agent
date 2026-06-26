import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, within, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import Chat from '@/pages/Chat'

const ctx = vi.hoisted(() => ({
  messages: [
    { role: 'user' as const, content: 'Hello', timestamp: 1000 },
    { role: 'assistant' as const, content: 'Hi there!', timestamp: 2000 },
    { role: 'user' as const, content: 'How are you?', timestamp: 3000 },
  ] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  usage: null as any,
  sessions: [
    { id: 'session-1', title: 'Test Session', created_at: 0, message_count: 3 },
  ] as any[],
  currentSessionId: 'session-1' as string | null,
  error: null as string | null,
  sendMessage: vi.fn(),
  cancelQuery: vi.fn(),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  renameSession: vi.fn(),
  refreshSessions: vi.fn(),
}))

vi.mock('@/context/AppContext', () => ({
  useApp: () => ctx,
}))

function renderChat() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Chat />
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Branch Session feature', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    ctx.currentSessionId = 'session-1'
    ctx.messages = [
      { role: 'user' as const, content: 'Hello', timestamp: 1000 },
      { role: 'assistant' as const, content: 'Hi there!', timestamp: 2000 },
      { role: 'user' as const, content: 'How are you?', timestamp: 3000 },
    ]
    // Mock the functions to return resolved promises
    ctx.refreshSessions = vi.fn().mockResolvedValue(undefined)
    ctx.switchSession = vi.fn().mockResolvedValue(undefined)
  })

  it('renders branch button on user messages', () => {
    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    expect(branchButtons).toHaveLength(2)
  })

  it('does not render branch button on assistant messages', () => {
    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    // Only user messages should have branch buttons
    expect(branchButtons).toHaveLength(2)
  })

  it('invokes branchSession with correct args when branch button is clicked', async () => {
    const branchSessionSpy = vi.spyOn(api, 'branchSession').mockResolvedValue({
      id: 'branch-1',
      title: 'Branch',
      created_at: 0,
      message_count: 2,
    })

    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    fireEvent.click(branchButtons[0])

    const dialog = await screen.findByRole('alertdialog')
    const confirmBtn = within(dialog).getByRole('button', { name: /^Branch$/i })
    fireEvent.click(confirmBtn)

    await waitFor(() => {
      expect(branchSessionSpy).toHaveBeenCalledWith('session-1', 0)
    })
    expect(ctx.refreshSessions).toHaveBeenCalled()
    expect(ctx.switchSession).toHaveBeenCalledWith('branch-1')
  })

  it('does not call branchSession when user cancels confirm dialog', async () => {
    const branchSessionSpy = vi.spyOn(api, 'branchSession').mockResolvedValue({
      id: 'branch-1',
      title: 'Branch',
      created_at: 0,
      message_count: 2,
    })

    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    fireEvent.click(branchButtons[0])

    const dialog = await screen.findByRole('alertdialog')
    const cancelBtn = within(dialog).getByRole('button', { name: /cancel/i })
    fireEvent.click(cancelBtn)

    await new Promise(resolve => setTimeout(resolve, 0))

    expect(branchSessionSpy).not.toHaveBeenCalled()
    expect(ctx.refreshSessions).not.toHaveBeenCalled()
  })

  it('disables branch button when no current session', () => {
    ctx.currentSessionId = null
    renderChat()

    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    expect(branchButtons[0]).toBeDisabled()
  })

  it('uses correct message index for each message', async () => {
    const branchSessionSpy = vi.spyOn(api, 'branchSession')
      .mockResolvedValueOnce({
        id: 'branch-1',
        title: 'Branch',
        created_at: 0,
        message_count: 2,
      })
      .mockResolvedValueOnce({
        id: 'branch-2',
        title: 'Branch 2',
        created_at: 0,
        message_count: 3,
      })

    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)

    // Click first user message (index 0)
    fireEvent.click(branchButtons[0])
    let dialog = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog).getByRole('button', { name: /^Branch$/i }))
    await waitFor(() => {
      expect(branchSessionSpy).toHaveBeenLastCalledWith('session-1', 0)
    })

    // Click second user message (index 2, since it's the third message overall)
    fireEvent.click(branchButtons[1])
    dialog = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog).getByRole('button', { name: /^Branch$/i }))
    await waitFor(() => {
      expect(branchSessionSpy).toHaveBeenLastCalledWith('session-1', 2)
    })
  })
})

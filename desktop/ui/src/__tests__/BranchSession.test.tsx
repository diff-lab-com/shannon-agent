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
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn(),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  renameSession: vi.fn(),
  refreshSessions: vi.fn(),
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ctx,
}))
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
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

  it('renders branch button on every message', () => {
    // P2-5d: branch-from-anywhere is the polished behaviour; matches
    // Claude / ChatGPT where any message can fork the conversation.
    renderChat()
    const branchButtons = screen.getAllByLabelText(/Branch from this message/i)
    expect(branchButtons).toHaveLength(3) // user / assistant / user
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

    // Click the LAST branch button — the third overall message
    // (P2-5d every message gets a branch button; user / assistant / user).
    const last = branchButtons[branchButtons.length - 1]
    fireEvent.click(last)
    dialog = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog).getByRole('button', { name: /^Branch$/i }))
    await waitFor(() => {
      expect(branchSessionSpy).toHaveBeenLastCalledWith('session-1', 2)
    })
  })
})

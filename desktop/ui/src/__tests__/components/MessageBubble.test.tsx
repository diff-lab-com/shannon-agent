import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { MessageBubble } from '@/components/chat/MessageBubble'
import type { ChatMessage } from '@/types'

const useChatMock = vi.hoisted(() => vi.fn())

vi.mock('@/context/ChatContext', () => ({
  useChat: useChatMock,
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({
    currentSessionId: 's-1',
    switchSession: vi.fn(),
    refreshSessions: vi.fn(),
  }),
}))

// Default chat-slice stub; individual tests can override once via
// useChatMock.mockReturnValueOnce(...).
useChatMock.mockImplementation(() => ({
  sendMessage: vi.fn(),
  feedback: {},
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  isQuerying: false,
}))

const wrap = (ui: React.ReactNode) => <MemoryRouter>{ui}</MemoryRouter>

const baseUser = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 'm1',
  role: 'user',
  content: 'hello there',
  timestamp: Date.UTC(2026, 0, 1, 12, 30),
  ...overrides,
} as ChatMessage)

const baseAssistant = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 'm2',
  role: 'assistant',
  content: 'world',
  timestamp: Date.UTC(2026, 0, 1, 12, 31),
  ...overrides,
} as ChatMessage)

const baseTool = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: 'm3',
  role: 'tool',
  content: 'tool ran',
  timestamp: Date.UTC(2026, 0, 1, 12, 32),
  ...overrides,
} as ChatMessage)

describe('MessageBubble — header (P2-5d)', () => {
  it('renders a "You" role label on user messages', () => {
    render(wrap(<MessageBubble message={baseUser()} messageIndex={0} onViewDiff={vi.fn()} />))
    // Header label is uppercased via Tailwind, so case-insensitive match.
    expect(screen.getByText(/you/i)).toBeInTheDocument()
  })

  it('renders an "Assistant" role label on assistant messages', () => {
    render(wrap(<MessageBubble message={baseAssistant()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByText(/assistant/i)).toBeInTheDocument()
  })

  it('renders a "Tool" role label on tool messages', () => {
    render(wrap(<MessageBubble message={baseTool()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByText(/^tool$/i)).toBeInTheDocument()
  })

  it('renders the user message content verbatim', () => {
    render(wrap(<MessageBubble message={baseUser({ content: 'good morning' })} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByText('good morning')).toBeInTheDocument()
  })
})

describe('MessageBubble — hover actions have accessible names (P2-5d a11y)', () => {
  it('user bubble has a Copy message button with an accessible name', () => {
    render(wrap(<MessageBubble message={baseUser()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByRole('button', { name: /copy message/i })).toBeInTheDocument()
  })

  it('user bubble has a Branch session button with an accessible name', () => {
    render(wrap(<MessageBubble message={baseUser()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByRole('button', { name: /branch from this message/i })).toBeInTheDocument()
  })

  it('assistant bubble has Like / Branch buttons, regenerate only when payload present (B1)', () => {
    render(wrap(<MessageBubble message={baseAssistant()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByRole('button', { name: /like message/i })).toBeInTheDocument()
    // B1 §4-7: without a regenerate payload (only the LAST assistant message
    // of a rewindable session gets one), the button must not render.
    expect(screen.queryByRole('button', { name: /regenerate/i })).toBeNull()
    expect(screen.getByRole('button', { name: /branch from this message/i })).toBeInTheDocument()
  })

  it('renders the regenerate button when the regenerate payload is present', () => {
    render(
      wrap(
        <MessageBubble
          message={baseAssistant()}
          messageIndex={0}
          onViewDiff={vi.fn()}
          regenerate={{ turnIndex: 0, content: 'question', attachmentPaths: [] }}
          onRewind={vi.fn().mockResolvedValue(undefined)}
        />,
      ),
    )
    expect(screen.getByRole('button', { name: /regenerate response/i })).toBeInTheDocument()
  })

  it('tool bubble hides Like / Regenerate (those actions only apply to text)', () => {
    render(wrap(<MessageBubble message={baseTool()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.queryByRole('button', { name: /like message/i })).toBeNull()
    expect(screen.queryByRole('button', { name: /regenerate/i })).toBeNull()
  })
})

describe('MessageBubble — a11y structure', () => {
  it('renders the assistant message with a data-message-from attribute', () => {
    const { container } = render(wrap(<MessageBubble message={baseAssistant()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(container.querySelector('[data-message-from="assistant"]')).not.toBeNull()
  })

  it('renders the user message with a data-message-from attribute', () => {
    const { container } = render(wrap(<MessageBubble message={baseUser()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(container.querySelector('[data-message-from="user"]')).not.toBeNull()
  })

  it('exposes the timestamp via <time dateTime>', () => {
    const when = Date.UTC(2026, 0, 1, 12, 30)
    render(wrap(
      <MessageBubble message={baseAssistant({ timestamp: when })} messageIndex={0} onViewDiff={vi.fn()} />
    ))
    const time = screen.getAllByText((_, el) => !!el?.tagName.match(/TIME/i))[0]
    expect(time).toBeDefined()
  })

  // B1 P2-17 — the header used to be blanket aria-hidden (role/timestamp
  // invisible to screen readers). Only the decorative bits stay hidden now.
  it('keeps the message header (role + timestamp) accessible', () => {
    const when = Date.UTC(2026, 0, 1, 12, 30)
    render(wrap(
      <MessageBubble message={baseAssistant({ timestamp: when })} messageIndex={0} onViewDiff={vi.fn()} />
    ))
    const header = screen.getByText(/assistant/i).closest('div')!
    expect(header).not.toHaveAttribute('aria-hidden', 'true')
    expect(screen.getByText(/assistant/i)).not.toHaveAttribute('aria-hidden', 'true')
  })
})

describe('MessageBubble — rewind (/rewind desktop)', () => {
  it('hides the rewind button when the message is not rewindable', () => {
    render(wrap(<MessageBubble message={baseUser()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.queryByRole('button', { name: 'Rewind conversation to this message' })).not.toBeInTheDocument()
  })

  it('shows the rewind button and fires onRewind after confirmation', async () => {
    const onRewind = vi.fn().mockResolvedValue(undefined)
    render(
      wrap(
        <MessageBubble
          message={baseUser()}
          messageIndex={0}
          onViewDiff={vi.fn()}
          rewindTurnIndex={1}
          onRewind={onRewind}
        />,
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: 'Rewind conversation to this message' }))
    // Confirmation gate — files may be reverted, so rewind is opt-in.
    expect(screen.getByText('Rewind to here?')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'Rewind' }))
    await waitFor(() => expect(onRewind).toHaveBeenCalledWith(1))
  })
})

describe('MessageBubble — persisted feedback (PM-12)', () => {
  it('assistant bubble exposes like and dislike toggles', () => {
    render(wrap(<MessageBubble message={baseAssistant()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByRole('button', { name: 'Like message' })).toHaveAttribute('aria-pressed', 'false')
    expect(screen.getByRole('button', { name: 'Dislike response' })).toHaveAttribute('aria-pressed', 'false')
  })
})

// B1 §4-7 — TRUE regenerate: rewind to the checkpoint before the preceding
// user turn, then re-send that turn's text (plus attachment paths). The old
// canned-prompt fake is gone.
describe('MessageBubble — true regenerate (B1 §4-7)', () => {
  const regeneratePayload = {
    turnIndex: 1,
    content: 'the original question',
    attachmentPaths: ['/tmp/report.pdf'],
  }

  it('rewinds then resends the ORIGINAL user turn (text + attachments)', async () => {
    const onRewind = vi.fn().mockResolvedValue(undefined)
    const sendMessage = vi.fn().mockResolvedValue(undefined)
    useChatMock.mockReturnValueOnce({
      sendMessage,
      feedback: {},
      recordFeedback: vi.fn().mockResolvedValue(undefined),
      isQuerying: false,
    } as any)
    render(
      wrap(
        <MessageBubble
          message={baseAssistant()}
          messageIndex={4}
          onViewDiff={vi.fn()}
          regenerate={regeneratePayload}
          onRewind={onRewind}
        />,
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: /regenerate response/i }))
    await waitFor(() => expect(onRewind).toHaveBeenCalledWith(1))
    await waitFor(() => expect(sendMessage).toHaveBeenCalledWith('the original question', ['/tmp/report.pdf']))
  })

  it('sends text only when the turn had no attachments', async () => {
    const onRewind = vi.fn().mockResolvedValue(undefined)
    const sendMessage = vi.fn().mockResolvedValue(undefined)
    useChatMock.mockReturnValueOnce({
      sendMessage,
      feedback: {},
      recordFeedback: vi.fn().mockResolvedValue(undefined),
      isQuerying: false,
    } as any)
    render(
      wrap(
        <MessageBubble
          message={baseAssistant()}
          messageIndex={4}
          onViewDiff={vi.fn()}
          regenerate={{ turnIndex: 0, content: 'plain question', attachmentPaths: [] }}
          onRewind={onRewind}
        />,
      ),
    )
    fireEvent.click(screen.getByRole('button', { name: /regenerate response/i }))
    await waitFor(() => expect(sendMessage).toHaveBeenCalledWith('plain question', undefined))
  })

  it('is disabled while the session is querying', () => {
    useChatMock.mockReturnValueOnce({
      sendMessage: vi.fn(),
      feedback: {},
      recordFeedback: vi.fn().mockResolvedValue(undefined),
      isQuerying: true,
    } as any)
    render(
      wrap(
        <MessageBubble
          message={baseAssistant()}
          messageIndex={4}
          onViewDiff={vi.fn()}
          regenerate={regeneratePayload}
          onRewind={vi.fn().mockResolvedValue(undefined)}
        />,
      ),
    )
    expect(screen.getByRole('button', { name: /regenerate response/i })).toBeDisabled()
  })
})

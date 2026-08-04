import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { MessageBubble } from '@/components/chat/MessageBubble'
import type { ChatMessage } from '@/types'

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ({ sendMessage: vi.fn() }),
}))
vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({
    currentSessionId: 's-1',
    switchSession: vi.fn(),
    refreshSessions: vi.fn(),
  }),
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

  it('assistant bubble has Like / Regenerate / Branch buttons', () => {
    render(wrap(<MessageBubble message={baseAssistant()} messageIndex={0} onViewDiff={vi.fn()} />))
    expect(screen.getByRole('button', { name: /like message/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /regenerate/i })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /branch from this message/i })).toBeInTheDocument()
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
})

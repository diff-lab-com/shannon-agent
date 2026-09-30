import { describe, expect, it, vi } from 'vitest'
import { render, screen, cleanup } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'

import { MessageBubble } from '@/components/chat/MessageBubble'
import type { ChatMessage } from '@/types'

/**
 * office Wave 1 (A5+B8a) — MessageBubble × FileCard integration:
 *   1. non-image user attachments render as a FileCard (open/reveal/save-as)
 *      instead of the old chip → lightbox "Open externally" detour;
 *   2. image attachments keep the chip + lightbox;
 *   3. a COMPLETED file-mutating tool with a path input surfaces the
 *      generated file as a FileCard under the tool block — read-only tools
 *      and failed writes never do.
 *
 * Uses the global setup mocks only (tauri-api is fully mocked there); the
 * FileCard interaction flows are covered exhaustively in FileCard.test.tsx.
 */

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

useChatMock.mockImplementation(() => ({
  sendMessage: vi.fn(),
  feedback: {},
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  isQuerying: false,
}))

const wrap = (ui: React.ReactNode) => <MemoryRouter>{ui}</MemoryRouter>

afterEach(() => {
  cleanup()
})

describe('MessageBubble — attachment FileCard (office Wave 1)', () => {
  it('renders a non-image attachment as a FileCard with the three actions', () => {
    const message = {
      role: 'user',
      content: 'summarize this',
      timestamp: Date.UTC(2026, 0, 1),
      file_attachments: [{ name: 'report.docx', path: '/tmp/report.docx', size: 2048 }],
    } as unknown as ChatMessage
    render(wrap(<MessageBubble message={message} messageIndex={0} onViewDiff={vi.fn()} />))

    const card = screen.getByTestId('file-card')
    expect(card).toHaveTextContent('report.docx')
    expect(screen.getByRole('button', { name: 'Open attachment' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Show in folder' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Save as…' })).toBeInTheDocument()
  })

  it('keeps image attachments on the chip + lightbox path (no FileCard)', () => {
    const message = {
      role: 'user',
      content: 'look at this',
      timestamp: Date.UTC(2026, 0, 1),
      file_attachments: [{ name: 'pic.png', path: '/tmp/pic.png', size: 5 }],
    } as unknown as ChatMessage
    render(wrap(<MessageBubble message={message} messageIndex={0} onViewDiff={vi.fn()} />))

    expect(screen.queryByTestId('file-card')).toBeNull()
    expect(screen.getByRole('button', { name: 'Open attachment' })).toBeInTheDocument()
  })
})

describe('MessageBubble — generated-file FileCard under the tool block', () => {
  const assistantWith = (toolCalls: ChatMessage['tool_calls']) =>
    ({
      role: 'assistant',
      content: 'done',
      timestamp: Date.UTC(2026, 0, 1),
      tool_calls: toolCalls,
    }) as unknown as ChatMessage

  it('renders a FileCard for a completed file-mutating tool with a path input', () => {
    render(
      wrap(
        <MessageBubble
          message={assistantWith([
            {
              tool_use_id: 't1',
              tool_name: 'write_file',
              tool_input: { path: '/tmp/out/report.md', content: '# hi' },
              result: 'written',
              status: 'completed',
            },
          ])}
          messageIndex={0}
          onViewDiff={vi.fn()}
        />,
      ),
    )
    const card = screen.getByTestId('file-card')
    expect(card).toHaveTextContent('report.md')
    expect(screen.getByRole('button', { name: 'Save as…' })).toBeInTheDocument()
  })

  it('does NOT render a FileCard for a failed write', () => {
    render(
      wrap(
        <MessageBubble
          message={assistantWith([
            {
              tool_use_id: 't1',
              tool_name: 'write_file',
              tool_input: { path: '/tmp/out/report.md', content: '# hi' },
              result: 'denied',
              status: 'error',
              is_error: true,
            },
          ])}
          messageIndex={0}
          onViewDiff={vi.fn()}
        />,
      ),
    )
    expect(screen.queryByTestId('file-card')).toBeNull()
  })

  it('does NOT render a FileCard for read-only tools, even with a path input', () => {
    render(
      wrap(
        <MessageBubble
          message={assistantWith([
            {
              tool_use_id: 't1',
              tool_name: 'read_file',
              tool_input: { path: '/tmp/src/main.rs' },
              result: 'fn main() {}',
              status: 'completed',
            },
          ])}
          messageIndex={0}
          onViewDiff={vi.fn()}
        />,
      ),
    )
    expect(screen.queryByTestId('file-card')).toBeNull()
  })
})

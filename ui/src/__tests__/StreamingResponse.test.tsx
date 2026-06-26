import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import StreamingResponse from '@/components/chat/StreamingResponse'
import type { ToolCall } from '@/types'

vi.mock('@/components/chat/Markdown', () => ({
  Markdown: ({ children }: { children: string }) => <div data-testid="markdown">{children}</div>,
}))
vi.mock('@/components/chat/MessageBubble', () => ({
  ToolCallDisplay: ({ toolCall }: { toolCall: ToolCall }) => (
    <div data-testid="tool-call" data-tool={toolCall.name} />
  ),
}))

const makeToolCall = (overrides: Partial<ToolCall> = {}): ToolCall => ({
  tool_use_id: 'tc-1',
  name: 'read_file',
  input: {},
  status: 'running',
  ...overrides,
} as ToolCall)

describe('StreamingResponse', () => {
  it('renders streaming text via Markdown', () => {
    render(
      <StreamingResponse
        streamingText="Hello there"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(screen.getByTestId('markdown')).toHaveTextContent('Hello there')
  })

  it('renders thinking block when thinkingText is non-empty', () => {
    render(
      <StreamingResponse
        streamingText=""
        thinkingText="Considering options"
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(screen.getByText('Considering options')).toBeInTheDocument()
  })

  it('omits thinking block when thinkingText is empty', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="Response"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('.uppercase')).toBeNull()
  })

  it('renders active tool calls', () => {
    render(
      <StreamingResponse
        streamingText=""
        thinkingText=""
        activeToolCalls={[makeToolCall({ name: 'bash' }), makeToolCall({ name: 'edit_file', tool_use_id: 'tc-2' })]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(screen.getAllByTestId('tool-call')).toHaveLength(2)
  })

  it('exposes aria-live=polite for screen readers', () => {
    const { container } = render(
      <StreamingResponse
        streamingText=""
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('[aria-live="polite"]')).not.toBeNull()
  })
})

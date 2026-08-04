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
    // Reasoning is a collapsible. The header is always visible; the
    // body is collapsed by default. Use the header label localized
    // value via the "Thinking" alias that the intl mock supplies.
    expect(screen.getByText('Thinking')).toBeInTheDocument()
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
    // No collapsible wrapper when there's nothing to think about.
    expect(container.querySelector('[data-reasoning]')).toBeNull()
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

  // P2-5d — typing cursor + jump-to-bottom + role=log
  it('renders a typing cursor when streamingText is non-empty', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="partial answer"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    const cursor = container.querySelector('.streaming-cursor')
    expect(cursor).not.toBeNull()
    expect(cursor?.getAttribute('aria-hidden')).toBe('true')
  })

  it('renders the role=log list for message history conformance', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="x"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('[role="log"]')).not.toBeNull()
  })

  it('hides the jump-to-bottom button by default', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="x"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    // The button should only be rendered when scrolled away; in jsdom
    // we never get a real scroll, so it's hidden by default.
    expect(container.querySelector('button[aria-label*="Jump" i]')).toBeNull()
  })
})

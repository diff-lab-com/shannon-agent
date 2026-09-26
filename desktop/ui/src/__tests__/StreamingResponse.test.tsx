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

  // B2 P2-17 — the streaming log used to announce every token via a whole
  // region aria-live=polite. Announcements moved to MessageArea's
  // StreamStatusRegion (state transitions only), so this component must
  // carry NO live region of its own.
  it('carries no aria-live region (status announcements live in MessageArea)', () => {
    const { container } = render(
      <StreamingResponse
        streamingText=""
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('[aria-live]')).toBeNull()
    expect(container.querySelector('[role="status"]')).toBeNull()
  })

  // P2-5d — typing cursor + role=log
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

  // B2 P2-17 — role="log" implies aria-live=polite, so the streaming log
  // container dropped it along with the explicit live region.
  it('renders the streaming log without an implicit live role', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="x"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('[role="log"]')).toBeNull()
  })

  // B0 P1-1 — the dead inner scroll guard (and its conditional
  // jump-to-bottom button) was removed: this component's inner div never
  // scrolls, so scroll-back belongs to MessageArea's scroll-to-latest FAB.
  it('renders no inner jump-to-bottom control (the scroll parent owns it)', () => {
    const { container } = render(
      <StreamingResponse
        streamingText="x"
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(container.querySelector('button[aria-label*="Jump" i]')).toBeNull()
  })
})

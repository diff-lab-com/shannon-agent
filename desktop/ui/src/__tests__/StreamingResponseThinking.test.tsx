// Settings R3 T9 — the live stream's thinking block obeys the display pref:
// 'none' hides it even while thinkingText is actively streaming in;
// 'first'/'all' stream as usual (the in-flight run IS the current turn).

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import StreamingResponse from '@/components/chat/StreamingResponse'
import { SHOW_THINKING_PREF_KEY } from '@/lib/thinkingPref'

vi.mock('@/components/chat/Markdown', () => ({
  Markdown: ({ children }: { children: string }) => <div data-testid="markdown">{children}</div>,
}))
vi.mock('@/components/chat/MessageBubble', () => ({
  ToolCallDisplay: () => <div data-testid="tool-call" />,
  SubagentBlock: () => <div data-testid="subagent-block" />,
}))

function renderStreaming(thinkingText: string) {
  return render(
    <StreamingResponse
      streamingText="partial answer"
      thinkingText={thinkingText}
      activeToolCalls={[]}
      onViewDiff={vi.fn()}
    />,
  )
}

describe('StreamingResponse thinking display pref (Settings R3 T9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('renders the thinking block under the default "all" tier', () => {
    renderStreaming('Live reasoning')
    expect(screen.getByText('Thinking')).toBeInTheDocument()
  })

  it('streams the thinking block under the "first" tier (current turn)', () => {
    window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'first')
    renderStreaming('Live reasoning')
    expect(screen.getByText('Thinking')).toBeInTheDocument()
  })

  it('hides the thinking block under the "none" tier while streaming', () => {
    window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'none')
    renderStreaming('Live reasoning')
    expect(screen.queryByText('Thinking')).not.toBeInTheDocument()
    expect(screen.queryByText('Live reasoning')).not.toBeInTheDocument()
    // The streamed answer itself is untouched.
    expect(screen.getByTestId('markdown')).toHaveTextContent('partial answer')
  })
})

// Settings R3 T9 — the live stream's thinking block obeys the display pref:
// 'none' hides it even while thinkingText is actively streaming in;
// 'first'/'all' stream as usual (the in-flight run IS the current turn).
// R3 §4 (Reasoning 头分态): the live header is always the IN-PROGRESS state
// 「思考中…」/「Thinking…」 — the completed 「已思考/Thought」 label belongs to
// MessageBubble's committed block and must never appear here.

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
    expect(screen.getByText('Thinking…')).toBeInTheDocument()
  })

  it('streams the thinking block under the "first" tier (current turn)', () => {
    window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'first')
    renderStreaming('Live reasoning')
    expect(screen.getByText('Thinking…')).toBeInTheDocument()
  })

  it('hides the thinking block under the "none" tier while streaming', () => {
    window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'none')
    renderStreaming('Live reasoning')
    expect(screen.queryByText('Thinking…')).not.toBeInTheDocument()
    expect(screen.queryByText('Live reasoning')).not.toBeInTheDocument()
    // The streamed answer itself is untouched.
    expect(screen.getByTestId('markdown')).toHaveTextContent('partial answer')
  })

  // R3 §4 (Reasoning 头分态) — 正例: the in-progress ellipsis label renders
  // while the run streams; 反例: the COMPLETED header (「已思考/Thought」) is
  // the committed bubble's state and never leaks into the live stream.
  it('shows the in-progress ellipsis label and never the completed one', () => {
    renderStreaming('Live reasoning')
    expect(screen.getByText('Thinking…')).toBeInTheDocument()
    expect(screen.queryByText('Thought')).not.toBeInTheDocument()
    expect(screen.queryByText(/Thought for/)).not.toBeInTheDocument()
  })
})

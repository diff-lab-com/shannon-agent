import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import StreamingResponse from '@/components/chat/StreamingResponse'
import type { ToolCall } from '@/types'

// B3-2 (§三 P1-6): render-count tests for the incremental streaming split.
// The Markdown module is mocked with a render logger so each <Markdown>
// mount/update is observable: while only the tail grows, the finalized
// prefix must NOT re-render (React.memo bails → no re-parse of the heavy
// pipeline over the prefix), and every streaming render must carry
// `deferHighlight`.

const markdownRenders = vi.hoisted(() =>
  [] as { text: string; deferHighlight: boolean }[],
)

vi.mock('@/components/chat/Markdown', () => ({
  Markdown: ({ children, deferHighlight }: { children: string; deferHighlight?: boolean }) => {
    markdownRenders.push({ text: children, deferHighlight: deferHighlight === true })
    return <div data-testid="markdown">{children}</div>
  },
}))
vi.mock('@/components/chat/MessageBubble', () => ({
  ToolCallDisplay: ({ toolCall }: { toolCall: ToolCall }) => (
    <div data-testid="tool-call" data-tool={toolCall.name} />
  ),
}))

const renderStreaming = (streamingText: string) =>
  render(
    <StreamingResponse
      streamingText={streamingText}
      thinkingText=""
      activeToolCalls={[]}
      onViewDiff={vi.fn()}
    />,
  )

describe('StreamingResponse — incremental split (B3-2)', () => {
  it('renders the active text via a single Markdown while nothing is finalized', () => {
    markdownRenders.length = 0
    renderStreaming('Hello there')
    expect(markdownRenders).toEqual([{ text: 'Hello there', deferHighlight: true }])
    expect(screen.getByTestId('markdown')).toHaveTextContent('Hello there')
  })

  it('finalizes the prefix once a blank-line boundary lands', () => {
    markdownRenders.length = 0
    const view = renderStreaming('Para one.')
    expect(markdownRenders).toHaveLength(1) // tail only
    view.rerender(
      <StreamingResponse
        streamingText={'Para one.\n\nPara two'}
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    // One render for the newly finalized prefix + one for the tail.
    expect(markdownRenders.map(r => r.text)).toEqual(['Para one.', 'Para one.\n\n', 'Para two'])
  })

  it('does NOT re-render the prefix while the tail grows (memo holds)', () => {
    markdownRenders.length = 0
    const view = renderStreaming('Para one.\n\nPara two')
    const prefixRenders = () => markdownRenders.filter(r => r.text === 'Para one.\n\n').length
    expect(prefixRenders()).toBe(1)

    // Several flushes appending to the active tail only.
    for (const text of ['Para one.\n\nPara two', 'Para one.\n\nPara two grows', 'Para one.\n\nPara two grows longer']) {
      view.rerender(
        <StreamingResponse
          streamingText={text}
          thinkingText=""
          activeToolCalls={[]}
          onViewDiff={vi.fn()}
        />,
      )
    }
    // The prefix parsed exactly once; each flush re-rendered only the tail.
    expect(prefixRenders()).toBe(1)
    expect(markdownRenders[markdownRenders.length - 1]).toEqual({
      text: 'Para two grows longer',
      deferHighlight: true,
    })
  })

  it('re-parses the prefix only when a NEW paragraph finalizes', () => {
    markdownRenders.length = 0
    const view = renderStreaming('A.\n\nB.')
    // Initial flush: finalized prefix + active tail, one parse each.
    expect(markdownRenders.map(r => r.text)).toEqual(['A.\n\n', 'B.'])
    view.rerender(
      <StreamingResponse
        streamingText={'A.\n\nB.\n\nC.'}
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    // The prefix re-parsed exactly once (with the grown text); the tail
    // render covered only the new paragraph.
    expect(markdownRenders.map(r => r.text)).toEqual(['A.\n\n', 'B.', 'A.\n\nB.\n\n', 'C.'])
  })

  it('keeps an unclosed fenced block in the tail (no prefix tear)', () => {
    markdownRenders.length = 0
    renderStreaming('Intro.\n\n```python\ndef f():\n    return 1\n')
    const prefix = markdownRenders.find(r => r.text === 'Intro.\n\n')
    const tail = markdownRenders[markdownRenders.length - 1]
    expect(prefix).toBeDefined()
    expect(tail.text).toBe('```python\ndef f():\n    return 1\n')
  })

  it('joins prefix and tail seamlessly (DOM text equals the full stream)', () => {
    const text = 'Para one.\n\nPara two with `code`.'
    const { container } = renderStreaming(text)
    expect(container.textContent).toContain(text)
    // Two sibling Markdown halves, in order.
    const halves = container.querySelectorAll('[data-testid="markdown"]')
    expect(halves).toHaveLength(2)
    expect(halves[0].textContent).toBe('Para one.\n\n')
    expect(halves[1].textContent).toBe('Para two with `code`.')
    // The typing cursor still trails the tail half.
    expect(container.querySelector('.streaming-cursor')?.previousElementSibling).toBe(halves[1])
  })

  it('carries deferHighlight on every streaming Markdown render', () => {
    markdownRenders.length = 0
    const view = renderStreaming('A.\n\nB.')
    view.rerender(
      <StreamingResponse
        streamingText={'A.\n\nB.\n\nC.'}
        thinkingText=""
        activeToolCalls={[]}
        onViewDiff={vi.fn()}
      />,
    )
    expect(markdownRenders.length).toBeGreaterThan(0)
    expect(markdownRenders.every(r => r.deferHighlight)).toBe(true)
  })
})

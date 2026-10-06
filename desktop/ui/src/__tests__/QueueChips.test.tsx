// QueueChips (B1 §4-9 / GB P2-10a) — removable, steerable chips for prompts
// queued while the session was streaming. Zero-coverage until R2: pin the
// render contract (count label, truncated text, attachments-only variant),
// the FIFO steering callbacks (up/down move toward head/tail) and the
// remove callback, plus the disabled edges at both ends of the queue.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import QueueChips from '@/pages/chat/QueueChips'
import { useChat } from '@/context/ChatContext'

vi.mock('@/context/ChatContext', () => ({
  useChat: vi.fn(),
}))

const mockedUseChat = vi.mocked(useChat)

function mockQueue(items: Array<{ id: number; text: string; attachments?: string[] }>) {
  mockedUseChat.mockReturnValue({
    promptQueue: items.map(i => ({ id: i.id, text: i.text, attachments: i.attachments ?? [] })),
    removeQueuedPrompt: vi.fn(),
    moveQueuedPrompt: vi.fn(),
  } as unknown as ReturnType<typeof useChat>)
}

beforeEach(() => {
  vi.clearAllMocks()
})

describe('QueueChips', () => {
  it('renders nothing when the queue is empty', () => {
    mockQueue([])
    const { container } = render(<QueueChips />)
    expect(container.querySelector('[data-testid="prompt-queue"]')).toBeNull()
  })

  it('renders the count label and one chip per queued prompt', () => {
    mockQueue([
      { id: 1, text: 'first queued prompt' },
      { id: 2, text: 'second queued prompt' },
    ])
    render(<QueueChips />)
    expect(screen.getByTestId('prompt-queue')).toHaveAttribute('role', 'list')
    expect(screen.getByText('2 queued')).toBeInTheDocument()
    const chips = screen.getAllByTestId('prompt-queue-chip')
    expect(chips).toHaveLength(2)
    expect(screen.getByText('first queued prompt')).toBeInTheDocument()
    expect(screen.getByText('second queued prompt')).toBeInTheDocument()
  })

  it('renders an attachments-only chip for blank text', () => {
    mockQueue([{ id: 7, text: '   ', attachments: ['/a.md', '/b.md'] }])
    render(<QueueChips />)
    expect(screen.getByText('2 attachments')).toBeInTheDocument()
  })

  it('removes via the chip dismiss callback', () => {
    const removeQueuedPrompt = vi.fn()
    mockedUseChat.mockReturnValue({
      promptQueue: [{ id: 3, text: 'doomed', attachments: [] }],
      removeQueuedPrompt,
      moveQueuedPrompt: vi.fn(),
    } as unknown as ReturnType<typeof useChat>)
    render(<QueueChips />)
    fireEvent.click(screen.getByRole('button', { name: 'Remove queued message' }))
    expect(removeQueuedPrompt).toHaveBeenCalledWith(3)
  })

  it('steers the FIFO: up moves toward the head, down toward the tail', () => {
    const moveQueuedPrompt = vi.fn()
    mockedUseChat.mockReturnValue({
      promptQueue: [
        { id: 1, text: 'head', attachments: [] },
        { id: 2, text: 'middle', attachments: [] },
        { id: 3, text: 'tail', attachments: [] },
      ],
      removeQueuedPrompt: vi.fn(),
      moveQueuedPrompt,
    } as unknown as ReturnType<typeof useChat>)
    render(<QueueChips />)
    const ups = screen.getAllByRole('button', { name: 'Move queued message up (sends sooner)' })
    const downs = screen.getAllByRole('button', { name: 'Move queued message down (sends later)' })
    expect(ups).toHaveLength(3)
    expect(downs).toHaveLength(3)

    // Head chip cannot move up; tail chip cannot move down.
    expect(ups[0]).toBeDisabled()
    expect(downs[2]).toBeDisabled()
    // Interior chips steer both ways with (id, direction).
    expect(ups[1]).toBeEnabled()
    expect(downs[1]).toBeEnabled()
    fireEvent.click(ups[1])
    expect(moveQueuedPrompt).toHaveBeenLastCalledWith(2, -1)
    fireEvent.click(downs[1])
    expect(moveQueuedPrompt).toHaveBeenLastCalledWith(2, 1)
  })
})

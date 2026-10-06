// office Wave 2 B7' — the FileCard "Review changes" entry: rendering rules
// and click behavior, plus the ToolCallDisplay wiring (engine-written file
// → dock the single-file diff via onViewDiff).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { FileCard } from '@/components/chat/FileCard'
import { ToolCallDisplay } from '@/components/chat/MessageBubble'
import * as api from '@/lib/tauri-api'
import type * as TauriApiModule from '@/lib/tauri-api'
import type { ToolCall } from '@/types'

vi.mock('@/lib/tauri-api', async (importOriginal) => ({
  ...(await importOriginal<typeof TauriApiModule>()),
  registerFileIndexEntry: vi.fn().mockResolvedValue(undefined),
}))

const REVIEW_LABEL = 'Review changes'

beforeEach(() => {
  vi.mocked(api.registerFileIndexEntry).mockClear()
})

describe('FileCard diff review (B7\')', () => {
  it('has no review button by default (plain card / user attachment)', () => {
    render(<FileCard name="notes.md" path="/tmp/notes.md" source="attachment" />)
    expect(screen.queryByRole('button', { name: REVIEW_LABEL })).not.toBeInTheDocument()
    // B9' mount hook still indexed the reference with the attachment source.
    expect(api.registerFileIndexEntry).toHaveBeenCalledWith('/tmp/notes.md', 'attachment')
  })

  it('renders the review button when onReviewDiff is provided and clicks through', () => {
    const onReviewDiff = vi.fn()
    render(
      <FileCard name="deck.md" path="/tmp/deck.md" source="generated" onReviewDiff={onReviewDiff} />,
    )
    fireEvent.click(screen.getByRole('button', { name: REVIEW_LABEL }))
    expect(onReviewDiff).toHaveBeenCalledTimes(1)
    expect(api.registerFileIndexEntry).toHaveBeenCalledWith('/tmp/deck.md', 'generated')
  })
})

describe('ToolCallDisplay → FileCard review wiring', () => {
  const writeCall: ToolCall = {
    tool_use_id: 't1',
    tool_name: 'write_file',
    tool_input: { path: '/tmp/report.md', content: 'x' },
    status: 'completed',
    is_error: false,
  }

  it('a completed file write renders the card with Review changes; click requests that path diff', () => {
    const onViewDiff = vi.fn()
    render(<ToolCallDisplay toolCall={writeCall} onViewDiff={onViewDiff} />)
    fireEvent.click(screen.getByRole('button', { name: REVIEW_LABEL }))
    expect(onViewDiff).toHaveBeenCalledWith('/tmp/report.md')
  })

  it('a running / failed write never renders the card (and thus no review entry)', () => {
    const onViewDiff = vi.fn()
    const { unmount } = render(
      <ToolCallDisplay
        toolCall={{ ...writeCall, status: 'running' }}
        onViewDiff={onViewDiff}
      />,
    )
    expect(screen.queryByTestId('file-card')).not.toBeInTheDocument()
    unmount()
    render(
      <ToolCallDisplay
        toolCall={{ ...writeCall, status: 'completed', is_error: true }}
        onViewDiff={onViewDiff}
      />,
    )
    expect(screen.queryByTestId('file-card')).not.toBeInTheDocument()
  })
})

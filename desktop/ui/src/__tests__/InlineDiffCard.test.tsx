// InlineDiffCard — Aurora redesign 2026-10 (02-chat.html 内联 diff 卡).
//
// Pins: (1) the card renders at a COMPLETED file-mutating tool card's
// position with the mono path and the 「打开 Diff」 action that reuses the
// diffPath chain (ToolCallDisplay's onViewDiff); (2) the "+N −N" counts
// render only when the getFileDiff computation actually yields them —
// a failed/unavailable diff leaves the numbers out (honesty contract);
// (3) running / errored writes render neither the diff card nor the
// artifact FileCard.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'

import { ToolCallDisplay } from '@/components/chat/MessageBubble'
import InlineDiffCard from '@/components/chat/InlineDiffCard'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import type * as TauriApiModule from '@/lib/tauri-api'
import type { ToolCall } from '@/types'

vi.mock('@/lib/tauri-api', async (importOriginal) => {
  const actual = await importOriginal<typeof TauriApiModule>()
  return {
    ...actual,
    getFileDiff: vi.fn(async (path: string) => ({
      old_content: `// old ${path}\nconst a = 1\nconst b = 2\n`,
      new_content: `// new ${path}\nconst a = 1\nconst B = 22\nconst c = 3\n`,
      file_name: path.split('/').pop() ?? path,
      language: 'typescript',
    })),
  }
})

function renderCard(ui: React.ReactElement) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

const writeCall: ToolCall = {
  tool_use_id: 't1',
  tool_name: 'write_file',
  tool_input: { path: '/tmp/report.md', content: 'x' },
  status: 'completed',
  is_error: false,
}

beforeEach(() => {
  clearDiffStatsCache()
})

describe('InlineDiffCard — component contract', () => {
  it('renders the mono path and opens the diff through the given callback', async () => {
    const onOpenDiff = vi.fn()
    renderCard(<InlineDiffCard path="/proj/webhooks/verify.rs" onOpenDiff={onOpenDiff} />)
    const card = screen.getByTestId('inline-diff-card')
    expect(card).toHaveTextContent('/proj/webhooks/verify.rs')
    fireEvent.click(screen.getByTestId('inline-diff-open'))
    expect(onOpenDiff).toHaveBeenCalledTimes(1)
    // Same aria contract the old header Diff chip carried.
    expect(screen.getByRole('button', { name: 'View diff for /proj/webhooks/verify.rs' })).toBeInTheDocument()
  })

  it('shows +N −N computed over getFileDiff, and omits the numbers when the diff is unavailable', async () => {
    const { rerender } = renderCard(<InlineDiffCard path="/proj/a.ts" onOpenDiff={() => {}} />)
    // Fixture: old 3 lines / new 4 lines, all differing → +3 −2.
    await waitFor(() => {
      expect(screen.getByTestId('inline-diff-card')).toHaveTextContent('+3')
    })
    expect(screen.getByTestId('inline-diff-card')).toHaveTextContent('−2')

    vi.mocked((await import('@/lib/tauri-api')).getFileDiff).mockRejectedValueOnce(new Error('gone'))
    clearDiffStatsCache()
    rerender(<InlineDiffCard path="/proj/deleted.ts" onOpenDiff={() => {}} />)
    await waitFor(() => {
      // The fetch failed → the card renders WITHOUT counts, never fake ones.
      expect(screen.getByTestId('inline-diff-card').textContent).not.toContain('+')
    })
    expect(screen.getByTestId('inline-diff-open')).toBeInTheDocument()
  })
})

describe('InlineDiffCard — ToolCallDisplay wiring (replaces the header Diff chip)', () => {
  it('a completed file write renders the inline diff card (and the artifact FileCard)', async () => {
    const onViewDiff = vi.fn()
    renderCard(<ToolCallDisplay toolCall={writeCall} onViewDiff={onViewDiff} />)
    await screen.findByTestId('inline-diff-card')
    expect(screen.getByTestId('file-card')).toBeInTheDocument()
    // No leftover header chip duplicate.
    expect(screen.queryByRole('button', { name: /^Diff$/ })).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('inline-diff-open'))
    expect(onViewDiff).toHaveBeenCalledWith('/tmp/report.md')
  })

  it('a running / failed write never renders the card (and thus no diff entry)', () => {
    const onViewDiff = vi.fn()
    const { unmount } = renderCard(
      <ToolCallDisplay toolCall={{ ...writeCall, status: 'running' }} onViewDiff={onViewDiff} />,
    )
    expect(screen.queryByTestId('inline-diff-card')).not.toBeInTheDocument()
    expect(screen.queryByTestId('file-card')).not.toBeInTheDocument()
    unmount()
    renderCard(
      <ToolCallDisplay toolCall={{ ...writeCall, status: 'completed', is_error: true }} onViewDiff={onViewDiff} />,
    )
    expect(screen.queryByTestId('inline-diff-card')).not.toBeInTheDocument()
  })
})

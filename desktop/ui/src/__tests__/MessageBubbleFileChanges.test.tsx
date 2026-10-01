// FileChangesCard (Batch C2, in MessageBubble) — zero coverage until R2.
//
// This suite is also the COMPONENT-LEVEL stand-in for journey #3's
// FileChangesCard leg: in a live (scripted or real) run the committed
// assistant message never carries tool_calls — the cards leave with the run
// (AppContext P2-4) and only return from persisted history on the next
// session load, which a scripted journey cannot express (the seed schema has
// no tool_calls). Here a committed message WITH completed file-mutating
// tool_calls is rendered directly, pinning the +x −y stats the card computes
// over the get_file_diff payload (demo handler returns a fixed diff).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'

import { MessageBubble } from '@/components/chat/MessageBubble'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import { clearDiffStatsCache } from '@/components/chat/diffStats'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'
import type { ChatMessage } from '@/types'

vi.mock('@/lib/tauri-api', async importOriginal => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    getFileDiff: vi.fn(async (path: string) => ({
      old_content: `// old ${path}\nconst a = 1\nconst b = 2\n`,
      new_content: `// new ${path}\nconst a = 1\nconst B = 22\nconst c = 3\n`,
      file_name: path.split('/').pop() ?? path,
      language: 'typescript',
    })),
    getTraceTimeline: vi.fn().mockResolvedValue({ session_id: 's', turns: [], cumulative: [] }),
  }
})

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  sendMessage: vi.fn().mockResolvedValue(true),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  isQuerying: false,
  currentSessionId: 'session-1' as string | null,
  switchSession: vi.fn(),
  refreshSessions: vi.fn().mockResolvedValue(undefined),
  subagentLive: null as any,
  config: null as any,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

function bubbleWith(message: ChatMessage) {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>
          <MessageBubble
            message={message}
            messageIndex={1}
            onViewDiff={() => {}}
            regenerate={null}
          />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

beforeEach(() => {
  vi.clearAllMocks()
  clearDiffStatsCache()
})

describe('FileChangesCard — committed tool_calls rendering', () => {
  it('renders +x −y for a completed file write, computed over get_file_diff', async () => {
    bubbleWith({
      role: 'assistant',
      content: 'Done — updated todo.md.',
      timestamp: 1,
      tool_calls: [{
        tool_use_id: 'tw-1',
        tool_name: 'write_file',
        tool_input: { path: '/proj/todo.md' },
        status: 'completed',
        is_error: false,
        result: 'ok',
      }],
    })
    const card = await screen.findByTestId('file-changes-card')
    expect(card).toBeInTheDocument()
    // old: 3 lines / new: 4 lines with every line differing → +3 −2 over the
    // client-side hunks (the compute runs in diffStats, one IPC per path).
    expect(card).toHaveTextContent('+3')
    expect(card).toHaveTextContent('−2')
    // Path summary + localized count; the review affordance opens the diff.
    expect(card).toHaveTextContent('/proj/todo.md')
    expect(screen.getByRole('button', { name: /View diff for \/proj\/todo\.md/ })).toBeInTheDocument()
  })

  it('aggregates several changed files into one card', async () => {
    bubbleWith({
      role: 'assistant',
      content: 'Two writes landed.',
      timestamp: 1,
      tool_calls: [
        { tool_use_id: 'tw-1', tool_name: 'write_file', tool_input: { path: '/proj/a.ts' }, status: 'completed', is_error: false, result: 'ok' },
        { tool_use_id: 'tw-2', tool_name: 'edit_file', tool_input: { file_path: '/proj/b.ts' }, status: 'completed', is_error: false, result: 'ok' },
        // Duplicate write to the same path — deduplicated into one entry.
        { tool_use_id: 'tw-3', tool_name: 'write_file', tool_input: { path: '/proj/a.ts' }, status: 'completed', is_error: false, result: 'ok' },
      ],
    })
    const card = await screen.findByTestId('file-changes-card')
    expect(card).toHaveTextContent('/proj/a.ts, /proj/b.ts')
  })

  it('renders nothing for failed, running or non-file tools', () => {
    const { rerender } = bubbleWith({
      role: 'assistant',
      content: 'attempt failed',
      timestamp: 1,
      tool_calls: [{ tool_use_id: 'tw-1', tool_name: 'write_file', tool_input: { path: '/proj/x.md' }, status: 'error', is_error: true, result: 'denied' }],
    })
    expect(screen.queryByTestId('file-changes-card')).toBeNull()

    rerender(
      <I18nProvider>
        <MemoryRouter>
          <ArtifactProvider>
            <MessageBubble
              message={{
                role: 'assistant',
                content: 'still running',
                timestamp: 1,
                tool_calls: [{ tool_use_id: 'tw-2', tool_name: 'write_file', tool_input: { path: '/proj/y.md' }, status: 'running', is_error: false }],
              }}
              messageIndex={1}
              onViewDiff={() => {}}
              regenerate={null}
            />
          </ArtifactProvider>
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.queryByTestId('file-changes-card')).toBeNull()

    // bash with a command-only input carries no path field.
    rerender(
      <I18nProvider>
        <MemoryRouter>
          <ArtifactProvider>
            <MessageBubble
              message={{
                role: 'assistant',
                content: 'ran a command',
                timestamp: 1,
                tool_calls: [{ tool_use_id: 'tw-3', tool_name: 'Bash', tool_input: { command: 'ls' }, status: 'completed', is_error: false, result: 'ok' }],
              }}
              messageIndex={1}
              onViewDiff={() => {}}
              regenerate={null}
            />
          </ArtifactProvider>
        </MemoryRouter>
      </I18nProvider>,
    )
    expect(screen.queryByTestId('file-changes-card')).toBeNull()
  })

  it('the review button surfaces the first changed path (multi-file: Review all)', async () => {
    const onViewDiff = vi.fn()
    const onViewDiffMulti = vi.fn()
    render(
      <I18nProvider>
        <MemoryRouter>
          <ArtifactProvider>
            <MessageBubble
              message={{
                role: 'assistant',
                content: 'two files',
                timestamp: 1,
                tool_calls: [
                  { tool_use_id: 'tw-1', tool_name: 'write_file', tool_input: { path: '/proj/a.ts' }, status: 'completed', is_error: false, result: 'ok' },
                  { tool_use_id: 'tw-2', tool_name: 'write_file', tool_input: { path: '/proj/b.ts' }, status: 'completed', is_error: false, result: 'ok' },
                ],
              }}
              messageIndex={1}
              onViewDiff={onViewDiff}
              onViewDiffMulti={onViewDiffMulti}
              regenerate={null}
            />
          </ArtifactProvider>
        </MemoryRouter>
      </I18nProvider>,
    )
    const card = await screen.findByTestId('file-changes-card')
    // Multi-file: the primary button reviews all; single-file reviews the one.
    fireEvent.click(screen.getByText('Review all'))
    expect(onViewDiffMulti).toHaveBeenCalledWith(['/proj/a.ts', '/proj/b.ts'])
    expect(onViewDiff).not.toHaveBeenCalled()
    expect(card).toBeInTheDocument()
  })
})

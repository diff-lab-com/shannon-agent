// W3-4 — memory citation chips on an assistant message.
//
// The chips render from the per-turn injected-memory snapshot the send
// response carries: one chip per injected memory, jumping to the session
// that produced it (same switchSession jump as ContextBreakdownCard).
// An empty / missing list (temporary-chat bypass, zero injections) renders
// nothing at all.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { MessageBubble } from '@/components/chat/MessageBubble'
import { ChatContext, type ChatContextValue } from '@/context/ChatContext'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import type { ChatMessage } from '@/types'

const switchSession = vi.fn().mockResolvedValue(undefined)

const sessionValue = {
  sessions: [],
  sessionActivity: {},
  goalRunsBySession: {},
  subagentLive: null,
  currentSessionId: 's-current',
  windowSessionId: null,
  switchingSession: false,
  switchSession,
} as unknown as SessionContextValue

const chatValue = {
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn().mockResolvedValue(undefined),
  promptQueue: [],
  enqueuePrompt: vi.fn(() => true),
  dequeuePrompt: vi.fn(() => null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  checkpoints: [],
  rewindSession: vi.fn().mockResolvedValue(undefined),
  compactSession: vi.fn(),
  feedback: {},
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  contextPanelOpen: false,
  toggleContextPanel: vi.fn(),
  setContextPanelOpen: vi.fn(),
} as unknown as ChatContextValue

const catalogValue = {
  config: null,
} as unknown as CatalogContextValue

function providers({ children }: { children: React.ReactNode }) {
  return (
    <MemoryRouter initialEntries={['/']}>
      <ChatContext.Provider value={chatValue}>
        <SessionContext.Provider value={sessionValue}>
          <CatalogContext.Provider value={catalogValue}>
            <I18nProvider>{children}</I18nProvider>
          </CatalogContext.Provider>
        </SessionContext.Provider>
      </ChatContext.Provider>
    </MemoryRouter>
  )
}

function assistantMessage(withCitations: ChatMessage['injected_memories']): ChatMessage {
  return {
    role: 'assistant',
    content: 'Answer text',
    timestamp: 1_700_000_000_000,
    injected_memories: withCitations,
  }
}

const sourced = { id: 'm1', title: 'use pnpm not npm', category: 'preference' as const, sourceSessionId: 's-other' }
const unsourced = { id: 'm2', title: 'deploys via k8s', category: 'decision' as const, sourceSessionId: null }

describe('MessageBubble memory citation chips (W3-4)', () => {
  it('renders one chip per injected memory when the turn carried memories', () => {
    render(<MessageBubble message={assistantMessage([sourced, unsourced])} messageIndex={1} onViewDiff={vi.fn()} />, { wrapper: providers })
    const chips = screen.getByTestId('memory-citations')
    expect(chips).toBeInTheDocument()
    expect(screen.getByText('use pnpm not npm')).toBeInTheDocument()
    expect(screen.getByText('deploys via k8s')).toBeInTheDocument()
    // The group reads as "from your memories".
    expect(screen.getByText('From your memories')).toBeInTheDocument()
  })

  it('marks a sourced chip jumpable and jumps to the producing session on click', async () => {
    render(<MessageBubble message={assistantMessage([sourced])} messageIndex={1} onViewDiff={vi.fn()} />, { wrapper: providers })
    const jump = screen.getByTestId('memory-citation-jump-m1')
    fireEvent.click(jump)
    expect(switchSession).toHaveBeenCalledWith('s-other')
  })

  it('renders an unsourced memory as an inert chip without a jump', () => {
    render(<MessageBubble message={assistantMessage([unsourced])} messageIndex={1} onViewDiff={vi.fn()} />, { wrapper: providers })
    expect(screen.getByTestId('memory-citation-m2')).toBeInTheDocument()
    expect(screen.queryByTestId('memory-citation-jump-m2')).not.toBeInTheDocument()
  })

  it('renders no chips for a zero-injection turn', () => {
    render(<MessageBubble message={assistantMessage([])} messageIndex={1} onViewDiff={vi.fn()} />, { wrapper: providers })
    expect(screen.queryByTestId('memory-citations')).not.toBeInTheDocument()
  })

  it('renders no chips when the snapshot is missing (bypass / reloaded history)', () => {
    render(<MessageBubble message={assistantMessage(undefined)} messageIndex={1} onViewDiff={vi.fn()} />, { wrapper: providers })
    expect(screen.queryByTestId('memory-citations')).not.toBeInTheDocument()
  })
})

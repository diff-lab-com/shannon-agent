// Settings R3 T9 — history/committed thinking blocks in MessageBubble,
// gated by the three-tier display pref (lib/thinkingPref): 'all' (default)
// renders every non-empty block collapsed; 'first' only on the assistant
// message the list parent flagged first-of-turn; 'none' hides everywhere.
// A message without thinking never renders a block, whatever the tier.

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { MessageBubble } from '@/components/chat/MessageBubble'
import { ChatContext, type ChatContextValue } from '@/context/ChatContext'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import { SHOW_THINKING_PREF_KEY } from '@/lib/thinkingPref'
import type { ChatMessage } from '@/types'

const sessionValue = {
  sessions: [],
  sessionActivity: {},
  goalRunsBySession: {},
  subagentLive: null,
  currentSessionId: 's-current',
  windowSessionId: null,
  switchingSession: false,
  switchSession: vi.fn().mockResolvedValue(undefined),
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

function assistantMessage(withThinking: ChatMessage['thinking']): ChatMessage {
  return {
    role: 'assistant',
    content: 'Answer text',
    timestamp: 1_700_000_000_000,
    ...(withThinking != null ? { thinking: withThinking } : {}),
  }
}

function renderBubble(message: ChatMessage, isFirstOfTurn = false) {
  return render(
    <MessageBubble
      message={message}
      messageIndex={1}
      isFirstAssistantOfTurn={isFirstOfTurn}
      onViewDiff={vi.fn()}
    />,
    { wrapper: providers },
  )
}

const setPref = (pref: 'all' | 'first' | 'none' | null) => {
  if (pref === null) window.localStorage.removeItem(SHOW_THINKING_PREF_KEY)
  else window.localStorage.setItem(SHOW_THINKING_PREF_KEY, pref)
}

describe('MessageBubble thinking display (Settings R3 T9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('renders the collapsed thinking block under the default "all" tier', () => {
    setPref(null)
    renderBubble(assistantMessage('Let me think about this'))
    expect(screen.getByText('Thinking')).toBeInTheDocument()
    // Collapsed by default — same defaultOpen={false} contract as the live
    // stream's Reasoning block; the body only appears after expanding.
    expect(screen.queryByText('Let me think about this')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: /Thinking/ }))
    expect(screen.getByText('Let me think about this')).toBeInTheDocument()
  })

  it('"first" renders on a first-of-turn assistant message', () => {
    setPref('first')
    renderBubble(assistantMessage('First-turn reasoning'), true)
    expect(screen.getByText('Thinking')).toBeInTheDocument()
  })

  it('"first" skips a non-first assistant message', () => {
    setPref('first')
    renderBubble(assistantMessage('Later reasoning'), false)
    expect(screen.queryByText('Thinking')).not.toBeInTheDocument()
    expect(screen.queryByText('Later reasoning')).not.toBeInTheDocument()
  })

  it('"none" hides the thinking block entirely', () => {
    setPref('none')
    renderBubble(assistantMessage('Hidden reasoning'), true)
    expect(screen.queryByText('Thinking')).not.toBeInTheDocument()
    expect(screen.queryByText('Hidden reasoning')).not.toBeInTheDocument()
  })

  it('renders no thinking block when the message carries none', () => {
    setPref(null)
    renderBubble(assistantMessage(undefined), true)
    expect(screen.queryByText('Thinking')).not.toBeInTheDocument()
  })

  it('never renders thinking on a tool message even with the field set', () => {
    setPref(null)
    const toolMessage: ChatMessage = { ...assistantMessage('tool reasoning'), role: 'tool' }
    renderBubble(toolMessage, true)
    expect(screen.queryByText('Thinking')).not.toBeInTheDocument()
  })
})

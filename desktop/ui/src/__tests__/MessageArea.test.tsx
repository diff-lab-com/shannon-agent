// MessageArea (src/pages/chat/MessageArea.tsx) — the container branches that
// had no coverage until R2: the auth vs plain error-banner routing, the
// run-status pill gate, the session-switch overlay, and the welcome/stream
// gating. (StreamStatusRegion / StreamNoticeLine / RunStatusLine internals
// have their own dedicated suites — StreamStatus / StreamNoticeLine /
// chatRunStatus; this file pins what MessageArea itself decides.)

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import type { Virtualizer } from '@tanstack/react-virtual'

import MessageArea, { VIRTUALIZE_THRESHOLD } from '@/pages/chat/MessageArea'
import { ComposerContext } from '@/pages/chat/ComposerContext'
import { useChat } from '@/context/ChatContext'
import { useSessions } from '@/context/SessionContext'
import { useCatalog } from '@/context/CatalogContext'

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  toolProgress: null as any,
  streamNotices: [] as any[],
  checkpoints: [] as any[],
  sessionActivity: {} as Record<string, any>,
  switchingSession: false,
  currentSessionId: 'session-1' as string | null,
  windowSessionId: null as string | null,
  error: null as string | null,
  errorKind: null as 'auth' | 'other' | null,
  providerStatus: null as any,
  feedback: {} as Record<string, string>,
  sendMessage: vi.fn().mockResolvedValue(true),
  rewindSession: vi.fn(),
}))

vi.mock('@/context/ChatContext', () => ({
  useChat: () => ctx,
}))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({
  useCatalog: () => ctx,
}))

// MessageArea's leaf consumers (ComposerWelcome → WelcomeState) read the
// composer context — provide the shape the page hands down.
const composerValue = {
  input: '',
  setInput: () => {},
  handleSend: () => {},
  handleSteer: () => {},
  attachedFiles: [],
  handleAttach: () => {},
  handleDetachAll: () => {},
  executeSlash: () => {},
  slashResult: null,
  dismissSlashResult: () => {},
  editing: null,
  cancelEdit: () => {},
}

// Below VIRTUALIZE_THRESHOLD the list renders flat — jsdom has no layout.
const virtualizer = {
  getTotalSize: () => 0,
  getVirtualItems: () => [],
  measureElement: undefined,
} as unknown as Virtualizer<HTMLDivElement, Element>

function renderArea() {
  return render(
    <MemoryRouter>
      <ComposerContext.Provider value={composerValue}>
        <MessageArea
          scrollParentRef={{ current: null }}
          messagesEndRef={{ current: null }}
          virtualizer={virtualizer}
          setDiffPath={() => {}}
          setDiffPaths={() => {}}
        />
      </ComposerContext.Provider>
    </MemoryRouter>,
  )
}

beforeEach(() => {
  ctx.messages = []
  ctx.streamingText = ''
  ctx.thinkingText = ''
  ctx.isQuerying = false
  ctx.activeToolCalls = []
  ctx.toolProgress = null
  ctx.streamNotices = []
  ctx.checkpoints = []
  ctx.sessionActivity = {}
  ctx.switchingSession = false
  ctx.currentSessionId = 'session-1'
  ctx.windowSessionId = null
  ctx.error = null
  ctx.errorKind = null
  ctx.providerStatus = {
    active_provider_id: 'prov-anthropic',
    display_name: 'Anthropic',
    kind: 'anthropic',
    has_api_key: true,
    env_provider: null,
  }
  ctx.sendMessage = vi.fn().mockResolvedValue(true)
})

describe('MessageArea — welcome / streaming gating', () => {
  it('shows the composer welcome on an empty session', () => {
    renderArea()
    expect(screen.getByRole('heading', { name: 'What can I help with?' })).toBeInTheDocument()
  })

  it('renders committed messages in DOM order below the virtualize threshold', () => {
    ctx.messages = [
      { role: 'user', content: 'question one', timestamp: 1 },
      { role: 'assistant', content: 'answer one', timestamp: 2 },
    ]
    renderArea()
    expect(screen.getByText('question one')).toBeInTheDocument()
    expect(screen.getByText('answer one')).toBeInTheDocument()
    expect(document.querySelectorAll('[data-message-index]')).toHaveLength(2)
  })

  it('keeps the welcome hidden while a stream is in flight', () => {
    ctx.streamingText = 'partial reply'
    renderArea()
    expect(screen.queryByRole('heading', { name: 'What can I help with?' })).toBeNull()
    expect(document.querySelector('.streaming-cursor')).not.toBeNull()
  })
})

describe('MessageArea — error banner routing (auth vs other)', () => {
  it('routes auth failures to the dedicated key banner (provider name, deep link, retry)', () => {
    ctx.error = 'Authentication failed: invalid x-api-key (HTTP 401)'
    ctx.errorKind = 'auth'
    ctx.messages = [{ role: 'user', content: 'hi', timestamp: 1 }]
    renderArea()

    const banner = screen.getByTestId('auth-error-banner')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('API key rejected by Anthropic')
    expect(banner).toHaveTextContent('Update the key in Settings → Models, then retry.')
    expect(screen.getByRole('button', { name: 'Update key' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
    // The raw engine line stays out of the UI.
    expect(screen.queryByText(ctx.error)).toBeNull()
  })

  it('falls back to the provider id when no display name is known', () => {
    ctx.error = 'Authentication failed'
    ctx.errorKind = 'auth'
    ctx.providerStatus = { active_provider_id: 'prov-glm', display_name: null }
    renderArea()
    expect(screen.getByTestId('auth-error-banner')).toHaveTextContent('API key rejected by prov-glm')
  })

  it('keeps every other failure on the plain banner with the raw line and Retry', () => {
    ctx.error = 'upstream connection reset while streaming'
    ctx.errorKind = 'other'
    ctx.messages = [{ role: 'user', content: 'tell me a story', timestamp: 1 }]
    renderArea()

    expect(screen.queryByTestId('auth-error-banner')).toBeNull()
    expect(screen.getByText('upstream connection reset while streaming')).toBeInTheDocument()
    // Retry resends the LAST USER MESSAGE (B0 P1-3)…
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(ctx.sendMessage).toHaveBeenCalledWith('tell me a story')
  })

  it('hides Retry entirely when there is no user message to resend', () => {
    ctx.error = 'boom'
    ctx.errorKind = 'other'
    renderArea()
    expect(screen.getByText('boom')).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: 'Retry' })).toBeNull()
  })
})

describe('MessageArea — run status pill gate', () => {
  it('pins the pill while the visible session runs, with elapsed and active tool', () => {
    ctx.isQuerying = true
    ctx.sessionActivity = { 'session-1': { running: true, startedAt: Date.now() - 5_000, activeTool: 'bash' } }
    renderArea()
    const pill = screen.getByTestId('run-status-line')
    expect(pill).toHaveTextContent('Worked')
    expect(pill).toHaveTextContent('Running bash')
  })

  it('takes the pill down when the run settles', () => {
    ctx.isQuerying = false
    ctx.sessionActivity = { 'session-1': { running: false, startedAt: Date.now(), activeTool: null } }
    renderArea()
    expect(screen.queryByTestId('run-status-line')).toBeNull()
  })
})

describe('MessageArea — session-switch overlay', () => {
  it('shows the busy veil only while a switch IPC is in flight', () => {
    const view = renderArea()
    expect(screen.queryByTestId('session-switch-overlay')).toBeNull()

    ctx.switchingSession = true
    view.rerender(
      <MemoryRouter>
        <ComposerContext.Provider value={composerValue}>
          <MessageArea
            scrollParentRef={{ current: null }}
            messagesEndRef={{ current: null }}
            virtualizer={virtualizer}
            setDiffPath={() => {}}
            setDiffPaths={() => {}}
          />
        </ComposerContext.Provider>
      </MemoryRouter>,
    )
    const overlay = screen.getByTestId('session-switch-overlay')
    expect(overlay).toBeInTheDocument()
    expect(overlay).toHaveAttribute('aria-busy', 'true')
    expect(overlay).toHaveAttribute('role', 'status')
  })
})

describe('MessageArea — list virtualization decision', () => {
  it('virtualizes strictly above the shared threshold', () => {
    expect(VIRTUALIZE_THRESHOLD).toBe(30)
  })
})

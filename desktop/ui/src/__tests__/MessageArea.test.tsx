// MessageArea (src/pages/chat/MessageArea.tsx) — the container branches that
// had no coverage until R2: the auth vs plain error-banner routing, the
// run-status pill gate, the session-switch overlay, and the welcome/stream
// gating. (StreamStatusRegion / StreamNoticeLine / RunStatusLine internals
// have their own dedicated suites — StreamStatus / StreamNoticeLine /
// chatRunStatus; this file pins what MessageArea itself decides.)

import { describe, it, expect, vi, beforeEach } from 'vitest'
import type { ChatErrorKind } from '@/context/CatalogContext'
import { render, screen, fireEvent, act, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import type { Virtualizer } from '@tanstack/react-virtual'

import MessageArea, { VIRTUALIZE_THRESHOLD } from '@/pages/chat/MessageArea'
import { ComposerContext } from '@/pages/chat/ComposerContext'
import * as api from '@/lib/tauri-api'

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
  errorKind: null as ChatErrorKind | null,
  providerStatus: null as any,
  feedback: {} as Record<string, string>,
  sendMessage: vi.fn().mockResolvedValue(true),
  rewindSession: vi.fn(),
  visionConfirm: null as any,
  resolveVisionConfirm: vi.fn().mockResolvedValue(undefined),
  dismissVisionConfirm: vi.fn().mockResolvedValue(undefined),
  toolsConfirm: null as any,
  resolveToolsConfirm: vi.fn().mockResolvedValue(undefined),
  dismissToolsConfirm: vi.fn().mockResolvedValue(undefined),
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
  ctx.visionConfirm = null
  ctx.resolveVisionConfirm = vi.fn().mockResolvedValue(undefined)
  ctx.dismissVisionConfirm = vi.fn().mockResolvedValue(undefined)
  ctx.toolsConfirm = null
  ctx.resolveToolsConfirm = vi.fn().mockResolvedValue(undefined)
  ctx.dismissToolsConfirm = vi.fn().mockResolvedValue(undefined)
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
    // Retry resends the LAST USER MESSAGE (B0 P1-3); A-3 fix: the second
    // arg carries its attachment paths — `undefined` when it has none.
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }))
    expect(ctx.sendMessage).toHaveBeenCalledWith('tell me a story', undefined)
  })

  it('routes quota (402) failures to the quota banner with update-key / view-usage actions and the model-switch hint', () => {
    ctx.error = 'Provider error (deepseek): insufficient_balance — Insufficient Balance'
    ctx.errorKind = 'quota'
    ctx.messages = [{ role: 'user', content: 'hi', timestamp: 1 }]
    renderArea()

    const banner = screen.getByTestId('quota-error-banner')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('Quota exhausted')
    expect(banner).toHaveTextContent('switch to a cheaper model')
    expect(screen.getByRole('button', { name: 'Update key' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'View usage' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
    // The raw provider line stays out of the UI, like the auth banner.
    expect(screen.queryByText(ctx.error)).toBeNull()
  })

  it('routes rate-limit (429) failures to the rate-limit banner with a wait hint, retry-in countdown, and Retry', () => {
    ctx.error = 'Rate limit exceeded: Rate limit reached, try again in 20s'
    ctx.errorKind = 'rate_limit'
    ctx.messages = [{ role: 'user', content: 'hi', timestamp: 1 }]
    renderArea()

    const banner = screen.getByTestId('rate-limit-error-banner')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('Requests are being rate limited')
    expect(banner).toHaveTextContent('You can retry in 20s')
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
    expect(screen.queryByText(ctx.error)).toBeNull()
  })

  it('omits the retry-in line when the rate-limit error text carries no parseable delay', () => {
    ctx.error = 'Rate limit exceeded'
    ctx.errorKind = 'rate_limit'
    ctx.messages = [{ role: 'user', content: 'hi', timestamp: 1 }]
    renderArea()

    const banner = screen.getByTestId('rate-limit-error-banner')
    expect(banner).toBeInTheDocument()
    expect(banner).not.toHaveTextContent('You can retry in')
    expect(screen.getByRole('button', { name: 'Retry' })).toBeInTheDocument()
  })

  it('routes authz (403) failures to the access-denied banner with a Settings pointer', () => {
    ctx.error = 'Provider error (openai): permission_error — your key cannot access this model'
    ctx.errorKind = 'authz'
    ctx.messages = [{ role: 'user', content: 'hi', timestamp: 1 }]
    renderArea()

    const banner = screen.getByTestId('authz-error-banner')
    expect(banner).toBeInTheDocument()
    expect(banner).toHaveTextContent('Access denied (403)')
    expect(banner).toHaveTextContent('does not have access to the requested model or resource')
    expect(screen.getByRole('button', { name: 'Check key' })).toBeInTheDocument()
    expect(screen.queryByText(ctx.error)).toBeNull()
  })

  it('parseRetryAfterSeconds extracts provider-provided delays, nothing otherwise', async () => {
    const { parseRetryAfterSeconds } = await import('@/pages/chat/MessageArea')
    expect(parseRetryAfterSeconds('Rate limited. Retry-After: 42 seconds')).toBe(42)
    expect(parseRetryAfterSeconds('please try again in 7s')).toBe(7)
    expect(parseRetryAfterSeconds('rate limit exceeded')).toBeNull()
    expect(parseRetryAfterSeconds('')).toBeNull()
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

// A-8 — the duration lookup used to fetch the L0 trace timeline on session
// switches only, so a turn that completed while the user stayed in the same
// session never got its historical durations. The lookup must also refresh
// when the run settles (isQuerying true→false); a run START reads no new
// history and must not refetch.
describe('MessageArea — tool duration lookup refresh on run settle (A-8)', () => {
  const timeline = {
    session_id: 'session-1',
    started_ts_ns: 0,
    ended_ts_ns: 2_000_000,
    turns: [
      {
        turn: 1,
        start_ts_ns: 0,
        end_ts_ns: 1,
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_tokens: 0,
        cache_read_tokens: 0,
        tools: [
          { tool_use_id: 'tc-9', tool_name: 'bash', start_ts_ns: 0, end_ts_ns: 1_234_000, duration_ms: 1234, is_error: false },
        ],
      },
    ],
    cumulative: [],
  }

  function rerenderArea(view: ReturnType<typeof renderArea>) {
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
  }

  beforeEach(() => {
    vi.mocked(api.getTraceTimeline).mockReset()
    vi.mocked(api.getTraceTimeline).mockResolvedValue(timeline)
  })

  it('refetches the trace timeline when the run settles in the same session', async () => {
    ctx.isQuerying = true
    const view = renderArea()
    await waitFor(() => expect(api.getTraceTimeline).toHaveBeenCalledTimes(1))
    expect(api.getTraceTimeline).toHaveBeenCalledWith('session-1')

    // The run settles — the just-finished turn's durations are now in the
    // backend timeline; the lookup must re-read it without a session switch.
    ctx.isQuerying = false
    rerenderArea(view)
    await waitFor(() => expect(api.getTraceTimeline).toHaveBeenCalledTimes(2))
    expect(api.getTraceTimeline).toHaveBeenLastCalledWith('session-1')
  })

  it('a run start does not refetch (no new history to read yet)', async () => {
    ctx.isQuerying = false
    const view = renderArea()
    await waitFor(() => expect(api.getTraceTimeline).toHaveBeenCalledTimes(1))

    ctx.isQuerying = true
    rerenderArea(view)
    await act(async () => {}) // drain microtasks — no fetch may follow
    expect(api.getTraceTimeline).toHaveBeenCalledTimes(1)
  })
})

describe('MessageArea — S2-4a vision confirm bar', () => {
  it('renders nothing when no send is held', () => {
    renderArea()
    expect(screen.queryByTestId('vision-confirm-bar')).not.toBeInTheDocument()
  })

  it('offers the one-click switch when a candidate exists', async () => {
    ctx.visionConfirm = {
      model: 'deepseek-v4-flash',
      suggestion: { provider: 'deepseek', model: 'deepseek-v4-vision', name: 'DeepSeek V4 Vision' },
    }
    renderArea()
    const bar = screen.getByTestId('vision-confirm-bar')
    expect(bar).toHaveTextContent('deepseek-v4-flash')
    expect(bar).toHaveTextContent('DeepSeek V4 Vision')
    fireEvent.click(screen.getByTestId('vision-confirm-switch'))
    await act(async () => {}) // settle the resolver promise
    expect(ctx.resolveVisionConfirm).toHaveBeenCalledWith('switch')
  })

  it('degrades to notice-only (no switch button) without a candidate', () => {
    ctx.visionConfirm = { model: 'deepseek-v4-flash', suggestion: null }
    renderArea()
    expect(screen.getByTestId('vision-confirm-bar')).toHaveTextContent('model menu')
    expect(screen.queryByTestId('vision-confirm-switch')).not.toBeInTheDocument()
    expect(screen.getByTestId('vision-confirm-send-anyway')).toBeInTheDocument()
  })

  it("'send anyway' and 'cancel' resolve their choices and close the bar path", async () => {
    ctx.visionConfirm = { model: 'deepseek-v4-flash', suggestion: null }
    renderArea()
    fireEvent.click(screen.getByTestId('vision-confirm-send-anyway'))
    await act(async () => {}) // settle the resolver promise
    expect(ctx.resolveVisionConfirm).toHaveBeenCalledWith('send-anyway')
    fireEvent.click(screen.getByTestId('vision-confirm-cancel'))
    await act(async () => {})
    expect(ctx.dismissVisionConfirm).toHaveBeenCalledTimes(1)
  })
})

describe('MessageArea — S2-4b tools confirm bar', () => {
  it('renders nothing when no send is held', () => {
    renderArea()
    expect(screen.queryByTestId('tool-confirm-bar')).not.toBeInTheDocument()
  })

  it('offers the one-click switch when a candidate exists (distinct tool-confirm testids)', async () => {
    ctx.toolsConfirm = {
      model: 'llama-4-70b',
      suggestion: { provider: 'ollama', model: 'qwen3-coder-480b', name: 'Qwen3 Coder 480B' },
    }
    renderArea()
    const bar = screen.getByTestId('tool-confirm-bar')
    expect(bar).toHaveTextContent('llama-4-70b')
    expect(bar).toHaveTextContent('Qwen3 Coder 480B')
    // Distinct from the vision bar: no vision testids leak into the tools
    // bar and vice versa.
    expect(screen.queryByTestId('vision-confirm-bar')).not.toBeInTheDocument()
    fireEvent.click(screen.getByTestId('tool-confirm-switch'))
    await act(async () => {}) // settle the resolver promise
    expect(ctx.resolveToolsConfirm).toHaveBeenCalledWith('switch')
    expect(ctx.resolveVisionConfirm).not.toHaveBeenCalled()
  })

  it('degrades to notice-only (no switch button) without a candidate', () => {
    ctx.toolsConfirm = { model: 'llama-4-70b', suggestion: null }
    renderArea()
    expect(screen.getByTestId('tool-confirm-bar')).toHaveTextContent('model menu')
    expect(screen.queryByTestId('tool-confirm-switch')).not.toBeInTheDocument()
    expect(screen.getByTestId('tool-confirm-send-anyway')).toBeInTheDocument()
  })

  it("'send anyway' and 'cancel' resolve their choices on the tools slot", async () => {
    ctx.toolsConfirm = { model: 'llama-4-70b', suggestion: null }
    renderArea()
    fireEvent.click(screen.getByTestId('tool-confirm-send-anyway'))
    await act(async () => {}) // settle the resolver promise
    expect(ctx.resolveToolsConfirm).toHaveBeenCalledWith('send-anyway')
    fireEvent.click(screen.getByTestId('tool-confirm-cancel'))
    await act(async () => {})
    expect(ctx.dismissToolsConfirm).toHaveBeenCalledTimes(1)
  })

  it('renders the vision bar and the tools bar through the same shared body when both states are somehow set', () => {
    // Defensive pin: the two bars are mutually exclusive by flow (the
    // resolvers re-enter sendMessage), but the shared CapabilityConfirmBar
    // body must render each independently if both states were ever set —
    // and each keeps its own testid namespace.
    ctx.visionConfirm = { model: 'deepseek-v4-flash', suggestion: null }
    ctx.toolsConfirm = { model: 'llama-4-70b', suggestion: null }
    renderArea()
    expect(screen.getByTestId('vision-confirm-bar')).toBeInTheDocument()
    expect(screen.getByTestId('tool-confirm-bar')).toBeInTheDocument()
  })
})

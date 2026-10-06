// B1-6 P1-8 — rejected-send recovery must not clobber a draft typed while the
// send IPC was in flight.
//
// Both recovery paths live in Chat.tsx: the manual send's `.then(ok => ...)`
// and the queue drain's. The old behavior restored the refused payload with
// `setInput(...)` unconditionally — settle-moment typing got wiped. The fix:
// an empty composer gets the old restore verbatim; a composer the user has
// typed into keeps their draft, the payload is still parked as the banner's
// continue target (blockedPayload), and a toast says where it went.
//
// Harness mirrors chatInputPersistence.test.tsx (mocked contexts + rerender),
// with a capturing @tauri-apps/api/event fake so the budget-exceeded banner
// can be driven to observe the blockedPayload handoff.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, act } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import Chat from '@/pages/Chat'
import { EVENT_NAMES } from '@/types'

const { toastMock } = vi.hoisted(() => ({
  toastMock: { error: vi.fn(), warning: vi.fn(), info: vi.fn(), success: vi.fn(), message: vi.fn() },
}))
vi.mock('sonner', () => ({ toast: toastMock }))

const ctx = vi.hoisted(() => ({
  messages: [] as any[],
  streamingText: '',
  thinkingText: '',
  isQuerying: false,
  activeToolCalls: [] as any[],
  streamNotices: [] as any[],
  usage: null as any,
  sessions: [] as any[],
  currentSessionId: null as string | null,
  windowSessionId: null as string | null,
  error: null as string | null,
  errorKind: null as 'auth' | 'other' | null,
  providerStatus: null as any,
  config: null as any,
  status: null as any,
  sendMessage: vi.fn().mockResolvedValue(true),
  cancelQuery: vi.fn(),
  checkpoints: [] as unknown[],
  rewindSession: vi.fn(),
  feedback: {} as Record<string, string>,
  recordFeedback: vi.fn().mockResolvedValue(undefined),
  createSession: vi.fn(),
  switchSession: vi.fn(),
  deleteSession: vi.fn(),
  renameSession: vi.fn(),
  promptQueue: [] as any[],
  enqueuePrompt: vi.fn().mockReturnValue(true),
  dequeuePrompt: vi.fn().mockReturnValue(null),
  removeQueuedPrompt: vi.fn(),
  moveQueuedPrompt: vi.fn(),
  toolProgress: null as any,
  runProcess: null as any,
  sessionActivity: {} as Record<string, any>,
}))

vi.mock('@/context/ChatContext', () => ({ useChat: () => ctx }))
vi.mock('@/context/SessionContext', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return { ...actual, useSessions: () => ctx }
})
vi.mock('@/context/CatalogContext', () => ({ useCatalog: () => ctx }))

// Capturing listen so the test can raise budget:exceeded and observe the
// banner's continue action (the blockedPayload consumer).
const { captured, flush } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  const flush = (event: string, payload: unknown) => {
    for (const h of captured[event] ?? []) h(payload)
  }
  return { captured, flush }
})
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: (e: { payload: unknown }) => void) => {
    captured[event] ??= []
    captured[event].push((payload) => handler({ payload }))
    return Promise.resolve(() => {
      captured[event] = (captured[event] ?? []).filter(h => h !== handler)
    })
  }),
  emit: vi.fn(),
}))

const SESSION_A = 'blocked-recovery-sess-a'

function deferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>(r => { resolve = r })
  return { promise, resolve }
}

function ChatTree() {
  return (
    <I18nProvider>
      <MemoryRouter>
        <ArtifactProvider>
          <Chat />
        </ArtifactProvider>
      </MemoryRouter>
    </I18nProvider>
  )
}

function composer() {
  return screen.getByRole('textbox', { name: 'Message' }) as HTMLTextAreaElement
}

function renderOn(sessionId: string) {
  ctx.currentSessionId = sessionId
  return render(<ChatTree />)
}

/** Fire the budget:exceeded event so BudgetBanner renders its continue
 *  action — the observable the blockedPayload hold is asserted through. */
async function showBudgetBanner() {
  await act(async () => {}) // let the async listen() registrations land
  await act(async () => {
    flush(EVENT_NAMES.BUDGET_EXCEEDED, { sessionId: SESSION_A, spentUsd: 2, budgetUsd: 1 })
  })
}

beforeEach(() => {
  vi.clearAllMocks()
  ctx.sendMessage.mockReset()
  ctx.sendMessage.mockResolvedValue(true)
  ctx.dequeuePrompt.mockReset()
  ctx.dequeuePrompt.mockReturnValue(null)
  ctx.messages = []
  ctx.streamingText = ''
  ctx.isQuerying = false
  ctx.promptQueue = []
  ctx.windowSessionId = null
  ctx.error = null
  ctx.providerStatus = null
  ctx.config = null
  ctx.sessions = []
  ctx.toolProgress = null
  ctx.runProcess = { status: 'idle', startedAt: null, endedAt: null, sources: [], outputs: [], summary: null, lastTool: null, toolCount: 0 }
  ctx.sessionActivity = {}
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
})

describe('P1-8 — rejected-send recovery vs the composer draft (manual send)', () => {
  it('keeps a draft typed mid-flight, still parks the payload for the banner', async () => {
    renderOn(SESSION_A)
    await showBudgetBanner()

    // The send is held in flight while the user keeps typing.
    const gate = deferred<boolean>()
    ctx.sendMessage.mockImplementationOnce(() => gate.promise)
    fireEvent.change(composer(), { target: { value: 'blocked msg' } })
    fireEvent.keyDown(composer(), { key: 'Enter' })
    expect(ctx.sendMessage).toHaveBeenCalledWith('blocked msg', undefined)
    expect(composer().value).toBe('') // cleared synchronously at send time

    fireEvent.change(composer(), { target: { value: 'typed during flight' } })

    await act(async () => { gate.resolve(false) })
    // P1-8: the draft wins; a toast points at the banner instead.
    expect(composer().value).toBe('typed during flight')
    expect(toastMock.info).toHaveBeenCalledTimes(1)

    // The blocked payload is still parked — the banner's continue resends
    // exactly the BLOCKED text (not the draft), bypassing the budget once.
    fireEvent.click(screen.getByRole('button', { name: 'Continue — send the blocked message (ignore once)' }))
    await act(async () => {})
    expect(ctx.sendMessage).toHaveBeenLastCalledWith('blocked msg', undefined, { budgetBypass: true })
  })

  it('restores the refused payload verbatim when the composer is empty at settle (old behavior)', async () => {
    renderOn(SESSION_A)

    const gate = deferred<boolean>()
    ctx.sendMessage.mockImplementationOnce(() => gate.promise)
    fireEvent.change(composer(), { target: { value: 'blocked msg' } })
    fireEvent.keyDown(composer(), { key: 'Enter' })

    // Nothing typed since — the recovery brings the text back as before.
    await act(async () => { gate.resolve(false) })
    expect(composer().value).toBe('blocked msg')
    expect(toastMock.info).not.toHaveBeenCalled()
  })
})

describe('P1-8 — rejected-send recovery vs the composer draft (queue drain)', () => {
  function armDrain() {
    ctx.promptQueue = [{ id: 1, text: 'queued item', attachments: [] }]
    ctx.dequeuePrompt.mockReturnValue(ctx.promptQueue[0])
    ctx.isQuerying = true
    const view = renderOn(SESSION_A)
    const gate = deferred<boolean>()
    ctx.sendMessage.mockImplementationOnce(() => gate.promise)
    // The run settles → the drain effect fires and sends the head.
    return { view, gate, settle: async () => {
      await act(async () => {
        ctx.isQuerying = false
        view.rerender(<ChatTree />)
      })
    } }
  }

  it('keeps a draft typed during the drain send, parks the item for the banner', async () => {
    const { gate, settle } = armDrain()
    await settle()
    expect(ctx.sendMessage).toHaveBeenCalledWith('queued item', undefined)

    fireEvent.change(composer(), { target: { value: 'fresh draft' } })
    await act(async () => { gate.resolve(false) })

    // P1-8: the drain's restore must not clobber the fresh draft.
    expect(composer().value).toBe('fresh draft')
    expect(toastMock.info).toHaveBeenCalledTimes(1)
    // The refused head is still the banner's continue target.
    await showBudgetBanner()
    fireEvent.click(screen.getByRole('button', { name: 'Continue — send the blocked message (ignore once)' }))
    await act(async () => {})
    expect(ctx.sendMessage).toHaveBeenLastCalledWith('queued item', undefined, { budgetBypass: true })
  })

  it('restores the refused head verbatim when the composer is empty at settle (old behavior)', async () => {
    const { gate, settle } = armDrain()
    await settle()
    expect(ctx.sendMessage).toHaveBeenCalledWith('queued item', undefined)

    await act(async () => { gate.resolve(false) })
    expect(composer().value).toBe('queued item')
    expect(toastMock.info).not.toHaveBeenCalled()
  })
})

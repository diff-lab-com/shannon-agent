// Office Wave 3 C3 — companionBridge + the App.tsx bridge host.
//
// The bridge is the only cross-window channel: emitCompanionPrompt must
// target `main` with the frozen event name, the listener must deliver text
// payloads (and only text payloads), and the App-level host must translate
// prompts into Wave 2 composer drafts — navigating to /chat first when the
// main window is elsewhere.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, renderHook, act, screen, waitFor, cleanup } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import { emitTo, listen } from '@tauri-apps/api/event'

// The App host reuses the Wave 2 draft bridge; spy on the push so tests can
// observe it without mounting the real chat composer.
const pushComposerDraft = vi.hoisted(() => vi.fn())
vi.mock('@/lib/composerBridge', async importOriginal => {
  const actual = await importOriginal<typeof composerBridgeModule>()
  return {
    ...actual,
    pushComposerDraft: (...args: unknown[]) => pushComposerDraft(...(args as [string])),
  }
})

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(),
  emitTo: vi.fn().mockResolvedValue(undefined),
}))

import {
  COMPANION_PROMPT_EVENT,
  COMPANION_TARGET_WINDOW,
  emitCompanionPrompt,
  isMainWindowLocation,
  useCompanionPromptListener,
} from '@/lib/companionBridge'
import { CompanionPromptBridge } from '@/App'
import type * as composerBridgeModule from '@/lib/composerBridge'

type ListenHandler = (event: { payload: unknown }) => void

/** Install a controllable `listen` mock; returns the captured handler. */
function mockListen() {
  const handlers: ListenHandler[] = []
  const unlisten = vi.fn()
  vi.mocked(listen).mockImplementation(async (_event: string, handler: ListenHandler) => {
    handlers.push(handler)
    return unlisten
  })
  return {
    handlers,
    unlisten,
    fire(payload: unknown) {
      act(() => {
        for (const handler of handlers) handler({ payload })
      })
    },
  }
}

const SESSION_UUID = '00000000-0000-0000-0000-000000000000'

/** jsdom URL surgery — window.location is what isMainWindowLocation reads. */
function pushUrl(url: string) {
  window.history.pushState({}, '', url)
}

beforeEach(() => {
  vi.mocked(emitTo).mockClear()
  pushComposerDraft.mockClear()
  pushUrl('/')
})

afterEach(() => {
  cleanup()
  pushUrl('/')
})

describe('emitCompanionPrompt', () => {
  it('targets the main window with the frozen event name and { text } payload', async () => {
    await emitCompanionPrompt('quick thought')
    expect(emitTo).toHaveBeenCalledWith(COMPANION_TARGET_WINDOW, COMPANION_PROMPT_EVENT, {
      text: 'quick thought',
    })
    expect(COMPANION_TARGET_WINDOW).toBe('main')
    expect(COMPANION_PROMPT_EVENT).toBe('shannon:companion-prompt')
  })

  it('propagates emit failures (the page turns them into an error line)', async () => {
    vi.mocked(emitTo).mockRejectedValueOnce(new Error('companion window gone'))
    await expect(emitCompanionPrompt('x')).rejects.toThrow('companion window gone')
  })
})

describe('isMainWindowLocation', () => {
  it('main window: no windowSession param, not the /companion route', () => {
    pushUrl('/chat')
    expect(isMainWindowLocation()).toBe(true)
    pushUrl('/')
    expect(isMainWindowLocation()).toBe(true)
  })

  it('session windows (?windowSession=<uuid>) are not the main window', () => {
    pushUrl(`/?windowSession=${SESSION_UUID}`)
    expect(isMainWindowLocation()).toBe(false)
  })

  it('the companion window (/companion) is not the main window', () => {
    pushUrl('/companion')
    expect(isMainWindowLocation()).toBe(false)
  })

  it('accepts explicit path/search arguments (pure form)', () => {
    expect(isMainWindowLocation('/chat', '')).toBe(true)
    expect(isMainWindowLocation('/companion', '')).toBe(false)
    expect(isMainWindowLocation('/', `?windowSession=${SESSION_UUID}`)).toBe(false)
  })
})

describe('useCompanionPromptListener', () => {
  it('delivers string payloads to the latest callback', async () => {
    const events = mockListen()
    const onPrompt = vi.fn()
    const { rerender } = renderHook(({ cb }) => useCompanionPromptListener(cb), {
      initialProps: { cb: onPrompt },
    })
    await waitFor(() => expect(events.handlers.length).toBe(1))

    events.fire({ text: 'first' })
    expect(onPrompt).toHaveBeenCalledWith('first')

    // Latest-ref contract: an inline closure over fresh state still wins
    // without re-subscribing (still exactly one Tauri listener).
    const second = vi.fn()
    rerender({ cb: second })
    events.fire({ text: 'second' })
    expect(second).toHaveBeenCalledWith('second')
    expect(onPrompt).not.toHaveBeenCalledWith('second')
    expect(events.handlers.length).toBe(1)
  })

  it('ignores payloads whose text is not a string', async () => {
    const events = mockListen()
    const onPrompt = vi.fn()
    renderHook(() => useCompanionPromptListener(onPrompt))
    await waitFor(() => expect(events.handlers.length).toBe(1))

    events.fire({})
    events.fire(undefined)
    events.fire({ text: 42 })
    expect(onPrompt).not.toHaveBeenCalled()
  })

  it('unlistens on unmount, including when listen resolves late', async () => {
    const events = mockListen()
    const onPrompt = vi.fn()
    const { unmount } = renderHook(() => useCompanionPromptListener(onPrompt))
    await waitFor(() => expect(events.handlers.length).toBe(1))
    unmount()
    expect(events.unlisten).toHaveBeenCalled()

    // Late-resolving listen after unmount must dispose immediately.
    const { unmount: unmount2 } = renderHook(() => useCompanionPromptListener(onPrompt))
    unmount2()
    await waitFor(() => expect(events.unlisten).toHaveBeenCalledTimes(2))
  })
})

describe('CompanionPromptBridge (App.tsx host)', () => {
  function renderHostAt(path: string) {
    return render(
      <MemoryRouter initialEntries={[path]}>
        <CompanionPromptBridge />
        <Routes>
          <Route path="/chat" element={<div data-testid="chat-page" />} />
          <Route path="/tasks" element={<div data-testid="tasks-page" />} />
        </Routes>
      </MemoryRouter>,
    )
  }

  it('on /chat: pushes the prompt straight into the composer draft bridge', async () => {
    const events = mockListen()
    renderHostAt('/chat')
    await waitFor(() => expect(events.handlers.length).toBe(1))

    events.fire({ text: 'captured' })
    expect(pushComposerDraft).toHaveBeenCalledWith('captured')
    expect(screen.getByTestId('chat-page')).toBeInTheDocument()
  })

  it('outside /chat: navigates to /chat first, then pushes the draft', async () => {
    const events = mockListen()
    renderHostAt('/tasks')
    await waitFor(() => expect(events.handlers.length).toBe(1))

    events.fire({ text: 'captured later' })
    expect(screen.getByTestId('chat-page')).toBeInTheDocument()
    await waitFor(
      () => expect(pushComposerDraft).toHaveBeenCalledWith('captured later'),
      { timeout: 1000 },
    )
  }, 5000)

  it('never acts when the webview is not the main window', async () => {
    // Companion window boot URL.
    pushUrl('/companion')
    const events = mockListen()
    renderHostAt('/chat')
    await waitFor(() => expect(events.handlers.length).toBe(1))

    events.fire({ text: 'echo' })
    expect(pushComposerDraft).not.toHaveBeenCalled()

    // Session window boot URL.
    cleanup()
    pushUrl(`/?windowSession=${SESSION_UUID}`)
    renderHostAt('/chat')
    events.fire({ text: 'echo' })
    expect(pushComposerDraft).not.toHaveBeenCalled()
  })
})

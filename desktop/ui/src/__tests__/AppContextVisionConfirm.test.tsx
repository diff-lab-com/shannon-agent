// S2-4a (review 2026-10-05 P-N9) — the desktop pre-send vision gate.
//
// Pins the interception contract of AppContext.sendMessage for messages
// that carry image attachments:
//   * effective model KNOWN to lack vision → the send HOLDS (no IPC, no
//     optimistic bubble), the confirm state surfaces, and each of the
//     three resolutions does exactly what its button says;
//   * unknown capability / failed pre-check / no attachments → the send
//     flows exactly as before (fail open; the engine gate is the backstop).
//
// Harness mirrors AppContextB16SendPath.test.tsx: a real AppProvider whose
// only replacement is a capturing @tauri-apps/api/event fake.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'

const { captured } = vi.hoisted(() => {
  const captured: Record<string, ((e: { payload: unknown }) => void)[]> = {}
  return { captured }
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

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.switchSession).mockResolvedValue([])
  // setup.ts default: effective model HAS vision — restore it explicitly
  // because vi.clearAllMocks() above wipes the resolved value.
  vi.mocked(api.checkVisionSend).mockResolvedValue({
    model: 'claude-sonnet-4-6',
    provider: 'anthropic',
    vision: true,
  })
  vi.mocked(api.sendMessage).mockResolvedValue({ message_id: '1', status: 'sent' })
})

describe('S2-4a — desktop pre-send vision check', () => {
  it('HOLDS an image-carrying send when the model is known to lack vision', async () => {
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'deepseek-v4-flash',
      provider: 'deepseek',
      vision: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await waitFor(() => {
      expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
    })

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('look at this', ['/tmp/pic.png']) })
    expect(await send).toBe(false)
    // No silent send: zero engine traffic, no optimistic bubble.
    expect(api.sendMessage).not.toHaveBeenCalled()
    expect(result.current.visionConfirm).toEqual({
      model: 'deepseek-v4-flash',
      suggestion: null,
    })
  })

  it("'send anyway' delivers the held payload and bypasses the re-check", async () => {
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'deepseek-v4-flash',
      provider: 'deepseek',
      vision: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => {
      await result.current.sendMessage('look', ['/tmp/pic.png'])
    })
    expect(result.current.visionConfirm).not.toBeNull()

    await act(async () => { await result.current.resolveVisionConfirm('send-anyway') })
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(api.sendMessage).toHaveBeenCalledWith('look', ['/tmp/pic.png'], undefined, SESSION_A)
    // The bar closed.
    expect(result.current.visionConfirm).toBeNull()
  })

  it("'switch' pins the suggested model on the session, then delivers", async () => {
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'deepseek-v4-flash',
      provider: 'deepseek',
      vision: false,
      suggestion: { provider: 'deepseek', model: 'deepseek-v4-vision', name: 'DeepSeek V4 Vision' },
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => {
      await result.current.sendMessage('look', ['/tmp/pic.png'])
    })
    await act(async () => { await result.current.resolveVisionConfirm('switch') })
    expect(api.setSessionModel).toHaveBeenCalledWith(SESSION_A, 'deepseek', 'deepseek-v4-vision')
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.visionConfirm).toBeNull()
  })

  it('dismiss drops the held payload without sending', async () => {
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'deepseek-v4-flash',
      provider: 'deepseek',
      vision: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => {
      await result.current.sendMessage('look', ['/tmp/pic.png'])
    })
    await act(async () => { await result.current.resolveVisionConfirm('dismiss') })
    expect(api.setSessionModel).not.toHaveBeenCalled()
    expect(api.sendMessage).not.toHaveBeenCalled()
    expect(result.current.visionConfirm).toBeNull()
  })

  it('fail-open: a failed pre-check never blocks the send', async () => {
    vi.mocked(api.checkVisionSend).mockRejectedValue(new Error('command unavailable'))
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('look', ['/tmp/pic.png']) })
    expect(await send).toBe(true)
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.visionConfirm).toBeNull()
  })

  it('capability-unknown models are NOT held (three-state rule)', async () => {
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'some-unknown-model',
      provider: 'deepseek',
      vision: null,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('look', ['/tmp/pic.png']) })
    expect(await send).toBe(true)
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
  })

  it('text-only sends skip the pre-check entirely', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('just words') })
    expect(await send).toBe(true)
    expect(api.checkVisionSend).not.toHaveBeenCalled()
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
  })
})

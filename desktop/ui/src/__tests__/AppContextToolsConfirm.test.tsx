// S2-4b (review 2026-10-05 P-N9) — the desktop pre-send tools gate.
//
// Pins the interception contract of AppContext.sendMessage for EVERY send
// (tools ride every desktop request — the gate is not attachment-gated):
//   * effective model KNOWN to lack tool calling → the send HOLDS (no IPC
//     send, no optimistic bubble), the confirm state surfaces, and each of
//     the three resolutions does exactly what its button says;
//   * "not applicable" (applies=false, tools-off session) and unknown
//     capability (tools=null) never hold; a failed pre-check fails open;
//   * the gates confirm SEQUENTIALLY with vision: a vision "send anyway"
//     resolution re-enters sendMessage, where the tools gate runs next.
//
// Harness mirrors AppContextVisionConfirm.test.tsx: a real AppProvider
// whose only replacement is a capturing @tauri-apps/api/event fake.

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
  // setup.ts defaults: the effective model HAS vision AND tools — restore
  // explicitly because vi.clearAllMocks() above wipes the resolved values.
  vi.mocked(api.checkVisionSend).mockResolvedValue({
    model: 'claude-sonnet-4-6',
    provider: 'anthropic',
    vision: true,
  })
  vi.mocked(api.checkToolsSend).mockResolvedValue({
    model: 'claude-sonnet-4-6',
    provider: 'anthropic',
    applies: true,
    tools: true,
  })
  vi.mocked(api.sendMessage).mockResolvedValue({ message_id: '1', status: 'sent' })
})

describe('S2-4b — desktop pre-send tools check', () => {
  it('HOLDS a send when the model is known to lack tool calling', async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await waitFor(() => {
      expect((captured[EVENT_NAMES.QUERY_TEXT] ?? []).length).toBeGreaterThan(0)
    })

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('run the tests') })
    expect(await send).toBe(false)
    // No silent send: zero engine traffic, no optimistic bubble.
    expect(api.sendMessage).not.toHaveBeenCalled()
    expect(result.current.toolsConfirm).toEqual({
      model: 'llama-4-70b',
      suggestion: null,
    })
  })

  it('runs on every send — no attachments required (unlike the vision gate)', async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => { await result.current.sendMessage('plain text, no files') })
    expect(api.checkToolsSend).toHaveBeenCalledTimes(1)
    expect(result.current.toolsConfirm).not.toBeNull()
    expect(api.sendMessage).not.toHaveBeenCalled()
  })

  it("'send anyway' delivers the held payload and bypasses the re-check", async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => { await result.current.sendMessage('run the tests') })
    expect(result.current.toolsConfirm).not.toBeNull()

    await act(async () => { await result.current.resolveToolsConfirm('send-anyway') })
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(api.sendMessage).toHaveBeenCalledWith('run the tests', undefined, undefined, SESSION_A)
    // The bar closed.
    expect(result.current.toolsConfirm).toBeNull()
  })

  it("'switch' pins the suggested model on the session, then delivers", async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: { provider: 'ollama', model: 'qwen3-coder-480b', name: 'Qwen3 Coder 480B' },
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => { await result.current.sendMessage('run the tests') })
    await act(async () => { await result.current.resolveToolsConfirm('switch') })
    expect(api.setSessionModel).toHaveBeenCalledWith(SESSION_A, 'ollama', 'qwen3-coder-480b')
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.toolsConfirm).toBeNull()
  })

  it('dismiss drops the held payload without sending', async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => { await result.current.sendMessage('run the tests') })
    await act(async () => { await result.current.resolveToolsConfirm('dismiss') })
    expect(api.setSessionModel).not.toHaveBeenCalled()
    expect(api.sendMessage).not.toHaveBeenCalled()
    expect(result.current.toolsConfirm).toBeNull()
  })

  it("a tools-off session ('not applicable') is NEVER held", async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: false,
      tools: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('run the tests') })
    expect(await send).toBe(true)
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.toolsConfirm).toBeNull()
  })

  it('capability-unknown models are NOT held (three-state rule)', async () => {
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'some-unknown-model',
      provider: 'ollama',
      applies: true,
      tools: null,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('run the tests') })
    expect(await send).toBe(true)
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
  })

  it('fail-open: a failed pre-check never blocks the send', async () => {
    vi.mocked(api.checkToolsSend).mockRejectedValue(new Error('command unavailable'))
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    let send!: Promise<boolean>
    await act(async () => { send = result.current.sendMessage('run the tests') })
    expect(await send).toBe(true)
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(result.current.toolsConfirm).toBeNull()
  })

  it('confirms SEQUENTIALLY after vision: vision send-anyway hands the same payload to the tools gate', async () => {
    // Both gates fire: the payload carries images AND the model lacks tools.
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      vision: false,
      suggestion: null,
    })
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'llama-4-70b',
      provider: 'ollama',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => {
      await result.current.sendMessage('look and run', ['/tmp/pic.png'])
    })
    // Bar one: vision. Tools bar not up yet — one hold at a time.
    expect(result.current.visionConfirm).not.toBeNull()
    expect(result.current.toolsConfirm).toBeNull()

    await act(async () => { await result.current.resolveVisionConfirm('send-anyway') })
    // The vision bar closed and the SAME send is now held by the tools bar.
    expect(result.current.visionConfirm).toBeNull()
    expect(result.current.toolsConfirm).toEqual({
      model: 'llama-4-70b',
      suggestion: null,
    })
    expect(api.sendMessage).not.toHaveBeenCalled()

    await act(async () => { await result.current.resolveToolsConfirm('send-anyway') })
    expect(api.sendMessage).toHaveBeenCalledTimes(1)
    expect(api.sendMessage).toHaveBeenCalledWith('look and run', ['/tmp/pic.png'], undefined, SESSION_A)
  })

  it('a vision-confirmed send skips only the vision arm, not the tools arm', async () => {
    // Vision passes on the first resolution; the tools gate must still run
    // afterwards (visionConfirmed skips ONLY the vision check).
    vi.mocked(api.checkVisionSend).mockResolvedValue({
      model: 'claude-sonnet-4-6',
      provider: 'anthropic',
      vision: true,
    })
    vi.mocked(api.checkToolsSend).mockResolvedValue({
      model: 'claude-sonnet-4-6',
      provider: 'anthropic',
      applies: true,
      tools: false,
      suggestion: null,
    })
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))

    await act(async () => {
      await result.current.sendMessage('look', ['/tmp/pic.png'])
    })
    // Vision passed (true), so the send proceeded to the tools arm and held.
    expect(result.current.visionConfirm).toBeNull()
    expect(result.current.toolsConfirm).not.toBeNull()
    expect(api.checkVisionSend).toHaveBeenCalledTimes(1)
    expect(api.checkToolsSend).toHaveBeenCalledTimes(1)
  })
})

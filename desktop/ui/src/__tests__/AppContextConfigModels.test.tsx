// P1-12 — CONFIG_UPDATED must refresh the chat model catalog.
//
// Providers can be edited from outside this window (CLI, session window);
// those edits emit CONFIG_UPDATED, and the composer's model picker reads the
// CatalogContext `models` slice. Before this, only config + providerStatus
// were refreshed on the event, so the catalog went stale until restart.
//
// Harness mirrors AppContextB1.test.tsx: a real AppProvider whose only
// replacement is a capturing @tauri-apps/api/event fake.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { useCatalog } from '@/context/CatalogContext'
import { EVENT_NAMES } from '@/types'
import * as api from '@/lib/tauri-api'

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

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

beforeEach(() => {
  vi.clearAllMocks()
  localStorage.clear()
  for (const key of Object.keys(captured)) delete captured[key]
  vi.mocked(api.listModels).mockResolvedValue([
    { id: 'claude-sonnet-4-6', name: 'Claude Sonnet', provider: 'anthropic', context_window: 200000 },
  ])
})

describe('P1-12 — CONFIG_UPDATED refreshes the model catalog', () => {
  it('re-fetches models when CONFIG_UPDATED fires', async () => {
    const { result } = renderHook(() => useCatalog(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await waitFor(() => {
      expect(captured[EVENT_NAMES.CONFIG_UPDATED]?.length).toBeGreaterThan(0)
    })
    const callsAfterInit = vi.mocked(api.listModels).mock.calls.length
    expect(callsAfterInit).toBeGreaterThan(0)

    act(() => { flush(EVENT_NAMES.CONFIG_UPDATED, { key: 'providers', value: 'updated' }) })
    await waitFor(() =>
      expect(vi.mocked(api.listModels).mock.calls.length).toBeGreaterThan(callsAfterInit),
    )
  })

  it('the refreshed catalog is visible through CatalogContext.models', async () => {
    const { result } = renderHook(() => useCatalog(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await waitFor(() => {
      expect(captured[EVENT_NAMES.CONFIG_UPDATED]?.length).toBeGreaterThan(0)
    })
    expect(result.current.models.map((m) => m.id)).toEqual(['claude-sonnet-4-6'])

    // A CLI-side providers.toml edit lands as a new catalog on the next fetch.
    vi.mocked(api.listModels).mockResolvedValue([
      { id: 'glm-5', name: 'GLM', provider: 'openai-compatible', context_window: 128000 },
    ])
    act(() => { flush(EVENT_NAMES.CONFIG_UPDATED, { key: 'providers', value: 'updated' }) })
    await waitFor(() => expect(result.current.models.map((m) => m.id)).toEqual(['glm-5']))
  })
})

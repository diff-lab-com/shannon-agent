// P2-5 — "which memories did this turn use" in the ContextBreakdownCard.
//
// The injected-memory list only renders when the memory category carries
// tokens; rows with a source session different from the current one are
// jumpable, entries without provenance render as plain rows, and every
// failure path degrades to "nothing injected".

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import ContextBreakdownCard from '@/components/chat/ContextBreakdownCard'
import { SessionContext, type SessionContextValue } from '@/context/SessionContext'
import * as api from '@/lib/tauri-api'
import type { ContextBreakdown } from '@/types'

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<object>('@/lib/tauri-api')
  return {
    ...actual,
    getSessionContextBreakdown: vi.fn(),
    getSessionUsage: vi.fn(),
    getSessionInjectedMemories: vi.fn(),
  }
})

const switchSession = vi.fn().mockResolvedValue(undefined)

/** Minimal slice — the card only touches currentSessionId/switchSession. */
const sessionValue = {
  currentSessionId: 's1',
  switchSession,
} as unknown as SessionContextValue

function renderCard(sessionId: string | null = 's1', withSessions = true) {
  return render(<ContextBreakdownCard sessionId={sessionId} usageTick={null} />, {
    wrapper: withSessions
      ? ({ children }: { children: React.ReactNode }) => (
          <SessionContext.Provider value={sessionValue}>
            <I18nProvider>{children}</I18nProvider>
          </SessionContext.Provider>
        )
      : I18nProvider,
  })
}

const mockedBreakdown = vi.mocked(api.getSessionContextBreakdown)
const mockedUsage = vi.mocked(api.getSessionUsage)
const mockedInjected = vi.mocked(api.getSessionInjectedMemories)

function makeBreakdown(memoryTokens: number): ContextBreakdown {
  return {
    totalTokens: 1000,
    contextWindow: 200_000,
    categories: [
      { key: 'system', tokens: 200 },
      { key: 'tools', tokens: 300 },
      { key: 'skills', tokens: 100 },
      { key: 'memory', tokens: memoryTokens },
      { key: 'mcp', tokens: 0 },
      { key: 'conversation', tokens: Math.max(0, 400 - memoryTokens) },
    ],
  }
}

beforeEach(() => {
  vi.clearAllMocks()
  mockedBreakdown.mockResolvedValue(makeBreakdown(50))
  mockedUsage.mockResolvedValue({
    input_tokens: 0, output_tokens: 0, cache_creation_tokens: 0, cache_read_tokens: 0, cost_usd: 0, events: 0,
  })
})

describe('ContextBreakdownCard injected memories', () => {
  it('lists injected memories under the memory row when the category has tokens', async () => {
    mockedInjected.mockResolvedValue([
      { id: 'm1', title: 'use pnpm not npm', category: 'preference', sourceSessionId: 's0' },
      { id: 'm2', title: 'deploys via k8s', category: 'decision', sourceSessionId: null },
    ])
    renderCard()

    await waitFor(() => expect(screen.getByTestId('injected-memories')).toBeInTheDocument())
    expect(screen.getByText('Memories used this turn')).toBeInTheDocument()
    expect(screen.getByText('use pnpm not npm')).toBeInTheDocument()
    expect(screen.getByText('deploys via k8s')).toBeInTheDocument()
  })

  it('marks rows from ANOTHER session as jumpable and jumps on click', async () => {
    mockedInjected.mockResolvedValue([
      { id: 'm1', title: 'use pnpm not npm', category: 'preference', sourceSessionId: 's0' },
    ])
    renderCard()
    const jump = await screen.findByTestId('injected-memory-jump-m1')
    fireEvent.click(jump)
    await waitFor(() => expect(switchSession).toHaveBeenCalledWith('s0'))
  })

  it('renders unsourced entries without a jump affordance', async () => {
    mockedInjected.mockResolvedValue([
      { id: 'm2', title: 'deploys via k8s', category: 'decision', sourceSessionId: null },
    ])
    renderCard()
    await waitFor(() => expect(screen.getByText('deploys via k8s')).toBeInTheDocument())
    expect(screen.queryByTestId('injected-memory-jump-m2')).not.toBeInTheDocument()
    expect(screen.getByText('Manual / imported')).toBeInTheDocument()
  })

  it('hides the list when the memory category is empty', async () => {
    mockedBreakdown.mockResolvedValue(makeBreakdown(0))
    mockedInjected.mockResolvedValue([
      { id: 'm1', title: 'use pnpm not npm', category: 'preference', sourceSessionId: 's0' },
    ])
    renderCard()
    await waitFor(() => expect(mockedInjected).toHaveBeenCalled())
    expect(screen.queryByTestId('injected-memories')).not.toBeInTheDocument()
  })

  it('degrades to nothing injected on fetch failure or non-array payload', async () => {
    mockedInjected.mockRejectedValue(new Error('offline'))
    const { unmount } = renderCard()
    await waitFor(() => expect(mockedInjected).toHaveBeenCalled())
    expect(screen.queryByTestId('injected-memories')).not.toBeInTheDocument()
    unmount()

    mockedInjected.mockResolvedValue(undefined as unknown as api.InjectedMemory[])
    renderCard()
    await waitFor(() => expect(screen.getAllByText('System prompt').length).toBeGreaterThan(0))
    expect(screen.queryByTestId('injected-memories')).not.toBeInTheDocument()
  })
})

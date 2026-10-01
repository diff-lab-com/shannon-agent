// R2-P2-2 — a tagged backend hard error on the send path localizes in the
// chat error surface: a known kind renders the mapped message; untagged
// errors keep the raw string.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, waitFor } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}))

function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.listSessions).mockResolvedValue([])
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
})

describe('AppContext — tagged backend hard errors localize (R2-P2-2)', () => {
  it('maps a known kind on the send rejection to localized copy', async () => {
    vi.mocked(api.sendMessage).mockRejectedValue(
      'shannon-error:no_working_dir|No working directory is set — choose one in Settings',
    )
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => { await result.current.sendMessage('see attached') })
    const error = result.current.error ?? ''
    // The tag is gone and the localized copy (en fallback in tests) rendered.
    expect(error).not.toContain('shannon-error:')
    expect(error).toContain('No working directory is set')
    expect(error).toContain('Settings')
  })

  it('keeps an untagged rejection raw', async () => {
    vi.mocked(api.sendMessage).mockRejectedValue('error sending request')
    const { result } = renderHook(() => useApp(), { wrapper })
    await waitFor(() => expect(result.current.loading).toBe(false))
    await act(async () => { await result.current.createSession() })

    await act(async () => { await result.current.sendMessage('hello') })
    expect(result.current.error).toBe('error sending request')
  })
})

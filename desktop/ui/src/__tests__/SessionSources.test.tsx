// office Wave 3 C4 — "Session sources" scratchpad:
//   * AppContext slice: per-session Record<sessionId, string[]> with
//     add (trim + dedupe) / remove (by value), in-memory only;
//   * Context-tab panel UI: add via input, list, cite → composer draft
//     (`[Source] <item>`, never sent), remove, empty state.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook, act, render, screen, fireEvent } from '@testing-library/react'
import { AppProvider, useApp } from '@/context/AppContext'
import { SessionSourcesSection } from '@/pages/chat/ContextPanel'
import { COMPOSER_DRAFT_EVENT } from '@/lib/composerBridge'
import * as api from '@/lib/tauri-api'

const SESSION_A = 'aaaa1111-0000-4000-8000-00000000000a'

// The global setup mocks @/lib/tauri-api wholesale (every export a vi.fn) —
// same harness as AppContextB1.test. Only the calls this flow touches need
// explicit resolutions.
function wrapper({ children }: { children: React.ReactNode }) {
  return <AppProvider>{children}</AppProvider>
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.newSession).mockResolvedValue(SESSION_A)
  vi.mocked(api.listSessions).mockResolvedValue([])
})

describe('AppContext — sessionSources slice (C4)', () => {
  it('addSessionSource appends per session; removeSessionSource deletes by value', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    await act(async () => { await result.current.createSession() })
    expect(result.current.currentSessionId).toBe(SESSION_A)

    act(() => {
      result.current.addSessionSource(SESSION_A, '/tmp/report.docx')
      result.current.addSessionSource(SESSION_A, '  https://example.com/spec  ')
    })
    expect(result.current.sessionSources[SESSION_A]).toEqual([
      '/tmp/report.docx',
      'https://example.com/spec',
    ])

    // Trimmed duplicates are dropped, not appended twice.
    act(() => { result.current.addSessionSource(SESSION_A, '/tmp/report.docx') })
    expect(result.current.sessionSources[SESSION_A]).toHaveLength(2)

    act(() => { result.current.removeSessionSource(SESSION_A, '/tmp/report.docx') })
    expect(result.current.sessionSources[SESSION_A]).toEqual(['https://example.com/spec'])
  })

  it('sessions are isolated and blank/whitespace adds are ignored', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    act(() => {
      result.current.addSessionSource('sess-other', '/b.csv')
      result.current.addSessionSource(SESSION_A, '   ')
      result.current.addSessionSource(SESSION_A, '')
    })
    expect(result.current.sessionSources).toEqual({ 'sess-other': ['/b.csv'] })
  })

  it('removing the last entry clears the session key', async () => {
    const { result } = renderHook(() => useApp(), { wrapper })
    act(() => { result.current.addSessionSource(SESSION_A, '/only.csv') })
    act(() => { result.current.removeSessionSource(SESSION_A, '/only.csv') })
    expect(result.current.sessionSources).toEqual({})
  })
})

// ─── Context-tab panel UI ───

function collectPushes(run: () => void): string[] {
  const texts: string[] = []
  const handler = (e: Event) => texts.push((e as CustomEvent<{ text: string }>).detail.text)
  window.addEventListener(COMPOSER_DRAFT_EVENT, handler)
  try {
    run()
  } finally {
    window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
  }
  return texts
}

describe('SessionSourcesSection panel (C4)', () => {
  it('shows the empty state when the session has no sources', () => {
    render(
      <SessionSourcesSection sessionId={SESSION_A} sources={[]} onAdd={vi.fn()} onRemove={vi.fn()} />,
    )
    expect(screen.getByRole('region', { name: 'Session sources' })).toBeInTheDocument()
    expect(screen.getByTestId('session-sources-empty')).toBeInTheDocument()
  })

  it('adding via the input trims and forwards to onAdd, then clears the field', () => {
    const onAdd = vi.fn()
    render(
      <SessionSourcesSection sessionId={SESSION_A} sources={[]} onAdd={onAdd} onRemove={vi.fn()} />,
    )
    const input = screen.getByTestId('session-source-input')
    fireEvent.change(input, { target: { value: '  /tmp/notes.md  ' } })
    fireEvent.click(screen.getByTestId('session-source-add'))
    expect(onAdd).toHaveBeenCalledWith('  /tmp/notes.md  ')
    expect(input).toHaveValue('')
    // Add is disabled while the draft is blank.
    expect(screen.getByTestId('session-source-add')).toBeDisabled()
  })

  it('lists sources; cite pushes a [Source] composer draft; remove forwards by value', () => {
    const onRemove = vi.fn()
    render(
      <SessionSourcesSection
        sessionId={SESSION_A}
        sources={['/tmp/report.docx', 'https://example.com/spec']}
        onAdd={vi.fn()}
        onRemove={onRemove}
      />
    )
    const items = screen.getAllByTestId('session-source-item')
    expect(items).toHaveLength(2)
    expect(screen.getByTitle('/tmp/report.docx')).toBeInTheDocument()

    const texts = collectPushes(() => {
      const cites = screen.getAllByTestId('session-source-cite')
      fireEvent.click(cites[0])
    })
    expect(texts).toEqual(['[Source] /tmp/report.docx'])

    fireEvent.click(screen.getAllByTestId('session-source-remove')[1])
    expect(onRemove).toHaveBeenCalledWith('https://example.com/spec')
  })

  it('adding is disabled without a session', () => {
    render(<SessionSourcesSection sessionId={null} sources={[]} onAdd={vi.fn()} onRemove={vi.fn()} />)
    expect(screen.getByTestId('session-source-input')).toBeDisabled()
    expect(screen.getByTestId('session-source-add')).toBeDisabled()
  })
})

import { describe, it, expect, vi } from 'vitest'
import { render } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { useDocumentTitle } from '@/hooks/useDocumentTitle'

// H3 (导航批, plan §2.3-3): document.title mirrors the Header's page-context
// derivation — session title on /chat, header.title.* page name elsewhere,
// suffixed with the app.brandName brand ("Shannon Code").

const mockSessionCtx = vi.hoisted(() => ({
  sessions: [] as any[],
  currentSessionId: null as string | null,
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => mockSessionCtx,
}))

function Probe({ path }: { path: string }) {
  useDocumentTitle()
  return <div data-testid="probe">{path}</div>
}

function renderAt(path: string) {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <Probe path={path} />
    </MemoryRouter>,
  )
}

describe('useDocumentTitle', () => {
  it('formats `${page context} · ${app.brandName}` for a non-chat route', () => {
    renderAt('/tasks')
    expect(document.title).toBe('Tasks · Shannon Code')
  })

  it('uses the most-specific mapping on nested routes', () => {
    renderAt('/opc/task/42')
    expect(document.title).toBe('OPC Task · Shannon Code')
  })

  it('carries the current session title on /chat (Header U2 parity)', () => {
    mockSessionCtx.sessions = [
      { id: 's1', title: 'Q3 roadmap brainstorm', created_at: 1, message_count: 0 },
    ]
    mockSessionCtx.currentSessionId = 's1'
    renderAt('/chat')
    expect(document.title).toBe('Q3 roadmap brainstorm · Shannon Code')
  })

  it('falls back to the fixed Chat title when no session is active', () => {
    mockSessionCtx.sessions = []
    mockSessionCtx.currentSessionId = null
    renderAt('/chat')
    expect(document.title).toBe('Chat · Shannon Code')
  })
})

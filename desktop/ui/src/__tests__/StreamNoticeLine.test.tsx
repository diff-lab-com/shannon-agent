// R5-2 — the chat conversation renders retry notices as subtle system-style
// lines (muted, small, distinct icon per kind), NOT error banners: a
// failover/key-rotation notice means the engine RECOVERED and the request
// continued. These tests pin the rendering branch of StreamNoticeLine and
// its placement in MessageArea.

import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { StreamNoticeLine } from '@/pages/chat/MessageArea'
import type { StreamNotice } from '@/context/ChatContext'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <MemoryRouter>{ui}</MemoryRouter>
    </I18nProvider>
  )
}

describe('StreamNoticeLine (R5-2)', () => {
  it('renders a failover notice with the failover kind testid and icon', () => {
    const notice: StreamNotice = {
      id: 1,
      kind: 'failover',
      message: 'falling back to glm-5.3-flash@zhipu (rate limited)',
    }
    render(wrap(<StreamNoticeLine notice={notice} />))
    const line = screen.getByTestId('stream-notice-failover')
    expect(line).toBeInTheDocument()
    // Localized label (en baseline) + verbatim engine line as the detail.
    expect(screen.getByText('Model fallback')).toBeInTheDocument()
    expect(
      screen.getByText('falling back to glm-5.3-flash@zhipu (rate limited)'),
    ).toBeInTheDocument()
    // Distinct material icon per kind; aria-hidden (decorative).
    const icon = line.querySelector('.material-symbols-outlined')
    expect(icon).not.toBeNull()
    expect(icon?.textContent).toBe('alt_route')
    expect(icon?.getAttribute('aria-hidden')).toBe('true')
  })

  it('renders a key-rotation notice with its own kind testid and icon', () => {
    const notice: StreamNotice = {
      id: 2,
      kind: 'key_rotation',
      message: 'rotating API key (2/3) for openai (429 rate limited)',
    }
    render(wrap(<StreamNoticeLine notice={notice} />))
    const line = screen.getByTestId('stream-notice-key_rotation')
    expect(line).toBeInTheDocument()
    expect(screen.getByText('API key rotation')).toBeInTheDocument()
    expect(
      screen.getByText('rotating API key (2/3) for openai (429 rate limited)'),
    ).toBeInTheDocument()
    const icon = line.querySelector('.material-symbols-outlined')
    expect(icon?.textContent).toBe('vpn_key')
  })

  it('never renders as an error banner (no alert role, muted presentation)', () => {
    // The whole point of R5-2: a recovered failover must not read as a
    // failure. Assert the notice line carries no error semantics.
    const notice: StreamNotice = { id: 3, kind: 'failover', message: 'falling back to x@y (z)' }
    const { container } = render(wrap(<StreamNoticeLine notice={notice} />))
    expect(screen.queryByRole('alert')).toBeNull()
    expect(container.querySelector('[data-testid="auth-error-banner"]')).toBeNull()
  })
})

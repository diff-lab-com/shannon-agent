import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import WelcomeState from '@/components/WelcomeState'
import type { ProviderStatus } from '@/types'

// WelcomeState reads the provider-status snapshot (CTA gate) and navigates
// to /settings/models (CTA click) — both need providers in the test harness.
function renderWelcomeState(
  onSelectPrompt: (prompt: string) => void = () => {},
  providerStatus: ProviderStatus | null = {
    active_provider_id: 'anthropic-main',
    display_name: 'Anthropic',
    kind: 'anthropic',
    has_api_key: true,
    model: null,
    env_provider: null,
  },
) {
  const value = { providerStatus } as CatalogContextValue
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={['/chat']}>
        <CatalogContext.Provider value={value}>
          <WelcomeState onSelectPrompt={onSelectPrompt} />
        </CatalogContext.Provider>
      </MemoryRouter>
    </I18nProvider>,
  )
}

const UNCONFIGURED: ProviderStatus = {
  active_provider_id: null,
  display_name: null,
  kind: null,
  has_api_key: false,
  model: null,
  env_provider: null,
}

describe('WelcomeState', () => {
  it('renders hero heading and rotating subtitle', () => {
    renderWelcomeState()
    expect(screen.getByText('What can I help with?')).toBeInTheDocument()
    // The subtitle is now "Shannon can help you <TextLoop items={...}>".
    // Under jsdom + query-aware matchMedia (prefers-reduced-motion: reduce
    // returns matches: true) the loop is static on items[0] = "draft emails".
    expect(screen.getByText('Shannon can help you')).toBeInTheDocument()
    expect(screen.getByTestId('text-loop').textContent).toBe('draft emails')
  })

  it('renders exactly 4 template cards', () => {
    renderWelcomeState()
    expect(screen.getByText('Draft an email')).toBeInTheDocument()
    expect(screen.getByText('Summarize')).toBeInTheDocument()
    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('Write code')).toBeInTheDocument()
  })

  it('calls onSelectPrompt with email prompt when email card clicked', () => {
    const spy = vi.fn()
    renderWelcomeState(spy)
    fireEvent.click(screen.getByText('Draft an email'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/follow-up email/i)
  })

  it('calls onSelectPrompt with summary prompt when summary card clicked', () => {
    const spy = vi.fn()
    renderWelcomeState(spy)
    fireEvent.click(screen.getByText('Summarize'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/5 bullet points/i)
  })

  it('calls onSelectPrompt with research prompt when research card clicked', () => {
    const spy = vi.fn()
    renderWelcomeState(spy)
    fireEvent.click(screen.getByText('Research'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/Rust web frameworks/i)
  })

  it('calls onSelectPrompt with code prompt when code card clicked', () => {
    const spy = vi.fn()
    renderWelcomeState(spy)
    fireEvent.click(screen.getByText('Write code'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/REST API endpoint in Rust/i)
  })

  it('shows keyboard hint chips (D9-a: the Alt+Up history item is deleted, not advertised)', () => {
    renderWelcomeState()
    expect(screen.getByText('Commands')).toBeInTheDocument()
    expect(screen.getByText('Shortcuts')).toBeInTheDocument()
    // D9-a (chat-testing v2 §9.3): input-history recall is a product backlog
    // item (A-22) — the welcome row must not advertise it anymore. Deleted,
    // not restyled: the key is gone from every locale too (check:i18n gate).
    expect(screen.queryByText('History')).not.toBeInTheDocument()
  })

  // Review §3-A1 (item e): unconfigured users get a prominent provider CTA
  // on the empty chat canvas — never shown once configured.
  it('shows the provider CTA card when nothing is configured', () => {
    renderWelcomeState(() => {}, UNCONFIGURED)
    expect(screen.getByTestId('welcome-provider-cta')).toBeInTheDocument()
    expect(screen.getByText('Connect a provider to start')).toBeInTheDocument()
  })

  it('hides the provider CTA card when a provider is configured', () => {
    renderWelcomeState()
    expect(screen.queryByTestId('welcome-provider-cta')).not.toBeInTheDocument()
  })

  it('hides the provider CTA card when only an env provider is configured', () => {
    renderWelcomeState(() => {}, { ...UNCONFIGURED, env_provider: 'anthropic' })
    expect(screen.queryByTestId('welcome-provider-cta')).not.toBeInTheDocument()
  })

  it('hides the provider CTA card while the snapshot is unavailable', () => {
    renderWelcomeState(() => {}, null)
    expect(screen.queryByTestId('welcome-provider-cta')).not.toBeInTheDocument()
  })
})

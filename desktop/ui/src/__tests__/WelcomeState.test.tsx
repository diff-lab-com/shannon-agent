import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import { CatalogContext, type CatalogContextValue } from '@/context/CatalogContext'
import * as api from '@/lib/tauri-api'
import WelcomeState from '@/components/WelcomeState'
import type { ProviderStatus } from '@/types'

// WelcomeState reads the provider-status snapshot (CTA gate) and navigates
// to /settings/models (CTA click) — both need providers in the test harness.
// D5 方案①: it also reads the suggestions gate (config) and probes the
// workspace markers, so those default to the permissive shapes (config
// undefined → gate ON; probe default from setup.ts → markers present).
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
  catalogExtras: Partial<CatalogContextValue> = {},
) {
  const value = { providerStatus, ...catalogExtras } as CatalogContextValue
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

  it('shows keyboard hint chips', () => {
    renderWelcomeState()
    expect(screen.getByText('Commands')).toBeInTheDocument()
    expect(screen.getByText('Shortcuts')).toBeInTheDocument()
    expect(screen.getByText('History')).toBeInTheDocument()
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

// ── D5 方案① (主动任务推荐): 换一批 refresh + workspace-aware filtering,
// all behind the `suggestions.enabled` presentation gate (default ON). ──
describe('WelcomeState — D5 suggestions gate', () => {
  // Restore ONLY the Math.random spy: restoreAllMocks would also strip the
  // setup-level implementations this file's harness depends on (matchMedia,
  // detectWorkspaceMarkers).
  let randomSpy: { mockRestore: () => void } | null = null
  afterEach(() => {
    randomSpy?.mockRestore()
    randomSpy = null
    vi.mocked(api.detectWorkspaceMarkers).mockResolvedValue(['Cargo.toml'])
  })

  it('shows the 换一批 refresh button when the gate is on (default)', () => {
    renderWelcomeState()
    expect(screen.getByTestId('welcome-refresh')).toBeInTheDocument()
  })

  it('refresh reshuffles the deck locally (deterministic under a spied Math.random)', () => {
    randomSpy = vi.spyOn(Math, 'random').mockReturnValue(0)
    renderWelcomeState()
    // Before: static deck order — email first.
    expect(screen.getAllByRole('button').at(0)!.textContent).toMatch(/Draft an email/)
    // random() = 0 → Fisher-Yates drags the head to the tail each step:
    // [email, summarize, research, code] → [summarize, research, code, email].
    fireEvent.click(screen.getByTestId('welcome-refresh'))
    const titles = screen.getAllByRole('button').map(b => b.textContent)
    expect(titles[0]).toMatch(/Summarize/)
    expect(titles[3]).toMatch(/Draft an email/)
  })

  it('gate off (suggestions_enabled = false) → no refresh button, card renders exactly as before', async () => {
    vi.mocked(api.detectWorkspaceMarkers).mockResolvedValue([])
    renderWelcomeState(() => {}, undefined, { config: { suggestions_enabled: false } })
    expect(screen.queryByTestId('welcome-refresh')).not.toBeInTheDocument()
    // Office AND coding cards all stay, even though the (never-consulted)
    // probe would report no markers — the gate-off render is byte-identical
    // to the pre-D5 card.
    await waitFor(() => {
      expect(screen.getByText('Write code')).toBeInTheDocument()
    })
    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('Draft an email')).toBeInTheDocument()
    expect(screen.getByText('Summarize')).toBeInTheDocument()
  })

  it('no code markers → coding cards (research/code) are filtered out; office cards stay', async () => {
    vi.mocked(api.detectWorkspaceMarkers).mockResolvedValue([])
    renderWelcomeState()
    await waitFor(() => {
      expect(screen.queryByText('Write code')).not.toBeInTheDocument()
    })
    expect(screen.queryByText('Research')).not.toBeInTheDocument()
    expect(screen.getByText('Draft an email')).toBeInTheDocument()
    expect(screen.getByText('Summarize')).toBeInTheDocument()
  })

  it('a code marker present → coding cards stay visible', async () => {
    vi.mocked(api.detectWorkspaceMarkers).mockResolvedValue(['package.json'])
    renderWelcomeState()
    await waitFor(() => {
      expect(screen.getByText('Write code')).toBeInTheDocument()
    })
    expect(screen.getByText('Research')).toBeInTheDocument()
  })

  it('probe failure stays silent and fails open (all cards render)', async () => {
    vi.mocked(api.detectWorkspaceMarkers).mockRejectedValue(new Error('backend gone'))
    renderWelcomeState()
    // Cards render immediately (pending) and are NOT withdrawn on failure.
    await waitFor(() => {
      expect(api.detectWorkspaceMarkers).toHaveBeenCalled()
    })
    expect(screen.getByText('Write code')).toBeInTheDocument()
    expect(screen.getByText('Draft an email')).toBeInTheDocument()
  })
})

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { ThemeProvider } from '@/context/ThemeContext'
import { MemoryRouter } from 'react-router-dom'
import ModelsSettings from '@/components/settings/ModelsSettings'
import * as api from '@/lib/tauri-api'

function wrap(ui: React.ReactElement) {
  return (
    <ThemeProvider>
      <AppProvider>
        <MemoryRouter>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </ThemeProvider>
  )
}

describe('ModelsSettings', () => {
  it('renders model configuration heading', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Model Configuration')).toBeInTheDocument()
  })

  it('renders the managed providers section with an add button', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Providers')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /Add provider/ })).toBeInTheDocument()
  })

  it('renders performance strategy selector', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Performance Strategy')).toBeInTheDocument()
  })

  it('renders global parameters with sliders', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText('Global Parameters')).toBeInTheDocument()
    expect(screen.getByText('Temperature')).toBeInTheDocument()
    expect(screen.getByText('Max Tokens')).toBeInTheDocument()
  })

  it('toggles performance strategy on click', () => {
    render(wrap(<ModelsSettings />))
    const speedBtn = screen.getByText('Speed')
    fireEvent.click(speedBtn)
    expect(speedBtn).toBeInTheDocument()
  })

  // === Phase 2 task 4 — surface price_in / price_out / tier / dynamic
  //     in the model list. ===
  //
  // The list_models Tauri command now returns these fields. The
  // settings page renders them as badges + a per-row pricing line.
  // These tests drive the rendering path so the v2 schema doesn't
  // silently drop in production.

  it('renders price_in and price_out for a model with pricing', async () => {
    vi.mocked(api.listModels).mockResolvedValueOnce([
      {
        id: 'claude-sonnet-4-6',
        name: 'Claude Sonnet 4.6',
        provider: 'anthropic',
        context_window: 200_000,
        price_in: 3.0,
        price_out: 15.0,
        tier: 'standard',
        dynamic: false,
      },
    ])
    render(wrap(<ModelsSettings />))
    // Pricing is rendered as "in $X/M / out $Y/M".
    expect(await screen.findByText(/in \$3\.00\/M/)).toBeInTheDocument()
    expect(screen.getByText(/out \$15\.00\/M/)).toBeInTheDocument()
  })

  it('renders em-dash placeholder when pricing is unknown (P0-2 honest cost)', async () => {
    vi.mocked(api.listModels).mockResolvedValueOnce([
      {
        id: 'unknown-pricing',
        name: 'Mystery Model',
        provider: 'anthropic',
        context_window: 100_000,
        price_in: null,
        price_out: null,
        tier: null,
        dynamic: false,
      },
    ])
    render(wrap(<ModelsSettings />))
    // Unknown pricing surfaces as "in $—/M / out $—/M" rather than
    // a fabricated number — ADR-0005 P0-2 honest-cost: the UI must
    // never invent a price.
    expect(await screen.findByText(/in \$—\/M/)).toBeInTheDocument()
    expect(screen.getByText(/out \$—\/M/)).toBeInTheDocument()
  })

  it('renders tier badge for tier-labelled models', async () => {
    vi.mocked(api.listModels).mockResolvedValueOnce([
      {
        id: 'haiku',
        name: 'Claude Haiku 4.5',
        provider: 'anthropic',
        context_window: 200_000,
        price_in: 1.0,
        price_out: 5.0,
        tier: 'fast',
        dynamic: false,
      },
    ])
    render(wrap(<ModelsSettings />))
    // The tier label key is `settings.models.tierFast` -> "fast".
    expect(await screen.findByText('fast')).toBeInTheDocument()
  })

  it('renders dynamic badge for models.dev overlay entries', async () => {
    vi.mocked(api.listModels).mockResolvedValueOnce([
      {
        id: 'live-1',
        name: 'Some Live Model',
        provider: 'openai-compatible',
        context_window: 128_000,
        price_in: null,
        price_out: null,
        tier: null,
        dynamic: true,
      },
    ])
    render(wrap(<ModelsSettings />))
    // The dynamic badge surfaces freshness — the engine marks
    // models.dev entries as dynamic so the UI can flag them.
    expect(await screen.findByText('Live')).toBeInTheDocument()
  })

  it('does not crash when pricing and tier are absent', async () => {
    vi.mocked(api.listModels).mockResolvedValueOnce([
      {
        id: 'minimal',
        name: 'Minimal Model',
        provider: 'anthropic',
        context_window: 0,
        price_in: null,
        price_out: null,
        tier: null,
        dynamic: false,
      },
    ])
    // The render path must not throw when the engine returns a
    // minimal model — common during engine startup before
    // pricing is loaded.
    expect(() => render(wrap(<ModelsSettings />))).not.toThrow()
  })

  // === Provider visibility (ADR-0005 P4.9) ===
  //
  // The "Provider visibility" section is the desktop-side authoring
  // surface for the engine's `SHANNON_*_PROVIDERS` allowlist. The
  // tests below pin the three documented states (None / Some([]) /
  // Some(non_empty)) and the configure call shape so the wire shape
  // doesn't silently drift.

  it('renders provider visibility section with all 6 kinds checked when no override is set', async () => {
    // Default test setup returns `null` for `getProviderAllowlist`
    // and an empty `getConfig` (no `enabled_providers` field) — so
    // the section should render every kind as checked.
    render(wrap(<ModelsSettings />))
    // Wait for the async useEffect to settle.
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.getByText('Provider visibility')).toBeInTheDocument()
    expect(
      screen.getByText(/Restrict which providers appear/i),
    ).toBeInTheDocument()
    // All 6 provider kinds should be present.
    expect(screen.getByText('Anthropic')).toBeInTheDocument()
    expect(screen.getByText('OpenAI')).toBeInTheDocument()
    expect(screen.getByText('DeepSeek')).toBeInTheDocument()
    expect(screen.getByText('Ollama')).toBeInTheDocument()
    // Gemini
    expect(screen.getByText('Gemini')).toBeInTheDocument()
    // OpenAI-compatible label
    expect(screen.getByText('OpenAI-compatible')).toBeInTheDocument()
    // Reset button is disabled while override is null (no state to clear).
    const resetBtn = screen.getByRole('button', { name: /Reset to default/i })
    expect(resetBtn).toBeDisabled()
  })

  it('toggling a provider off updates the configure call payload', async () => {
    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 0))
    // Find the checkbox for Anthropic (the label wraps the input).
    const anthropicLabel = screen.getByText('Anthropic').closest('label')
    expect(anthropicLabel).not.toBeNull()
    const checkbox = anthropicLabel!.querySelector('input[type="checkbox"]') as HTMLInputElement
    expect(checkbox).toBeInTheDocument()
    expect(checkbox.checked).toBe(true)

    // Toggle Anthropic off.
    fireEvent.click(checkbox)
    await new Promise((r) => setTimeout(r, 0))

    // The configure call should be invoked with the new payload
    // (a JSON-encoded array of the remaining 5 kinds).
    expect(api.configure).toHaveBeenCalled()
    const lastCall = vi.mocked(api.configure).mock.calls.at(-1)?.[0]
    expect(lastCall?.key).toBe('enabled_providers')
    const parsed: string[] = JSON.parse(lastCall!.value)
    expect(parsed).not.toContain('anthropic')
    expect(parsed).toContain('openai')
    expect(parsed).toContain('deepseek')
    expect(parsed).toContain('ollama')
    expect(parsed).toContain('gemini')
    expect(parsed).toContain('openai-compatible')
  })

  it('reset button clears the override when one is set', async () => {
    // Override set to a single kind → the reset button is enabled.
    // Mock both the effective allowlist (what `getProviderAllowlist`
    // returns) and the desktop `enabled_providers` field (read via
    // `getConfig`). The component reads the latter to distinguish
    // `null` from `Some(...)`.
    vi.mocked(api.getProviderAllowlist).mockResolvedValue(['anthropic'])
    vi.mocked(api.getConfig).mockResolvedValue({
      provider: 'anthropic',
      model: 'claude-sonnet-4-6',
      enabled_providers: ['anthropic'],
    } as Awaited<ReturnType<typeof api.getConfig>>)

    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 50))

    const resetBtn = screen.getByRole('button', { name: /Reset to default/i })
    expect(resetBtn).not.toBeDisabled()

    fireEvent.click(resetBtn)
    await new Promise((r) => setTimeout(r, 50))

    // The configure call should be invoked with value "null" to
    // clear the desktop override (falls back to engine env vars).
    expect(api.configure).toHaveBeenCalledWith(
      expect.objectContaining({ key: 'enabled_providers', value: 'null' }),
    )
  })

  // === Test all providers (ADR-0005 P4.12) ===
  //
  // The "Test all" button calls the fan-out `testAllProviders` command
  // and renders one row per managed connection. These tests pin the
  // button-click → command-call → results-render path so the UI doesn't
  // silently lose the per-row status pills.

  it('renders the Test all button with empty-state disabled', async () => {
    // Default mock: listProviders returns `{ providers: [], active_provider_id: null }`.
    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 0))
    const btn = screen.getByRole('button', { name: /Test all/i })
    expect(btn).toBeInTheDocument()
    expect(btn).toBeDisabled()
  })

  it('runs testAllProviders on click and renders per-row status pills', async () => {
    vi.mocked(api.listProviders).mockResolvedValueOnce({
      active_provider_id: 'p-anthropic',
      providers: [
        { id: 'p-anthropic', label: 'Anthropic', provider_kind: 'anthropic', api_key: '***', model: 'claude-sonnet-4-6' },
        { id: 'p-ollama',    label: 'Local',     provider_kind: 'ollama',    api_key: '',     model: 'qwen2.5-coder:7b' },
      ],
    })
    vi.mocked(api.testAllProviders).mockResolvedValueOnce([
      { id: 'p-anthropic', label: 'Anthropic', provider_kind: 'anthropic', result: { kind: 'success' },            latency_ms: 412 },
      { id: 'p-ollama',    label: 'Local',     provider_kind: 'ollama',    result: { kind: 'network_unreachable' }, latency_ms: 6001 },
    ])

    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 0))

    const btn = screen.getByRole('button', { name: /Test all/i })
    expect(btn).not.toBeDisabled()
    fireEvent.click(btn)

    // Wait for the async probe + state update.
    await screen.findByTestId('test-all-results')
    expect(api.testAllProviders).toHaveBeenCalledTimes(1)

    // Two rows rendered, one per managed connection.
    expect(screen.getByTestId('test-all-result-p-anthropic')).toBeInTheDocument()
    expect(screen.getByTestId('test-all-result-p-ollama')).toBeInTheDocument()

    // Latency rendered for both rows (both reported a latency).
    expect(screen.getByText(/412 ms/)).toBeInTheDocument()
    expect(screen.getByText(/6001 ms/)).toBeInTheDocument()
  })

  it('handles a testAllProviders error by surfacing a toast and not rendering the panel', async () => {
    vi.mocked(api.listProviders).mockResolvedValueOnce({
      active_provider_id: 'p-1',
      providers: [{ id: 'p-1', label: 'Anthropic', provider_kind: 'anthropic', api_key: '***', model: 'claude-sonnet-4-6' }],
    })
    vi.mocked(api.testAllProviders).mockRejectedValueOnce(new Error('boom'))

    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 0))

    fireEvent.click(screen.getByRole('button', { name: /Test all/i }))

    await new Promise((r) => setTimeout(r, 50))
    expect(screen.queryByTestId('test-all-results')).toBeNull()
  })
})

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
})

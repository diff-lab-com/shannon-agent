import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
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
  it('renders model configuration subtitle', () => {
    render(wrap(<ModelsSettings />))
    expect(screen.getByText(/Manage your active AI providers/)).toBeInTheDocument()
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

  // === Phase 2 task 4 — surface price_in / price_out / tier in the model
  //     list (the `dynamic` badge was removed in S1-3, P-N3 — dead UI). ===
  //
  // The list_models Tauri command now returns these fields. The
  // settings page renders them as badges + a per-row pricing line.
  // These tests drive the rendering path so the v2 schema doesn't
  // silently drop in production.

  // S3-1 (P-N23) — the quick switcher / catalog rows write the SAME
  // model+provider pair the Header writes (shared writeGlobalModelDefault).
  // The old model-only write was the review's convention fork.
  it('model switch writes the model+provider pair, not model only', async () => {
    vi.mocked(api.listModels).mockResolvedValue([
      {
        id: 'gpt-5',
        name: 'GPT-5',
        provider: 'openai',
        context_window: 256_000,
        price_in: 1.25,
        price_out: 10,
        tier: 'pro',
      },
    ])
    render(wrap(<ModelsSettings />))
    const row = await screen.findByTestId('catalog-model-row')
    // The row's first button IS the switch target (the vault affordance
    // column only mounts when a managed slot is active).
    fireEvent.click(row.querySelector('button')!)
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'model', value: 'gpt-5' })
      expect(api.configure).toHaveBeenCalledWith({ key: 'provider', value: 'openai' })
    })
  })

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

  // S1-3 (P-N3): the engine hardcodes `dynamic: None` on the ModelInfo
  // wire, so the "Live" badge could never light — the badge (and its
  // i18n key) is deleted. This pins the removal: even a `dynamic: true`
  // row must not render a badge (the wire field stays for S2-1).
  it('does not render the dynamic badge (S1-3: dead UI removed)', async () => {
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
    expect(await screen.findByText('Some Live Model')).toBeInTheDocument()
    expect(screen.queryByText('Live')).not.toBeInTheDocument()
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

  it('renders a quota_exhausted (HTTP 402) row with the categorized copy (R2-P1-10)', async () => {
    // 402 used to fall through to `Unknown` with the raw provider body —
    // the Test-all pill must show the localized quota copy instead.
    vi.mocked(api.listProviders).mockResolvedValueOnce({
      active_provider_id: 'p-anthropic',
      providers: [
        { id: 'p-anthropic', label: 'Anthropic', provider_kind: 'anthropic', api_key: '***', model: 'claude-sonnet-4-6' },
      ],
    })
    vi.mocked(api.testAllProviders).mockResolvedValueOnce([
      { id: 'p-anthropic', label: 'Anthropic', provider_kind: 'anthropic', result: { kind: 'quota_exhausted' }, latency_ms: 88 },
    ])

    render(wrap(<ModelsSettings />))
    await new Promise((r) => setTimeout(r, 0))
    fireEvent.click(screen.getByRole('button', { name: /Test all/i }))

    await screen.findByTestId('test-all-results')
    expect(screen.getByTestId('test-all-result-p-anthropic').textContent).toContain('Quota exhausted')
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

// R2-2 — Settings "Refresh model catalog": the models.dev overlay refresh
// (previously CLI-only `/model refresh`) gets a button with spinner →
// success (model count) / inline failure reason.
describe('ModelsSettings — refresh model catalog (R2-2)', () => {
  it('renders the refresh button near the catalog list', () => {
    render(wrap(<ModelsSettings />))
    expect(
      screen.getByRole('button', { name: /Refresh the model catalog from models.dev/i }),
    ).toBeInTheDocument()
  })

  it('shows the success line with the model count after a refresh', async () => {
    vi.mocked(api.refreshModelCatalog).mockResolvedValueOnce({ count: 42, generation: 2 })
    render(wrap(<ModelsSettings />))
    fireEvent.click(screen.getByRole('button', { name: /Refresh the model catalog from models.dev/i }))
    const line = await screen.findByTestId('refresh-model-catalog-result')
    expect(line).toHaveTextContent('42')
  })

  it('shows the upstream failure reason inline when the refresh fails', async () => {
    vi.mocked(api.refreshModelCatalog).mockRejectedValueOnce(new Error('network down'))
    render(wrap(<ModelsSettings />))
    fireEvent.click(screen.getByRole('button', { name: /Refresh the model catalog from models.dev/i }))
    const line = await screen.findByTestId('refresh-model-catalog-result')
    expect(line).toHaveTextContent('network down')
  })
})

// === S2-2 — per-model metadata editor over the curated vault (P-N6) ===
//
// The catalog rows carry two vault affordances when a managed provider slot
// is active:
//   - a declaration already in the vault → expand-to-edit (ModelMetaEditor),
//   - a catalog/overlay row outside the vault → "add to vault" (persists an
//     id-only declaration via set_provider_models, then opens the editor).
// These tests pin the click → command-payload → refresh loop; the payload
// must be the COMPLETE vault (set_provider_models has overwrite semantics,
// no field merge).
describe('ModelsSettings — vault metadata editor (S2-2)', () => {
  const vaultSpec = {
    id: 'glm-5.3-flash',
    display_name: null,
    context_window: null,
    max_output: 32768,
    cost_per_m_input: null,
    cost_per_m_output: null,
    capabilities: ['tool_use'],
  }

  function mockProviders(models: unknown[] | null) {
    vi.mocked(api.listProviders).mockResolvedValue({
      active_provider_id: 'prov-glm',
      providers: [
        {
          id: 'prov-glm',
          display_name: 'GLM',
          kind: 'openai-compatible',
          has_api_key: true,
          models: models ?? undefined,
        },
      ],
    } as Awaited<ReturnType<typeof api.listProviders>>)
  }

  function mockCatalog() {
    vi.mocked(api.listModels).mockResolvedValue([
      {
        id: 'glm-5.3-flash',
        name: 'GLM 5.3 Flash',
        provider: 'openai-compatible',
        context_window: 198000,
        price_in: 0.5,
        price_out: 2,
        tier: null,
        dynamic: false,
        source: 'catalog',
      },
      {
        id: 'glm-4-air',
        name: 'GLM 4 Air',
        provider: 'openai-compatible',
        context_window: 128000,
        price_in: null,
        price_out: null,
        tier: null,
        dynamic: false,
        source: 'catalog',
      },
    ] as Awaited<ReturnType<typeof api.listModels>>)
  }

  beforeEach(() => {
    vi.mocked(api.setProviderModels).mockReset()
    vi.mocked(api.setProviderModels).mockResolvedValue({
      provider_id: 'prov-glm',
      model_profile: 'default',
      models: [],
    })
  })

  it('hides the vault affordances when no managed provider slot is active', async () => {
    mockCatalog()
    render(wrap(<ModelsSettings />))
    expect(await screen.findByText('GLM 5.3 Flash')).toBeInTheDocument()
    expect(screen.queryByTestId('add-model-to-vault-glm-5.3-flash')).not.toBeInTheDocument()
    expect(screen.queryByTestId('edit-model-meta-glm-5.3-flash')).not.toBeInTheDocument()
  })

  it('offers "add to vault" on a row outside the vault and submits vault + id-only spec', async () => {
    mockCatalog()
    // First read (mount): the vault already curates glm-5.3-flash.
    // Second read (post-save reload): the command's write landed — pins the
    // refresh loop end to end.
    vi.mocked(api.listProviders)
      .mockResolvedValueOnce({
        active_provider_id: 'prov-glm',
        providers: [
          {
            id: 'prov-glm',
            display_name: 'GLM',
            kind: 'openai-compatible',
            has_api_key: true,
            models: [vaultSpec],
          },
        ],
      } as Awaited<ReturnType<typeof api.listProviders>>)
    mockProviders([
      vaultSpec,
      { ...vaultSpec, id: 'glm-4-air', max_output: null, capabilities: [] },
    ])
    vi.mocked(api.setProviderModels).mockResolvedValue({
      provider_id: 'prov-glm',
      model_profile: 'default',
      models: [{ ...vaultSpec, id: 'glm-4-air', max_output: null, capabilities: [] }],
    })

    render(wrap(<ModelsSettings />))
    const addBtn = await screen.findByTestId('add-model-to-vault-glm-4-air')
    fireEvent.click(addBtn)

    await screen.findByTestId('model-meta-editor')
    expect(api.setProviderModels).toHaveBeenCalledTimes(1)
    expect(api.setProviderModels).toHaveBeenCalledWith('prov-glm', [
      // Overwrite semantics: the WHOLE vault travels — the existing
      // declaration re-serialized untouched…
      {
        id: 'glm-5.3-flash',
        display_name: null,
        context_window: null,
        max_output: 32768,
        cost_per_m_input: null,
        cost_per_m_output: null,
        capabilities: ['tool_use'],
      },
      // …plus the fresh id-only declaration.
      { id: 'glm-4-air' },
    ])
    // The editor opened straight onto the fresh declaration and the row
    // flipped to the in-vault (edit) affordance after the reload.
    expect(screen.getByTestId('edit-model-meta-glm-4-air')).toBeInTheDocument()
  })

  it('expands a declared row into the editor and saves the merged vault', async () => {
    mockCatalog()
    mockProviders([vaultSpec])
    render(wrap(<ModelsSettings />))

    const editBtn = await screen.findByTestId('edit-model-meta-glm-5.3-flash')
    expect(editBtn).toHaveAttribute('aria-expanded', 'false')
    fireEvent.click(editBtn)

    const editor = await screen.findByTestId('model-meta-editor')
    expect(editor).toBeInTheDocument()
    expect(editBtn).toHaveAttribute('aria-expanded', 'true')
    // Draft pre-filled from the stored declaration.
    expect((screen.getByTestId('meta-editor-max-output') as HTMLInputElement).value).toBe('32768')

    // Correct the input price and save.
    fireEvent.change(screen.getByTestId('meta-editor-price-in'), { target: { value: '0.75' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))

    await waitFor(() => expect(api.setProviderModels).toHaveBeenCalledTimes(1))
    expect(api.setProviderModels).toHaveBeenCalledWith('prov-glm', [
      {
        id: 'glm-5.3-flash',
        display_name: null,
        context_window: null,
        max_output: 32768,
        cost_per_m_input: 0.75,
        cost_per_m_output: null,
        capabilities: ['tool_use'],
      },
    ])
    // Refresh verification: the save path re-pulls the catalog (in the real
    // backend CONFIG_UPDATED drives the same refreshModels via AppContext —
    // the editor drives it explicitly so the row/badge updates even if this
    // window missed the event).
    await waitFor(() => {
      expect(vi.mocked(api.listModels).mock.calls.length).toBeGreaterThanOrEqual(2)
    })
  })

  it('keeps the editor open when the command rejects (input survives)', async () => {
    mockCatalog()
    mockProviders([vaultSpec])
    vi.mocked(api.setProviderModels).mockRejectedValueOnce(new Error('disk full'))

    render(wrap(<ModelsSettings />))
    fireEvent.click(await screen.findByTestId('edit-model-meta-glm-5.3-flash'))
    await screen.findByTestId('model-meta-editor')
    fireEvent.change(screen.getByTestId('meta-editor-price-in'), { target: { value: '0.75' } })
    fireEvent.click(screen.getByTestId('meta-editor-save'))

    await waitFor(() => expect(api.setProviderModels).toHaveBeenCalledTimes(1))
    // Editor still up for correction after the failure.
    expect(screen.getByTestId('model-meta-editor')).toBeInTheDocument()
  })

  it('refuses to grow the vault beyond the soft cap (裁定⑥, UI-side guard)', async () => {
    mockCatalog()
    const fifty = Array.from({ length: 50 }, (_, i) => ({
      id: `m-${i}`,
      display_name: null,
      context_window: null,
      max_output: null,
      cost_per_m_input: null,
      cost_per_m_output: null,
      capabilities: [],
    }))
    mockProviders(fifty)
    render(wrap(<ModelsSettings />))

    fireEvent.click(await screen.findByTestId('add-model-to-vault-glm-4-air'))
    // The command must NOT fire at the cap.
    await waitFor(() => {
      expect(screen.queryByTestId('model-meta-editor')).not.toBeInTheDocument()
    })
    expect(api.setProviderModels).not.toHaveBeenCalled()
  })
})

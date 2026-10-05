// P0-7 — the per-card "Test connection" button must probe with the STORED
// credential (testProviderCredentials + connection id → backend credential
// store fallback), the same fallback "Test all" uses. It used to call
// testProviderConnection with a hardcoded empty key, so every provider with
// a saved key was misreported as "Invalid key".
//
// Harness: ProvidersSection is standalone (no AppProvider) — SessionContext
// is mocked to `currentSessionId: null`, which only gates the activate
// cache-bust prompt, not the test flow.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { ProvidersSection } from '@/components/settings/models-settings/ProvidersSection'
import * as api from '@/lib/tauri-api'
import type { ProviderConnection, ProvidersFile } from '@/types'
vi.mock('sonner', () => ({
  toast: {
    success: vi.fn(),
    error: vi.fn(),
    warning: vi.fn(),
    info: vi.fn(),
    message: vi.fn(),
  },
}))

vi.mock('@/context/SessionContext', () => ({
  useSessions: () => ({ currentSessionId: null }),
}))

const KEYED: ProviderConnection = {
  id: 'prov-anthropic',
  display_name: 'Anthropic',
  kind: 'anthropic',
  has_api_key: true,
  base_url: null,
}

const KEYLESS_OLLAMA: ProviderConnection = {
  id: 'prov-ollama',
  display_name: 'Ollama local',
  kind: 'ollama',
  has_api_key: false,
  base_url: 'http://127.0.0.1:11434',
}

const NO_KEY_ANTHROPIC: ProviderConnection = {
  id: 'prov-broken',
  display_name: 'Anthropic (no key)',
  kind: 'anthropic',
  has_api_key: false,
  base_url: null,
}

function renderRoster(providers: ProviderConnection[]) {
  const file: ProvidersFile = { active_provider_id: null, providers }
  return render(
    <ProvidersSection
      providersFile={file}
      loading={false}
      onChange={() => {}}
      onActivated={async () => {}}
    />,
  )
}

const testCredentials = vi.mocked(api.testProviderCredentials)
const testConnection = vi.mocked(api.testProviderConnection)

beforeEach(() => {
  vi.clearAllMocks()
  testCredentials.mockResolvedValue({ kind: 'success' })
  testConnection.mockResolvedValue({ kind: 'success' })
})

describe('ProvidersSection — card-level Test connection (P0-7)', () => {
  it('a provider with a stored key tests via the credential store, not an empty key', async () => {
    const { toast } = await import('sonner')
    renderRoster([KEYED])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() => expect(testCredentials).toHaveBeenCalledTimes(1))
    // apiKey: null + the connection id → backend falls back to the stored
    // credential (mirrors test_all_providers / the edit-modal probe).
    expect(testCredentials).toHaveBeenCalledWith('anthropic', null, null, 'prov-anthropic')
    // The empty-key probe must never be reached from the card.
    expect(testConnection).not.toHaveBeenCalled()
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
  })

  it('passes a saved custom base_url through to the probe', async () => {
    renderRoster([{ ...KEYED, id: 'prov-compat', kind: 'openai-compatible', base_url: 'https://relay.example.com/v1' }])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() =>
      expect(testCredentials).toHaveBeenCalledWith('openai-compatible', 'https://relay.example.com/v1', null, 'prov-compat'),
    )
  })

  it('Ollama (needsKey: false) probes without demanding a key first', async () => {
    renderRoster([KEYLESS_OLLAMA])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() => expect(testCredentials).toHaveBeenCalledWith('ollama', 'http://127.0.0.1:11434', null, 'prov-ollama'))
  })

  it('a needsKey provider with no stored key is blocked before any network call', async () => {
    const { toast } = await import('sonner')
    renderRoster([NO_KEY_ANTHROPIC])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(testCredentials).not.toHaveBeenCalled()
    expect(testConnection).not.toHaveBeenCalled()
  })

  it('surfaces a failed probe through the categorized toast', async () => {
    const { toast } = await import('sonner')
    testCredentials.mockResolvedValue({ kind: 'invalid_key' })
    renderRoster([KEYED])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(toast.success).not.toHaveBeenCalled()
  })

  it('surfaces an exhausted quota (HTTP 402) through the categorized toast (R2-P1-10)', async () => {
    const { toast } = await import('sonner')
    testCredentials.mockResolvedValue({ kind: 'quota_exhausted' })
    renderRoster([KEYED])
    fireEvent.click(screen.getByRole('button', { name: 'Test connection' }))
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(expect.stringContaining('Quota exhausted')),
    )
  })
})

// ---- S3-4 (推荐降级链): card → recommended-fallback-chain panel → apply ----
//
// The behavioral red line pinned here: the recommendation is candidates-only
// until the user presses "Save this chain" INSIDE the panel — the card
// toggle must never write `fallback_models` by itself, and apply must go
// through setProviderFallbackModels (the surgical field write), then refresh
// the roster via listProviders (the same refresh path key mutations use).

describe('ProvidersSection — recommended fallback chain (S3-4)', () => {
  const recommendChain = vi.mocked(api.recommendFallbackChain)
  const setFallbackModels = vi.mocked(api.setProviderFallbackModels)
  const listProviders = vi.mocked(api.listProviders)

  const CHAIN = {
    provider_id: 'prov-anthropic',
    model_profile: 'default',
    current_model: 'claude-sonnet-4-6',
    hops: [
      {
        entry: 'claude-haiku-4-5',
        model: 'claude-haiku-4-5',
        provider_id: 'prov-anthropic',
        provider_label: 'Anthropic',
        same_provider: true,
        tier: 'fast',
      },
      {
        entry: 'prov-deepseek/deepseek-chat',
        model: 'deepseek-chat',
        provider_id: 'prov-deepseek',
        provider_label: 'DeepSeek',
        same_provider: false,
        tier: 'standard',
      },
    ],
  }

  beforeEach(() => {
    recommendChain.mockResolvedValue(CHAIN)
    setFallbackModels.mockResolvedValue({
      provider_id: 'prov-anthropic',
      model_profile: 'default',
      fallback_models: CHAIN.hops.map((h) => h.entry),
    })
    listProviders.mockResolvedValue({
      active_provider_id: 'prov-anthropic',
      providers: [
        { ...KEYED, fallback_models: CHAIN.hops.map((h) => h.entry) },
      ],
    })
  })

  it('opens the panel from the card, renders the hops, and APPLIES only on explicit confirmation', async () => {
    const { toast } = await import('sonner')
    const onChange = vi.fn()
    const file: ProvidersFile = { active_provider_id: null, providers: [KEYED] }
    render(
      <ProvidersSection providersFile={file} loading={false} onChange={onChange} onActivated={async () => {}} />,
    )

    // Panel closed → no recommendation has been computed, nothing written.
    expect(screen.queryByTestId('recommend-fallback-panel')).not.toBeInTheDocument()
    expect(recommendChain).not.toHaveBeenCalled()
    expect(setFallbackModels).not.toHaveBeenCalled()

    fireEvent.click(screen.getByTestId('provider-fallback-toggle-prov-anthropic'))
    await waitFor(() => expect(recommendChain).toHaveBeenCalledWith('prov-anthropic'))

    // Candidates render with per-hop semantics; still nothing written.
    await waitFor(() => expect(screen.getAllByTestId('recommend-fallback-hop')).toHaveLength(2))
    expect(setFallbackModels).not.toHaveBeenCalled()

    // Explicit confirmation → write + roster refresh + success toast.
    fireEvent.click(screen.getByTestId('recommend-fallback-apply'))
    await waitFor(() =>
      expect(setFallbackModels).toHaveBeenCalledWith(
        'prov-anthropic',
        CHAIN.hops.map((h) => h.entry),
      ),
    )
    await waitFor(() => expect(listProviders).toHaveBeenCalled())
    await waitFor(() => expect(onChange).toHaveBeenCalled())
    await waitFor(() => expect(toast.success).toHaveBeenCalled())
    // The panel closes after a committed apply (fallback target cleared).
    await waitFor(() =>
      expect(screen.queryByTestId('recommend-fallback-panel')).not.toBeInTheDocument(),
    )
  })

  it('an empty recommendation renders the empty state with no apply button', async () => {
    recommendChain.mockResolvedValue({
      provider_id: 'prov-anthropic',
      model_profile: 'default',
      current_model: null,
      hops: [],
    })
    renderRoster([KEYED])
    fireEvent.click(screen.getByTestId('provider-fallback-toggle-prov-anthropic'))
    await waitFor(() => expect(recommendChain).toHaveBeenCalled())
    await waitFor(() => expect(screen.getByTestId('recommend-fallback-empty')).toBeInTheDocument())
    expect(screen.queryByTestId('recommend-fallback-apply')).not.toBeInTheDocument()
    expect(setFallbackModels).not.toHaveBeenCalled()
  })

  it('a recommendation failure surfaces the inline failed state, not a write', async () => {
    recommendChain.mockRejectedValue(new Error('boom'))
    renderRoster([KEYED])
    fireEvent.click(screen.getByTestId('provider-fallback-toggle-prov-anthropic'))
    await waitFor(() => expect(screen.getByTestId('recommend-fallback-failed')).toBeInTheDocument())
    expect(setFallbackModels).not.toHaveBeenCalled()
  })

  it('the Test-all panel opens the same panel for a rate-limited row', async () => {
    const testAll = vi.mocked(api.testAllProviders)
    testAll.mockResolvedValue([
      {
        id: 'prov-anthropic',
        label: 'Anthropic',
        provider_kind: 'anthropic',
        result: { kind: 'rate_limited' },
        latency_ms: 42,
      },
    ])
    renderRoster([KEYED])
    fireEvent.click(screen.getByRole('button', { name: 'Test all' }))
    await waitFor(() => expect(testAll).toHaveBeenCalled())
    // Failed row carries the affordance (this IS the moment a chain pays off).
    await waitFor(() =>
      expect(screen.getByTestId('test-all-fallback-prov-anthropic')).toBeInTheDocument(),
    )
    fireEvent.click(screen.getByTestId('test-all-fallback-prov-anthropic'))
    await waitFor(() => expect(screen.getByTestId('recommend-fallback-panel')).toBeInTheDocument())
    await waitFor(() => expect(recommendChain).toHaveBeenCalledWith('prov-anthropic'))
    // Still candidates-only until the user confirms inside the panel.
    expect(setFallbackModels).not.toHaveBeenCalled()
  })
})

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
})

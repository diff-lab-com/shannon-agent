// ApiKeyBanner (2026-09-29 provider review §3-A1) — zero-coverage until R2.
//
// Two halves:
//  1. `shouldShowApiKeyBanner` — the four-quadrant gate over the reliable
//     get_provider_status snapshot (Chat.tsx): no-provider / no-key /
//     env-provider fallback / keyless Ollama, plus the never-nag cases
//     (null status, configured users).
//  2. The banner itself — hidden when not visible, both variants render
//     their copy (the no-key variant names the provider), dismiss and the
//     Settings CTA call back.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'

import ApiKeyBanner, { type ApiKeyBannerVariant } from '@/pages/chat/ApiKeyBanner'
import { shouldShowApiKeyBanner } from '@/pages/Chat'
import type { ProviderStatus } from '@/types'

function status(patch: Partial<ProviderStatus>): ProviderStatus {
  return {
    active_provider_id: 'prov-anthropic',
    display_name: 'Anthropic',
    kind: 'anthropic',
    has_api_key: true,
    model: null,
    env_provider: null,
    ...patch,
  } as ProviderStatus
}

describe('shouldShowApiKeyBanner — the four quadrants', () => {
  it.each([
    ['null status (read failed / loading) never nags', null, false],
    ['no active provider and no env fallback → show', status({ active_provider_id: null, env_provider: null }), true],
    ['no active provider but env-detected one → hide', status({ active_provider_id: null, env_provider: 'anthropic' }), false],
    ['active provider with missing key → show', status({ has_api_key: false }), true],
    ['keyless Ollama needs no key → hide', status({ has_api_key: false, kind: 'ollama' }), false],
    ['configured + keyed users → hide', status({}), false],
  ])('%s', (_label, input, expected) => {
    expect(shouldShowApiKeyBanner(input)).toBe(expected)
  })
})

describe('ApiKeyBanner rendering', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders nothing while not visible', () => {
    const { container } = render(
      <ApiKeyBanner visible={false} onDismiss={() => {}} onOpenSettings={() => {}} />,
    )
    expect(container.querySelector('.shannon-apikey-banner')).toBeNull()
  })

  it('renders the generic no-provider variant with the Settings CTA and dismiss', () => {
    const onDismiss = vi.fn()
    const onOpenSettings = vi.fn()
    render(
      <ApiKeyBanner visible variant={'no-provider' as ApiKeyBannerVariant} providerName={undefined} onDismiss={onDismiss} onOpenSettings={onOpenSettings} />,
    )
    expect(screen.getByText('Add your API key to start chatting')).toBeInTheDocument()
    expect(screen.getByText('Shannon needs a provider API key to send messages. Add one in Settings → Models.')).toBeInTheDocument()
    const cta = screen.getByRole('button', { name: 'Open Settings' })
    fireEvent.click(cta)
    expect(onOpenSettings).toHaveBeenCalledTimes(1)
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('renders the no-key variant naming the provider', () => {
    render(
      <ApiKeyBanner visible variant={'no-key' as ApiKeyBannerVariant} providerName="Anthropic" onDismiss={() => {}} onOpenSettings={() => {}} />,
    )
    expect(screen.getByText('API key missing for Anthropic')).toBeInTheDocument()
    expect(screen.getByText('Add a key for Anthropic in Settings → Models to keep chatting.')).toBeInTheDocument()
  })
})

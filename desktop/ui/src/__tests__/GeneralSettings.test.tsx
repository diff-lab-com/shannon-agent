import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import GeneralSettings from '@/components/settings/GeneralSettings'
import { WELCOME_SEEN_KEY } from '@/pages/Welcome'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>
          {/* Batch D4: GeneralSettings consumes the app-level artifact
              context (auto-open toggle). */}
          <ArtifactProvider>
            {ui}
          </ArtifactProvider>
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('GeneralSettings', () => {
  it('renders general settings subtitle', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText(/Refine your AI workflow and interface preferences/)).toBeInTheDocument()
  })

  it('renders approval mode section', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Approval Mode')).toBeInTheDocument()
  })

  it('renders the four shared approval tiers (round-1 R3: strict/balanced/permissive/full)', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getAllByText('Strict').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Balanced').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Permissive').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Full').length).toBeGreaterThanOrEqual(1)
    // R3: the no-op suggest/confirm pair left the table — the settings page
    // can no longer offer two tiers that behave identically.
    expect(screen.queryByText('Confirm')).not.toBeInTheDocument()
  })

  // Round-2 review: the factory-default `approval_mode: "confirm"` is an
  // out-of-table engine value (R3) — it must show the RAW value with NO tier
  // selected, never masquerade as a pickable tier (the old index-based read
  // fell back to Permissive, looser than the engine's actual ask-every-time).
  const configWith = (approvalMode: string) => ({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    api_key: 'sk-test',
    working_dir: '/tmp',
    approval_mode: approvalMode,
  })

  it('round-2: factory-default "confirm" selects NO tier and reads out the raw value', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('confirm'))
    render(wrap(<GeneralSettings />))
    // every radio unchecked — no tier may claim the confirm value
    await waitFor(() => {
      const radios = screen.getAllByRole('radio')
      expect(radios).toHaveLength(4)
      for (const radio of radios) expect(radio).toHaveAttribute('aria-checked', 'false')
    })
    // the raw engine value is named, with the honest switch hint
    expect(screen.getByTestId('approval-mode-raw-hint')).toHaveTextContent('confirm')
    expect(screen.getByTestId('approval-mode-raw-hint')).toHaveTextContent(/engine-managed value/i)
  })

  it('round-2: an in-table config still selects its tier (suggest → Balanced)', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('suggest'))
    render(wrap(<GeneralSettings />))
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Balanced' })).toHaveAttribute('aria-checked', 'true'))
    expect(screen.getByRole('radio', { name: 'Strict' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: 'Permissive' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('renders provider section', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Provider')).toBeInTheDocument()
  })

  it('renders working directory label', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Working Directory')).toBeInTheDocument()
  })

  it('shows current mode description', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText(/Current:/)).toBeInTheDocument()
  })

  describe('Re-run setup wizard', () => {
    beforeEach(() => {
      window.localStorage.clear()
      window.localStorage.setItem(WELCOME_SEEN_KEY, '1')
    })

    it('renders the re-run wizard section', () => {
      render(wrap(<GeneralSettings />))
      expect(screen.getByRole('heading', { name: 'Setup wizard' })).toBeInTheDocument()
      expect(screen.getByRole('button', { name: 'Re-run setup wizard' })).toBeInTheDocument()
    })

    it('clears the seen flag on click', () => {
      render(wrap(<GeneralSettings />))
      expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
      fireEvent.click(screen.getByRole('button', { name: 'Re-run setup wizard' }))
      expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBeNull()
    })
  })
})

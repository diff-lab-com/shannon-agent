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

  it('renders the three shared ladder tiers (4+3: ask/auto-edit/full-auto)', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getAllByText('Ask').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Auto Edit').length).toBeGreaterThanOrEqual(1)
    expect(screen.getAllByText('Full').length).toBeGreaterThanOrEqual(1)
    // The expert tiers (Strict et al.) live behind the Advanced picker, not
    // the quick segmented control.
    expect(screen.queryByRole('radio', { name: 'Strict' })).not.toBeInTheDocument()
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

  it('legacy factory-default "confirm" normalizes into the Ask tier', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('confirm'))
    render(wrap(<GeneralSettings />))
    // Legacy values normalize for display: confirm ≡ ask, and the ask tier
    // radio is the one that selects.
    await waitFor(() => {
      const radios = screen.getAllByRole('radio')
      expect(radios).toHaveLength(3)
      expect(screen.getByRole('radio', { name: 'Ask' })).toHaveAttribute('aria-checked', 'true')
      for (const radio of radios) {
        if (radio !== screen.getByRole('radio', { name: 'Ask' })) {
          expect(radio).toHaveAttribute('aria-checked', 'false')
        }
      }
    })
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('an in-table config still selects its tier (auto_edit → Auto Edit)', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('auto_edit'))
    render(wrap(<GeneralSettings />))
    await waitFor(() => expect(screen.getByRole('radio', { name: 'Auto Edit' })).toHaveAttribute('aria-checked', 'true'))
    expect(screen.getByRole('radio', { name: 'Ask' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.getByRole('radio', { name: 'Full' })).toHaveAttribute('aria-checked', 'false')
    expect(screen.queryByTestId('approval-mode-raw-hint')).not.toBeInTheDocument()
  })

  it('an expert config (bypass_permissions) selects no quick tier and names itself', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('bypass_permissions'))
    render(wrap(<GeneralSettings />))
    // Expert modes are not quick tiers: no radio selects, but the current
    // mode line still states the truth (Bypass approvals).
    await waitFor(() => {
      for (const radio of screen.getAllByRole('radio')) {
        expect(radio).toHaveAttribute('aria-checked', 'false')
      }
    })
    expect(screen.getByText(/Current:/)).toHaveTextContent(/bypass/i)
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

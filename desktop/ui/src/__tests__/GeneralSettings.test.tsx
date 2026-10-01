import { describe, it, expect, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
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

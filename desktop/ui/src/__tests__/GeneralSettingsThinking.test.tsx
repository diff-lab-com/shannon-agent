// Settings R3 T9 — the 「显示思考过程」 card in GeneralSettings: a three-tier
// segmented control (radiogroup, styled after the approval-mode one) that
// persists `shannon.chat.showThinking` instantly (localStorage, no
// backend round-trip — hence the instant EffectBadge).

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import GeneralSettings from '@/components/settings/GeneralSettings'
import { SHOW_THINKING_PREF_KEY } from '@/lib/thinkingPref'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>
          <ArtifactProvider>{ui}</ArtifactProvider>
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

const radio = (tier: 'all' | 'first' | 'none') =>
  screen.getByTestId(`settings-show-thinking-${tier}`)

describe('GeneralSettings show-thinking control (Settings R3 T9)', () => {
  beforeEach(() => {
    window.localStorage.clear()
    vi.mocked(api.getConfig).mockResolvedValue({
      provider: 'anthropic',
      model: 'claude-sonnet-4-6',
      api_key: 'sk-test',
      working_dir: '/tmp',
      approval_mode: 'normal',
    })
    vi.mocked(api.getPowerCapabilities).mockResolvedValue({
      platform: 'linux',
      keepAwakeSupported: true,
    })
  })

  it('renders the card with three tiers and selects "All" on first run', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByRole('heading', { name: 'Show thinking' })).toBeInTheDocument()
    // Instant-effect badge (the keep-awake card carries one too, hence getAll).
    expect(screen.getAllByText('Instant effect').length).toBeGreaterThan(0)
    expect(radio('all')).toHaveAttribute('aria-checked', 'true')
    expect(radio('first')).toHaveAttribute('aria-checked', 'false')
    expect(radio('none')).toHaveAttribute('aria-checked', 'false')
    // First run picks nothing — no key written until the user chooses.
    expect(window.localStorage.getItem(SHOW_THINKING_PREF_KEY)).toBeNull()
  })

  it('selecting "First only" persists the pref and moves the selection', () => {
    render(wrap(<GeneralSettings />))
    fireEvent.click(radio('first'))
    expect(window.localStorage.getItem(SHOW_THINKING_PREF_KEY)).toBe('first')
    expect(radio('first')).toHaveAttribute('aria-checked', 'true')
    expect(radio('all')).toHaveAttribute('aria-checked', 'false')
  })

  it('selecting "Off" persists "none"', () => {
    render(wrap(<GeneralSettings />))
    fireEvent.click(radio('none'))
    expect(window.localStorage.getItem(SHOW_THINKING_PREF_KEY)).toBe('none')
    expect(radio('none')).toHaveAttribute('aria-checked', 'true')
  })

  it('a stored tier preselects its segment', () => {
    window.localStorage.setItem(SHOW_THINKING_PREF_KEY, 'first')
    render(wrap(<GeneralSettings />))
    expect(radio('first')).toHaveAttribute('aria-checked', 'true')
    expect(radio('all')).toHaveAttribute('aria-checked', 'false')
  })
})

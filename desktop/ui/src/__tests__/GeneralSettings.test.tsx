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

  // Task 2 (settings-parity R3): the language control is a select whose
  // first option is the explicit "follow system" pref. The pref persists
  // verbatim — 'system' for follow, the locale id for a pinned language —
  // so both survive restarts and read back exactly as picked.
  describe('Language select (follow system)', () => {
    const LANGUAGE_KEY = 'shannon.locale'

    beforeEach(() => {
      window.localStorage.clear()
    })

    const languageSelect = () =>
      // Stable testid, not the accessible name: picking a locale switches
      // the whole UI (aria-label included) to that language mid-test.
      screen.getByTestId('settings-language-select') as HTMLSelectElement

    it('renders a select with the system option first plus all supported locales', () => {
      render(wrap(<GeneralSettings />))
      // A11y contract while the UI is still in English: exposed as a
      // combobox named after the Language section label.
      expect(screen.getByRole('combobox', { name: 'Language' })).toBe(languageSelect())
      const values = [...languageSelect().options].map(o => o.value)
      expect(values[0]).toBe('system')
      expect(values).toEqual(expect.arrayContaining([
        'en', 'zh-CN', 'zh-TW', 'ja', 'ko', 'es', 'fr', 'de', 'pt-BR', 'ru',
      ]))
      expect(values).toHaveLength(11)
    })

    it('first run (no stored pref) shows the follow-system option', () => {
      render(wrap(<GeneralSettings />))
      expect(languageSelect()).toHaveValue('system')
      // And picking nothing writes nothing — first-run behavior is unchanged.
      expect(window.localStorage.getItem(LANGUAGE_KEY)).toBeNull()
    })

    it('selecting system persists the literal "system" pref', () => {
      render(wrap(<GeneralSettings />))
      const select = languageSelect()
      fireEvent.change(select, { target: { value: 'system' } })
      expect(window.localStorage.getItem(LANGUAGE_KEY)).toBe('system')
      // Element identity survives the locale re-render — assert on the
      // captured node, not a fresh (name-dependent) query.
      expect(select).toHaveValue('system')
    })

    it('selecting a concrete locale persists its id', () => {
      render(wrap(<GeneralSettings />))
      const select = languageSelect()
      fireEvent.change(select, { target: { value: 'ja' } })
      expect(window.localStorage.getItem(LANGUAGE_KEY)).toBe('ja')
      expect(select).toHaveValue('ja')
    })

    it('a stored concrete locale selects that option, a stored system pref selects system', () => {
      window.localStorage.setItem(LANGUAGE_KEY, 'de')
      const { unmount } = render(wrap(<GeneralSettings />))
      expect(languageSelect()).toHaveValue('de')
      unmount()

      window.localStorage.setItem(LANGUAGE_KEY, 'system')
      render(wrap(<GeneralSettings />))
      expect(languageSelect()).toHaveValue('system')
    })
  })

  // Settings R3 T3 — the System group: hardware-acceleration escape hatch
  // (hidden on macOS, restart-app semantics) + the two prevent-sleep
  // switches (instant semantics, disabled with a note when the platform
  // has no backend).
  describe('System group (hardware acceleration + prevent sleep)', () => {
    const baseConfig = {
      provider: 'anthropic',
      model: 'claude-sonnet-4-6',
      api_key: 'sk-test',
      working_dir: '/tmp',
      approval_mode: 'normal',
    }

    beforeEach(() => {
      vi.mocked(api.getPowerCapabilities).mockResolvedValue({
        platform: 'linux',
        keepAwakeSupported: true,
      })
    })

    it('renders the System group with both cards on a supported non-macos platform', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
      render(wrap(<GeneralSettings />))
      await waitFor(() => {
        expect(screen.getByTestId('settings-hwaccel-switch')).toBeInTheDocument()
        expect(screen.getByTestId('settings-keepawake-task-switch')).toBeInTheDocument()
        expect(screen.getByTestId('settings-keepawake-always-switch')).toBeInTheDocument()
      })
      expect(screen.getByText('Hardware acceleration')).toBeInTheDocument()
      expect(screen.getByText('Prevent sleep')).toBeInTheDocument()
    })

    it('hides the hardware acceleration card on macOS', async () => {
      vi.mocked(api.getPowerCapabilities).mockResolvedValue({
        platform: 'macos',
        keepAwakeSupported: true,
      })
      render(wrap(<GeneralSettings />))
      // The keep-awake card still renders (macOS has caffeinate).
      await waitFor(() => {
        expect(screen.getByTestId('settings-keepawake-task-switch')).toBeInTheDocument()
      })
      expect(screen.queryByTestId('settings-hwaccel-switch')).not.toBeInTheDocument()
      expect(screen.queryByText('Hardware acceleration')).not.toBeInTheDocument()
    })

    it('toggling hardware acceleration writes the config key (restart-app semantics badge shown)', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig, hardware_acceleration: true })
      render(wrap(<GeneralSettings />))
      const hwSwitch = await screen.findByTestId('settings-hwaccel-switch')
      expect(hwSwitch).not.toBeDisabled()
      fireEvent.click(hwSwitch)
      await waitFor(() => {
        expect(api.configure).toHaveBeenCalledWith({ key: 'hardware_acceleration', value: 'false' })
      })
      expect(screen.getByText('Restart required')).toBeInTheDocument()
    })

    it('hardware acceleration defaults on and reads the persisted off state', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig, hardware_acceleration: false })
      render(wrap(<GeneralSettings />))
      await waitFor(() => {
        expect(screen.getByTestId('settings-hwaccel-switch')).toHaveAttribute('aria-checked', 'false')
      })
    })

    it('task-run sleep blocker defaults on and writes power.block_sleep_during_tasks', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
      render(wrap(<GeneralSettings />))
      const taskSwitch = await screen.findByTestId('settings-keepawake-task-switch')
      // Default on.
      expect(taskSwitch).toHaveAttribute('aria-checked', 'true')
      fireEvent.click(taskSwitch)
      await waitFor(() => {
        expect(api.configure).toHaveBeenCalledWith({
          key: 'power.block_sleep_during_tasks',
          value: 'false',
        })
      })
    })

    it('always-on wake lock defaults off and writes power.keep_awake', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
      render(wrap(<GeneralSettings />))
      const alwaysSwitch = await screen.findByTestId('settings-keepawake-always-switch')
      // Default off.
      expect(alwaysSwitch).toHaveAttribute('aria-checked', 'false')
      fireEvent.click(alwaysSwitch)
      await waitFor(() => {
        expect(api.configure).toHaveBeenCalledWith({ key: 'power.keep_awake', value: 'true' })
      })
    })

    it('disables the prevent-sleep switches and shows the unsupported note when the platform has no backend', async () => {
      vi.mocked(api.getPowerCapabilities).mockResolvedValue({
        platform: 'linux',
        keepAwakeSupported: false,
      })
      vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
      render(wrap(<GeneralSettings />))
      const note = await screen.findByTestId('settings-keepawake-unsupported')
      expect(note).toHaveTextContent('Sleep prevention is not available on this platform.')
      // Base UI Switch renders aria-disabled on its <span role="switch">
      // wrapper rather than the native disabled attribute (same convention
      // as NotificationsSettings/ConnectionsSettings tests).
      await waitFor(() => {
        expect(screen.getByTestId('settings-keepawake-task-switch')).toHaveAttribute('aria-disabled', 'true')
        expect(screen.getByTestId('settings-keepawake-always-switch')).toHaveAttribute('aria-disabled', 'true')
      })
    })
  })
})

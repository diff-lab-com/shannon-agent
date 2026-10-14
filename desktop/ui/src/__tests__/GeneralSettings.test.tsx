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

  it('renders the approval link card aligned with the Permissions pane title', () => {
    render(wrap(<GeneralSettings />))
    // R3 wording audit: the link card title mirrors PermissionsPane's
    // 默认执行模式 entity card — one config, one name.
    expect(screen.getByText('Default execution mode')).toBeInTheDocument()
  })

  // design-parity R2 (Wave B′): the 默认执行模式 entity card (four-stop
  // segmented ladder + expert picker, same approval_mode key as the
  // composer) LIVES in 权限与安全 (PermissionsPaneExecutionMode.test.tsx
  // pins it there). General keeps only the short link card — and the
  // honest raw-value read the Round-2 review introduced.
  it('carries no approval radios — the control moved to PermissionsPane', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByTestId('approval-mode-link-card')).toBeInTheDocument()
    expect(screen.queryByRole('radio', { name: 'Ask' })).not.toBeInTheDocument()
    expect(screen.queryByRole('radio', { name: 'Strict' })).not.toBeInTheDocument()
    expect(document.getElementById('approval-mode-advanced')).toBeNull()
    expect(screen.getByTestId('general-to-permissions-link')).toBeInTheDocument()
  })

  const configWith = (approvalMode: string) => ({
    provider: 'anthropic',
    model: 'claude-sonnet-4-6',
    api_key: 'sk-test',
    working_dir: '/tmp',
    approval_mode: approvalMode,
  })

  it('legacy factory-default "confirm" names its resolved tier in the link card', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('confirm'))
    render(wrap(<GeneralSettings />))
    // confirm is a legacy alias: normalizeApprovalMode resolves it to the
    // ask tier, so the one-liner shows Ask (never the raw engine value).
    await waitFor(() =>
      expect(screen.getByTestId('approval-mode-link-card')).toHaveTextContent('Current: Ask'),
    )
  })

  it('an in-table config names its tier (auto_edit → Auto Edit)', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('auto_edit'))
    render(wrap(<GeneralSettings />))
    await waitFor(() =>
      expect(screen.getByTestId('approval-mode-link-card')).toHaveTextContent('Current: Auto Edit'),
    )
  })

  it('an expert config (bypass_permissions) names its resolved tier in the link card', async () => {
    vi.mocked(api.getConfig).mockResolvedValueOnce(configWith('bypass_permissions'))
    render(wrap(<GeneralSettings />))
    // bypass_permissions normalizes to the bypassPermissions expert mode;
    // its label travels verbatim in the Current readout.
    await waitFor(() =>
      expect(screen.getByTestId('approval-mode-link-card')).toHaveTextContent('Current: Bypass approvals'),
    )
  })

  it('renders provider section', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Provider')).toBeInTheDocument()
  })

  it('renders working directory label', () => {
    render(wrap(<GeneralSettings />))
    expect(screen.getByText('Working Directory')).toBeInTheDocument()
  })

  // design-parity R2 (M3a, design 12-settings-general.html:212-217): the
  // 默认工作目录 row became WRITABLE — input + Browse + Save over the same
  // configure('working_dir') key the engine resolves sessions from. The
  // write is gated by a get_file_tree probe (the only dir-existence probe
  // on the IPC surface): a miss renders the inline 目录不存在 error and
  // persists NOTHING.
  describe('Default working directory (writable)', () => {
    it('renders the input prefilled from the config, save disabled while unchanged', async () => {
      vi.mocked(api.getConfig).mockResolvedValue(configWith('/tmp'))
      render(wrap(<GeneralSettings />))
      const input = await screen.findByTestId('settings-working-dir-input') as HTMLInputElement
      await waitFor(() => expect(input).toHaveValue('/tmp'))
      expect(screen.getByTestId('settings-working-dir-save')).toBeDisabled()
      expect(screen.queryByTestId('settings-working-dir-error')).not.toBeInTheDocument()
    })

    it('save is disabled for a blank draft', async () => {
      vi.mocked(api.getConfig).mockResolvedValue(configWith('/tmp'))
      render(wrap(<GeneralSettings />))
      const input = await screen.findByTestId('settings-working-dir-input') as HTMLInputElement
      await waitFor(() => expect(input).toHaveValue('/tmp'))
      fireEvent.change(input, { target: { value: '   ' } })
      expect(screen.getByTestId('settings-working-dir-save')).toBeDisabled()
      expect(api.getFileTree).not.toHaveBeenCalled()
    })

    it('a valid path probes get_file_tree, then writes configure(working_dir)', async () => {
      vi.mocked(api.getConfig).mockResolvedValue(configWith('/tmp'))
      vi.mocked(api.getFileTree).mockResolvedValue([{ name: 'proj', path: '/home/me/proj', is_dir: true, children: [] }])
      render(wrap(<GeneralSettings />))
      const input = await screen.findByTestId('settings-working-dir-input') as HTMLInputElement
      await waitFor(() => expect(input).toHaveValue('/tmp'))
      fireEvent.change(input, { target: { value: '/home/me/proj' } })
      fireEvent.click(screen.getByTestId('settings-working-dir-save'))
      await waitFor(() => {
        expect(api.configure).toHaveBeenCalledWith({ key: 'working_dir', value: '/home/me/proj' })
      })
      expect(screen.queryByTestId('settings-working-dir-error')).not.toBeInTheDocument()
    })

    it('a missing path shows the inline error and persists nothing', async () => {
      vi.mocked(api.getConfig).mockResolvedValue(configWith('/tmp'))
      vi.mocked(api.getFileTree).mockRejectedValue('Path is not a directory')
      render(wrap(<GeneralSettings />))
      const input = await screen.findByTestId('settings-working-dir-input') as HTMLInputElement
      await waitFor(() => expect(input).toHaveValue('/tmp'))
      fireEvent.change(input, { target: { value: '/home/me/does-not-exist' } })
      fireEvent.click(screen.getByTestId('settings-working-dir-save'))
      const err = await screen.findByTestId('settings-working-dir-error')
      expect(err).toHaveTextContent('Directory does not exist or is not accessible')
      expect(err).toHaveAttribute('role', 'alert')
      expect(api.configure).not.toHaveBeenCalledWith({ key: 'working_dir', value: '/home/me/does-not-exist' })
    })

    it('Browse fills the draft from the OS directory picker without writing', async () => {
      vi.mocked(api.getConfig).mockResolvedValue(configWith('/tmp'))
      const { open } = await import('@tauri-apps/plugin-dialog')
      vi.mocked(open).mockResolvedValueOnce('/picked/dir')
      render(wrap(<GeneralSettings />))
      const input = await screen.findByTestId('settings-working-dir-input') as HTMLInputElement
      await waitFor(() => expect(input).toHaveValue('/tmp'))
      fireEvent.click(screen.getByTestId('settings-working-dir-browse'))
      await waitFor(() => expect(input).toHaveValue('/picked/dir'))
      // The picker only fills the draft — persisting is the explicit Save.
      expect(api.configure).not.toHaveBeenCalledWith({ key: 'working_dir', value: '/picked/dir' })
    })
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

  // D5 方案① — 主动任务推荐 (suggestions) presentation toggle. A pure
  // configure round-trip like the System group: the backend never gates
  // anything on the key; the UI reads it live for the completion chips and
  // the welcome card's refresh/filter.
  describe('D5 suggestions toggle', () => {
    const cfg = {
      provider: 'anthropic',
      model: 'claude-sonnet-4-6',
      api_key: 'sk-test',
      working_dir: '/tmp',
      approval_mode: 'normal',
    }
    it('defaults to ON when the config omits suggestions_enabled', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...cfg })
      render(wrap(<GeneralSettings />))
      const sw = await screen.findByTestId('settings-suggestions-switch')
      expect(sw).toHaveAttribute('aria-checked', 'true')
    })

    it('hydrates from the persisted config (off stays off)', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...cfg, suggestions_enabled: false })
      render(wrap(<GeneralSettings />))
      const sw = await screen.findByTestId('settings-suggestions-switch')
      await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'false'))
    })

    it('persists suggestions.enabled through configure when toggled', async () => {
      vi.mocked(api.getConfig).mockResolvedValue({ ...cfg })
      render(wrap(<GeneralSettings />))
      const sw = await screen.findByTestId('settings-suggestions-switch')
      await waitFor(() => expect(sw).toHaveAttribute('aria-checked', 'true'))
      fireEvent.click(sw)
      await waitFor(() => {
        expect(api.configure).toHaveBeenCalledWith({ key: 'suggestions.enabled', value: 'false' })
      })
    })
  })
})

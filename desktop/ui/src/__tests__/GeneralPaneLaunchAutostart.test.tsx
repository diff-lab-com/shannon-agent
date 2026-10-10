// 缓期批 3 — the 启动 card's 开机自启 toggle (Settings → 通用 → 启动).
//
// Pins the honesty contract that makes this toggle different from its two
// card siblings: the switch state comes from `get_launch_on_login` (the OS
// login-item registration, which the user can also flip in the system
// settings app) — NOT from `get_config().launch_on_login` (remembered
// intent only). Covers:
//   - on/off render from the OS truth;
//   - the switch staying disabled until the OS probe answers;
//   - the config-fallback shape when the OS query fails (the same fallback
//     the backend's honest read uses when the plugin state is absent);
//   - the optimistic write through configure('launch_on_login');
//   - the revert-on-refusal: the backend applies the OS change FIRST and
//     rejects when the registration is refused → the switch reverts and the
//     error copy names the refusal.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { AppProvider } from '@/context/AppContext'
import { ArtifactProvider } from '@/components/artifact/ArtifactContext'
import GeneralPane from '@/pages/settings/GeneralPane'
import * as api from '@/lib/tauri-api'
import { toast } from 'sonner'

vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() },
}))

vi.mock('@/lib/tauri-api', async (importOriginal) => {
  const actual = await importOriginal<object>()
  return {
    ...actual,
    getLaunchOnLogin: vi.fn(),
    configure: vi.fn().mockResolvedValue(undefined),
    // The CatalogContext slice — controlled so the config-fallback test can
    // arm a persisted `launch_on_login` intent value.
    getConfig: vi.fn().mockResolvedValue({}),
    // GeneralSettings' feedback card reads the real list command; the core
    // invoke mock resolves undefined and the card expects an array.
    listFeedbackSessions: vi.fn().mockResolvedValue([]),
  }
})

function wrap(ui: React.ReactElement) {
  // Same harness as GeneralPaneDataExport.test.tsx — GeneralPane (and the
  // GeneralSettings/SessionSettings it stacks) read the CatalogContext.
  return (
    <AppProvider>
      <MemoryRouter>
        <ArtifactProvider>{ui}</ArtifactProvider>
      </MemoryRouter>
    </AppProvider>
  )
}

const getLaunchOnLogin = vi.mocked(api.getLaunchOnLogin)
const configure = vi.mocked(api.configure)
const getConfig = vi.mocked(api.getConfig)

function switchEl() {
  return screen.getByTestId('general-launch-autostart-switch')
}

beforeEach(() => {
  vi.mocked(toast.success).mockClear()
  vi.mocked(toast.error).mockClear()
  getLaunchOnLogin.mockReset()
  configure.mockReset().mockResolvedValue(undefined)
  getConfig.mockReset().mockResolvedValue({} as never)
})

describe('GeneralPane 启动 card — 开机自启 (缓期批 3)', () => {
  it('renders OFF from the OS truth (get_launch_on_login, not the config field)', async () => {
    getLaunchOnLogin.mockResolvedValue(false)
    render(wrap(<GeneralPane />))
    const el = await waitFor(() => {
      const el = switchEl()
      // Pending probe = data-disabled; a resolved probe clears it.
      expect(el).not.toHaveAttribute('data-disabled')
      return el
    })
    expect(el).not.toBeChecked()
    // The state came from the command — the honest read path.
    expect(getLaunchOnLogin).toHaveBeenCalledTimes(1)
  })

  it('renders ON from the OS truth', async () => {
    getLaunchOnLogin.mockResolvedValue(true)
    render(wrap(<GeneralPane />))
    await waitFor(() => expect(switchEl()).not.toHaveAttribute('data-disabled'))
    expect(switchEl()).toBeChecked()
  })

  it('stays disabled until the OS probe answers', async () => {
    getLaunchOnLogin.mockImplementation(() => new Promise<boolean>(() => {}))
    render(wrap(<GeneralPane />))
    // Base UI expresses disabled as aria/data-disabled, not the HTML attr.
    expect(switchEl()).toHaveAttribute('data-disabled', '')
    // And the user cannot fire a write against a guess.
    fireEvent.click(switchEl())
    expect(configure).not.toHaveBeenCalled()
  })

  it('falls back to the config value when the OS query fails (plugin absent)', async () => {
    getLaunchOnLogin.mockRejectedValue('autostart unavailable: plugin is not registered')
    getConfig.mockResolvedValue({ launch_on_login: true } as never)
    render(wrap(<GeneralPane />))
    // The backend's own fallback shape: remembered intent, not a blind off.
    // checked=true can only come from the config fallback here.
    await waitFor(() => expect(switchEl()).toBeChecked())
    expect(switchEl()).not.toHaveAttribute('data-disabled')
  })

  it('falls back to OFF when both the OS query and the config value are absent', async () => {
    getLaunchOnLogin.mockRejectedValue('unsupported platform')
    getConfig.mockResolvedValue({} as never)
    render(wrap(<GeneralPane />))
    await waitFor(() => expect(switchEl()).not.toHaveAttribute('data-disabled'))
    expect(switchEl()).not.toBeChecked()
  })

  it('writes optimistically through configure(\'launch_on_login\')', async () => {
    getLaunchOnLogin.mockResolvedValue(false)
    render(wrap(<GeneralPane />))
    await waitFor(() => expect(switchEl()).not.toHaveAttribute('data-disabled'))
    fireEvent.click(switchEl())
    // Optimistic: the switch flips before the write lands.
    expect(switchEl()).toBeChecked()
    await waitFor(() => expect(configure).toHaveBeenCalledWith({ key: 'launch_on_login', value: 'true' }))
    await waitFor(() => expect(switchEl()).toBeChecked())
  })

  it('reverts and toasts the OS refusal when the registration is rejected', async () => {
    getLaunchOnLogin.mockResolvedValue(false)
    configure.mockRejectedValue('autostart enable failed: launchd refused')
    render(wrap(<GeneralPane />))
    await waitFor(() => expect(switchEl()).not.toHaveAttribute('data-disabled'))
    fireEvent.click(switchEl())
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    // Honest copy: the system refused the registration (not a generic
    // "update failed"), and the switch is back where the OS says.
    expect(String(vi.mocked(toast.error).mock.calls[0][0])).toMatch(/refused the registration/i)
    expect(switchEl()).not.toBeChecked()
  })

  it('reverts to ON when turning it off is rejected', async () => {
    getLaunchOnLogin.mockResolvedValue(true)
    configure.mockRejectedValue('autostart disable failed')
    render(wrap(<GeneralPane />))
    await waitFor(() => expect(switchEl()).not.toHaveAttribute('data-disabled'))
    fireEvent.click(switchEl())
    await waitFor(() => expect(toast.error).toHaveBeenCalled())
    expect(switchEl()).toBeChecked()
  })
})

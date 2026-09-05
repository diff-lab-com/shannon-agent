import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent, waitFor, within } from '@testing-library/react'

import ConnectionsSettings from '@/components/settings/ConnectionsSettings'
import * as api from '@/lib/tauri-api'

// The global setup auto-wraps each render() in I18nProvider (default locale en),
// and the @/lib/tauri-api factory mock seeds gatewayReadConfig/HasSecret with
// sensible defaults — matching the BillingSettings/AdvancedSettings convention
// (no beforeEach restore: that would wipe the factory mocks' mockResolvedValue).

describe('ConnectionsSettings', () => {
  it('renders the title and all eight platforms, none configured by default', async () => {
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByText('Social Connections')).toBeInTheDocument())
    expect(screen.getByText('Slack')).toBeInTheDocument()
    expect(screen.getByText('DingTalk (钉钉)')).toBeInTheDocument()
    // P1-4 status model: no credentials stored → every platform is 未配置.
    await waitFor(() => expect(screen.getAllByText('Not configured').length).toBe(8))
  })

  it('stores the platform credential in the OS keyring on save', async () => {
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByTestId('connection-slack')).toBeInTheDocument())
    const row = within(screen.getByTestId('connection-slack'))
    fireEvent.change(row.getByLabelText('Bot token — Slack'), {
      target: { value: 'xoxb-secret' },
    })
    fireEvent.click(row.getByRole('button', { name: 'Save' }))
    await waitFor(() =>
      expect(api.gatewaySetSecret).toHaveBeenCalledWith('slack/bot-token', 'xoxb-secret'),
    )
  })

  it('writes the adapter into the gateway config when a platform is enabled', async () => {
    const writeSpy = vi.spyOn(api, 'gatewayWriteConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByTestId('connection-telegram')).toBeInTheDocument())
    // P1-4: three switches per row now (enable + 2 trigger toggles) — pick by name.
    fireEvent.click(
      within(screen.getByTestId('connection-telegram')).getByRole('switch', { name: 'Enable' }),
    )
    await waitFor(() => expect(writeSpy).toHaveBeenCalled())
    const written = writeSpy.mock.calls[0]![0]
    expect(written.adapters).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          platform: 'telegram',
          enabled: true,
          secrets: { botToken: 'telegram/bot-token' },
        }),
      ]),
    )
  })

  it('persists the engine connection URLs', async () => {
    const writeSpy = vi.spyOn(api, 'gatewayWriteConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://new/ws', httpBaseUrl: 'http://new' },
      adapters: [],
    })
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByText('Engine connection')).toBeInTheDocument())
    fireEvent.change(screen.getByLabelText('Engine WebSocket URL'), {
      target: { value: 'ws://new/ws' },
    })
    fireEvent.change(screen.getByLabelText('Engine HTTP base URL'), {
      target: { value: 'http://new' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Save engine' }))
    await waitFor(() => expect(writeSpy).toHaveBeenCalled())
    const written = writeSpy.mock.calls[0]![0]
    expect(written.engine.wsUrl).toBe('ws://new/ws')
    expect(written.engine.httpBaseUrl).toBe('http://new')
  })

  // ── P2 per-platform multi-secret model ─────────────────────────────────────

  it('writes every platform slot into the gateway config when slack is enabled', async () => {
    const writeSpy = vi.spyOn(api, 'gatewayWriteConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByTestId('connection-slack')).toBeInTheDocument())
    fireEvent.click(
      within(screen.getByTestId('connection-slack')).getByRole('switch', { name: 'Enable' }),
    )
    await waitFor(() => expect(writeSpy).toHaveBeenCalled())
    const written = writeSpy.mock.calls[0]![0]
    expect(written.adapters).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          platform: 'slack',
          enabled: true,
          // both the bot-token and the signing-secret slots are mapped
          secrets: { botToken: 'slack/bot-token', signingSecret: 'slack/signing-secret' },
        }),
      ]),
    )
  })

  it('saves both slack slots when both are filled', async () => {
    const setSpy = vi.spyOn(api, 'gatewaySetSecret').mockResolvedValue(undefined)
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByTestId('connection-slack')).toBeInTheDocument())
    const row = within(screen.getByTestId('connection-slack'))
    fireEvent.change(row.getByLabelText('Bot token — Slack'), { target: { value: 'xoxb-tok' } })
    fireEvent.change(row.getByLabelText('Signing secret — Slack'), { target: { value: 'shh' } })
    fireEvent.click(row.getByRole('button', { name: 'Save' }))
    await waitFor(() => expect(setSpy).toHaveBeenCalledTimes(2))
    expect(setSpy).toHaveBeenCalledWith('slack/bot-token', 'xoxb-tok')
    expect(setSpy).toHaveBeenCalledWith('slack/signing-secret', 'shh')
  })

  it('marks every platform Configured once all required slots are stored', async () => {
    vi.spyOn(api, 'gatewayHasSecret').mockResolvedValue(true)
    render(<ConnectionsSettings />)
    // Gateway supervisor mock is "stopped", so stored credentials read as
    // Configured (ready) rather than Running for all eight platforms.
    await waitFor(() => expect(screen.getAllByText('Configured').length).toBe(8))
    expect(screen.queryAllByText('Not configured').length).toBe(0)
  })


  // ── E-1 方案 C — gateway process lifecycle card ────────────────────────────

  it('renders the gateway process card with managed on by default', async () => {
    render(<ConnectionsSettings />)
    const managedSwitch = await waitFor(() =>
      screen.getByRole('switch', { name: 'Managed by desktop' }),
    )
    expect(managedSwitch).toBeChecked()
    expect(screen.getByRole('button', { name: 'Start' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument()
    // Mock gatewaySupervisorStatus → { managed: true, status: 'stopped' }
    expect(screen.getByTestId('gateway-status-badge')).toHaveTextContent('Stopped')
  })

  it('persists the managed flag when the switch is toggled off', async () => {
    const managedSpy = vi
      .spyOn(api, 'gatewaySetManaged')
      .mockResolvedValue({ managed: false, status: 'stopped' })
    render(<ConnectionsSettings />)
    await waitFor(() =>
      expect(screen.getByRole('switch', { name: 'Managed by desktop' })).toBeInTheDocument(),
    )
    fireEvent.click(screen.getByRole('switch', { name: 'Managed by desktop' }))
    await waitFor(() => expect(managedSpy).toHaveBeenCalledWith(false))
    // Toggling off hides the start/stop + status row.
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Start' })).not.toBeInTheDocument(),
    )
  })

  it('starts the supervised gateway and shows the running badge', async () => {
    const startSpy = vi
      .spyOn(api, 'gatewaySupervisorStart')
      .mockResolvedValue({ managed: true, status: { running: { pid: 1234 } } })
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Start' })).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Start' }))
    await waitFor(() => expect(startSpy).toHaveBeenCalled())
    await waitFor(() =>
      expect(screen.getByTestId('gateway-status-badge')).toHaveTextContent('Running'),
    )
    expect(screen.getByTestId('gateway-status-badge')).toHaveTextContent('1234')
  })

  it('stops the supervised gateway on click', async () => {
    const stopSpy = vi
      .spyOn(api, 'gatewaySupervisorStop')
      .mockResolvedValue({ managed: true, status: 'stopped' })
    render(<ConnectionsSettings />)
    await waitFor(() => expect(screen.getByRole('button', { name: 'Stop' })).toBeInTheDocument())
    fireEvent.click(screen.getByRole('button', { name: 'Stop' }))
    await waitFor(() => expect(stopSpy).toHaveBeenCalled())
  })

  // ── P1.3 — mobile device pairing card ──────────────────────────────────────

  it('renders the mobile pairing card with no devices by default', async () => {
    render(<ConnectionsSettings />)
    await waitFor(() =>
      expect(screen.getByText('Mobile device pairing')).toBeInTheDocument(),
    )
    expect(screen.getByText('No devices paired yet.')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'Generate pairing code' })).toBeInTheDocument()
  })

  it('mints a pair token and shows the QR + LAN endpoint on generate', async () => {
    render(<ConnectionsSettings />)
    const btn = await screen.findByRole('button', { name: 'Generate pairing code' })
    fireEvent.click(btn)
    await waitFor(() => expect(api.mobileGeneratePairToken).toHaveBeenCalled())
    // The QR image + endpoint code appear.
    const qr = await screen.findByTestId('mobile-qr')
    expect(qr.querySelector('img')).toHaveAttribute(
      'src',
      'data:image/svg+xml;base64,PHN2Zz4=',
    )
    expect(screen.getByText('ws://192.168.1.10:33430')).toBeInTheDocument()
  })

  it('surfaces a LAN-detection error and renders no QR', async () => {
    vi.spyOn(api, 'mobileGeneratePairToken').mockRejectedValue('no LAN IPv4 route')
    render(<ConnectionsSettings />)
    fireEvent.click(await screen.findByRole('button', { name: 'Generate pairing code' }))
    await waitFor(() =>
      expect(screen.getByTestId('mobile-pair-error')).toHaveTextContent('no LAN IPv4 route'),
    )
    expect(screen.queryByTestId('mobile-qr')).not.toBeInTheDocument()
  })

  it('lists a paired device and revokes it through a confirm dialog', async () => {
    vi.spyOn(api, 'mobileListPairedDevices').mockResolvedValue([
      { deviceId: 'dev-1', publicKey: 'pk', label: 'Pixel', addedAt: 1, lastSeenAt: 2 },
    ])
    const revokeSpy = vi.spyOn(api, 'mobileRevokeDevice').mockResolvedValue(true)
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('mobile-device-dev-1')
    expect(within(row).getByText('Pixel')).toBeInTheDocument()
    // Click the row's Revoke button → opens the confirm dialog.
    fireEvent.click(within(row).getByRole('button', { name: 'Revoke' }))
    const dialog = await screen.findByRole('alertdialog')
    // Confirm via the dialog's destructive button (also labelled "Revoke").
    fireEvent.click(within(dialog).getByRole('button', { name: 'Revoke' }))
    await waitFor(() => expect(revokeSpy).toHaveBeenCalledWith('dev-1'))
    // The row is gone.
    await waitFor(() =>
      expect(screen.queryByTestId('mobile-device-dev-1')).not.toBeInTheDocument(),
    )
  })
  // ── P1-4 — per-platform status dot ──────────────────────────────────────────

  it('shows Running for an enabled, fully-configured platform while the gateway runs', async () => {
    vi.spyOn(api, 'gatewaySupervisorStatus').mockResolvedValue({
      managed: true,
      status: { running: { pid: 42 } },
    })
    vi.spyOn(api, 'gatewayHasSecret').mockResolvedValue(true)
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [
        { platform: 'telegram', enabled: true, secrets: { botToken: 'telegram/bot-token' } },
      ],
    })
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    await waitFor(() => expect(within(row).getByText('Running')).toBeInTheDocument())
    expect(within(row).getByTestId('connection-status-telegram')).toBeInTheDocument()
  })

  it('shows Error for an enabled platform after the gateway process exited', async () => {
    vi.spyOn(api, 'gatewaySupervisorStatus').mockResolvedValue({
      managed: true,
      status: { exited: { code: 1 } },
    })
    vi.spyOn(api, 'gatewayHasSecret').mockResolvedValue(true)
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [
        { platform: 'telegram', enabled: true, secrets: { botToken: 'telegram/bot-token' } },
      ],
    })
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    await waitFor(() => expect(within(row).getByText('Error')).toBeInTheDocument())
  })

  // ── P1-4 — credentials never echoed ─────────────────────────────────────────

  it('never echoes a stored credential into the form', async () => {
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    vi.spyOn(api, 'gatewayHasSecret').mockResolvedValue(true)
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    const input = within(row).getByLabelText('Bot token — Telegram') as HTMLInputElement
    // type=password + empty value: presence is probed, the secret value never
    // crosses back into the webview.
    expect(input.getAttribute('type')).toBe('password')
    expect(input.value).toBe('')
  })

  // ── P1-4 — inbound trigger policy toggles ───────────────────────────────────

  it('persists group @mention trigger policy into options.trigger', async () => {
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    const writeSpy = vi
      .spyOn(api, 'gatewayWriteConfig')
      .mockImplementation((cfg) => Promise.resolve(cfg))
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    // Enable first — the trigger toggles only apply to an existing adapter entry.
    fireEvent.click(within(row).getByRole('switch', { name: 'Enable' }))
    const groupSwitch = await within(row).findByRole<'switch'>('switch', {
      name: 'Groups need @mention or /shannon',
    })
    await waitFor(() => expect(groupSwitch).not.toHaveAttribute('aria-disabled', 'true'))
    fireEvent.click(groupSwitch)
    await waitFor(() => {
      const last = writeSpy.mock.calls.at(-1)![0] as {
        adapters: Array<{ platform: string; options?: { trigger?: Record<string, unknown> } }>
      }
      const tg = last.adapters.find((a) => a.platform === 'telegram')!
      expect(tg.options?.trigger).toEqual({ groupMode: 'any' })
    })
  })

  it('persists dmDirect:false when DM direct response is switched off', async () => {
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    const writeSpy = vi
      .spyOn(api, 'gatewayWriteConfig')
      .mockImplementation((cfg) => Promise.resolve(cfg))
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    fireEvent.click(within(row).getByRole('switch', { name: 'Enable' }))
    const dmSwitch = await within(row).findByRole<'switch'>('switch', {
      name: 'Answer DMs directly',
    })
    await waitFor(() => expect(dmSwitch).not.toHaveAttribute('aria-disabled', 'true'))
    fireEvent.click(dmSwitch)
    await waitFor(() => {
      const last = writeSpy.mock.calls.at(-1)![0] as {
        adapters: Array<{ platform: string; options?: { trigger?: Record<string, unknown> } }>
      }
      const tg = last.adapters.find((a) => a.platform === 'telegram')!
      expect(tg.options?.trigger).toEqual({ dmDirect: false })
    })
  })

  it('keeps the trigger toggles disabled until the platform has an adapter entry', async () => {
    // Spies leak across tests in this file (no beforeEach restore by convention),
    // so pin the state this test needs: no adapter entries, nothing configured.
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    const groupSwitch = within(row).getByRole('switch', {
      name: 'Groups need @mention or /shannon',
    })
    // Base UI renders the switch as a span with aria-disabled — jest-dom's
    // toBeDisabled() only understands form-control semantics.
    const expectAriaDisabled = (el: HTMLElement) =>
      expect(el).toHaveAttribute('aria-disabled', 'true')
    expectAriaDisabled(groupSwitch)
    expectAriaDisabled(within(row).getByRole('switch', { name: 'Answer DMs directly' }))
  })

  // ── P1-4 — reload semantics + test-connection ───────────────────────────────

  it('offers a one-click gateway restart after a change while the gateway runs', async () => {
    vi.spyOn(api, 'gatewayReadConfig').mockResolvedValue({
      engine: { wsUrl: 'ws://x/ws', httpBaseUrl: 'http://x' },
      adapters: [],
    })
    vi.spyOn(api, 'gatewaySupervisorStatus').mockResolvedValue({
      managed: true,
      status: { running: { pid: 42 } },
    })
    const stopSpy = vi
      .spyOn(api, 'gatewaySupervisorStop')
      .mockResolvedValue({ managed: true, status: 'stopped' })
    const startSpy = vi
      .spyOn(api, 'gatewaySupervisorStart')
      .mockResolvedValue({ managed: true, status: { running: { pid: 43 } } })
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    fireEvent.click(within(row).getByRole('switch', { name: 'Enable' }))
    fireEvent.click(await screen.findByRole('button', { name: 'Restart gateway' }))
    await waitFor(() => expect(startSpy).toHaveBeenCalled())
    expect(stopSpy).toHaveBeenCalled()
  })

  it('marks test-connection as unsupported for now', async () => {
    render(<ConnectionsSettings />)
    const row = await screen.findByTestId('connection-telegram')
    const btn = within(row).getByRole('button', { name: 'Test connection' })
    expect(btn).toBeDisabled()
  })

})

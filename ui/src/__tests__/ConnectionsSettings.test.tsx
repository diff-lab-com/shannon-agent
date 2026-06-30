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
    // gatewayHasSecret defaults to false for every platform.
    await waitFor(() => expect(screen.getAllByText('No credential').length).toBe(8))
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
    fireEvent.click(within(screen.getByTestId('connection-telegram')).getByRole('switch'))
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
})

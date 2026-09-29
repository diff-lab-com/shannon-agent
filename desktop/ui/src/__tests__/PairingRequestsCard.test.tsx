import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { toast } from 'sonner'

import { PairingRequestsCard } from '@/components/settings/connections-settings/PairingRequestsCard'
import * as api from '@/lib/tauri-api'
import type { GatewayConfig, GatewayPairingRequest } from '@/types'

// The global setup auto-wraps each render() in I18nProvider (default locale
// en) and seeds the @/lib/tauri-api factory mock; these tests override the
// pairing calls per-case with spyOn (the ConnectionsSettings convention).

function request(partial: Partial<GatewayPairingRequest>): GatewayPairingRequest {
  return {
    code: '123456',
    platform: 'slack',
    senderId: 'UDEMO',
    requestedAt: Date.now() - 30_000,
    expiresAt: Date.now() + 240_000,
    ...partial,
  }
}

const baseConfig: GatewayConfig = {
  engine: { wsUrl: 'ws://127.0.0.1:33420/api/ws', httpBaseUrl: 'http://127.0.0.1:33420' },
  adapters: [],
  mobile: { enabled: true, host: '0.0.0.0', port: 33430 },
}

const running = { managed: true, status: { running: { pid: 42 } } } as const
const stopped = { managed: true, status: 'stopped' } as const

describe('PairingRequestsCard', () => {
  beforeEach(() => {
    vi.spyOn(api, 'gatewayPairingPending').mockResolvedValue([])
    vi.spyOn(api, 'gatewayPairingApprove').mockResolvedValue(request({}))
  })

  it('renders nothing when the gateway mobile server is disabled', () => {
    const cfg: GatewayConfig = { ...baseConfig, mobile: { enabled: false } }
    const { container } = render(<PairingRequestsCard config={cfg} procState={null} />)
    expect(container.querySelector('[data-testid="pairing-requests-card"]')).toBeNull()
    expect(api.gatewayPairingPending).not.toHaveBeenCalled()
  })

  it('lists pending pairing requests with code + sender on mount', async () => {
    vi.spyOn(api, 'gatewayPairingPending').mockResolvedValue([
      request({ code: '246810', platform: 'slack', senderId: 'U0DEMO1' }),
      request({ code: '135791', platform: 'telegram', senderId: 'TEDEMO2' }),
    ])
    render(<PairingRequestsCard config={baseConfig} procState={{ ...running }} />)
    await waitFor(() =>
      expect(screen.getByTestId('pairing-request-246810')).toBeInTheDocument(),
    )
    expect(screen.getByTestId('pairing-request-135791')).toBeInTheDocument()
    expect(screen.getByText(/slack:U0DEMO1/)).toBeInTheDocument()
    // The challenge code is visible so the approver can correlate with the IM.
    expect(screen.getByText(new RegExp('246810'))).toBeInTheDocument()
  })

  it('approves a request through the gateway command and then refreshes', async () => {
    const pendingSpy = vi
      .spyOn(api, 'gatewayPairingPending')
      .mockResolvedValueOnce([request({ code: '246810' })])
      .mockResolvedValueOnce([])
    vi.spyOn(api, 'gatewayPairingApprove').mockResolvedValue(request({ code: '246810' }))

    render(<PairingRequestsCard config={baseConfig} procState={{ ...running }} />)
    await waitFor(() => expect(screen.getByTestId('pairing-request-246810')).toBeInTheDocument())

    fireEvent.click(screen.getByTestId('pairing-approve-246810'))
    await waitFor(() => expect(api.gatewayPairingApprove).toHaveBeenCalledWith('246810'))
    // The list re-pulls after approval; the row is gone.
    await waitFor(() => expect(screen.getByTestId('pairing-requests-empty')).toBeInTheDocument())
    expect(pendingSpy).toHaveBeenCalledTimes(2)
  })

  it('shows the empty state when nothing is pending', async () => {
    render(<PairingRequestsCard config={baseConfig} procState={{ ...running }} />)
    await waitFor(() => expect(screen.getByTestId('pairing-requests-empty')).toBeInTheDocument())
  })

  it('flags a stopped gateway next to the refresh control', async () => {
    render(<PairingRequestsCard config={baseConfig} procState={{ ...stopped }} />)
    await waitFor(() => expect(screen.getByTestId('pairing-requests-stopped')).toBeInTheDocument())
    // The refresh control is still there for when the user starts the gateway.
    expect(screen.getByTestId('pairing-requests-refresh')).toBeInTheDocument()
  })

  it('surfaces an approve failure without losing the list', async () => {
    vi.spyOn(api, 'gatewayPairingPending').mockResolvedValue([request({ code: '000000' })])
    vi.spyOn(api, 'gatewayPairingApprove').mockRejectedValue('Unknown or expired pairing code')
    const errorSpy = vi.spyOn(toast, 'error').mockImplementation(() => 'x')

    render(<PairingRequestsCard config={baseConfig} procState={{ ...running }} />)
    await waitFor(() => expect(screen.getByTestId('pairing-request-000000')).toBeInTheDocument())
    fireEvent.click(screen.getByTestId('pairing-approve-000000'))
    await waitFor(() => expect(errorSpy).toHaveBeenCalled())
    // The failed request stays listed so the user can retry after a refresh.
    expect(screen.getByTestId('pairing-request-000000')).toBeInTheDocument()
  })
})

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import * as api from '@/lib/tauri-api'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import NetworkSettings, { validateProxyUrl } from '@/components/settings/NetworkSettings'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>{ui}</MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

const baseConfig = {
  provider: 'anthropic',
  model: 'claude-sonnet-4-6',
  api_key: 'sk-test',
  working_dir: '/tmp',
  approval_mode: 'normal',
}

describe('validateProxyUrl', () => {
  it('accepts http/https and empty (clear), rejects the rest', () => {
    expect(validateProxyUrl('http://127.0.0.1:7890')).toBe(true)
    expect(validateProxyUrl('HTTPS://corp-gateway.local:3128')).toBe(true)
    expect(validateProxyUrl('')).toBe(true)
    expect(validateProxyUrl('   ')).toBe(true)
    expect(validateProxyUrl('127.0.0.1:7890')).toBe(false)
    expect(validateProxyUrl('socks5://corp:1080')).toBe(false)
    expect(validateProxyUrl('ftp://x')).toBe(false)
  })
})

describe('NetworkSettings (Settings R3 T4 — corporate network trio)', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    vi.mocked(api.getConfig).mockResolvedValue({ ...baseConfig })
  })

  it('renders the three cards and hydrates the persisted values', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({
      ...baseConfig,
      network_proxy_url: 'http://127.0.0.1:7890',
      network_no_proxy: 'localhost,127.0.0.1',
      network_ca_cert_path: '/home/u/certs/root-ca.pem',
    })
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input') as HTMLInputElement
    await waitFor(() => {
      expect((screen.getByTestId('network-no-proxy-input') as HTMLInputElement).value).toBe('localhost,127.0.0.1')
      expect((screen.getByTestId('network-ca-input') as HTMLInputElement).value).toBe('/home/u/certs/root-ca.pem')
    })
    expect(proxy.value).toBe('http://127.0.0.1:7890')
    expect(screen.getByTestId('network-proxy-card')).toBeInTheDocument()
    expect(screen.getByTestId('network-no-proxy-card')).toBeInTheDocument()
    expect(screen.getByTestId('network-ca-card')).toBeInTheDocument()
  })

  it('marks every card as restart-app semantics (three badges)', async () => {
    render(wrap(<NetworkSettings />))
    // The EffectBadge copy comes from the T1 i18n seed.
    const badges = await screen.findAllByText('Restart required')
    expect(badges).toHaveLength(3)
  })

  it('shows the inline proxy scheme error on blur and blocks save', async () => {
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input')
    fireEvent.change(proxy, { target: { value: '127.0.0.1:7890' } })
    fireEvent.blur(proxy)
    const error = screen.getByTestId('network-proxy-error')
    expect(error).toHaveTextContent('The proxy URL must start with http:// or https://')

    fireEvent.click(screen.getByTestId('network-save'))
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('clears the inline error once the value becomes a valid http(s) URL', async () => {
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input')
    fireEvent.change(proxy, { target: { value: 'nope' } })
    fireEvent.blur(proxy)
    expect(screen.getByTestId('network-proxy-error')).toBeInTheDocument()
    fireEvent.change(proxy, { target: { value: 'https://corp:3128' } })
    expect(screen.queryByTestId('network-proxy-error')).not.toBeInTheDocument()
  })

  it('saves all three keys via configure and refreshes the config snapshot', async () => {
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input')
    fireEvent.change(proxy, { target: { value: 'http://127.0.0.1:7890' } })
    fireEvent.change(screen.getByTestId('network-no-proxy-input'), { target: { value: 'localhost,127.0.0.1' } })
    fireEvent.change(screen.getByTestId('network-ca-input'), { target: { value: '~/certs/root-ca.pem' } })

    fireEvent.click(screen.getByTestId('network-save'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledTimes(3)
    })
    expect(api.configure).toHaveBeenCalledWith({ key: 'network.proxy_url', value: 'http://127.0.0.1:7890' })
    expect(api.configure).toHaveBeenCalledWith({ key: 'network.no_proxy', value: 'localhost,127.0.0.1' })
    expect(api.configure).toHaveBeenCalledWith({ key: 'network.ca_cert_path', value: '~/certs/root-ca.pem' })
  })

  it('trims values before sending them to configure', async () => {
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input')
    fireEvent.change(proxy, { target: { value: '  http://p.local:8080  ' } })
    fireEvent.change(screen.getByTestId('network-no-proxy-input'), { target: { value: '  localhost  ' } })
    fireEvent.change(screen.getByTestId('network-ca-input'), { target: { value: '  ' } })

    fireEvent.click(screen.getByTestId('network-save'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.proxy_url', value: 'http://p.local:8080' })
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.no_proxy', value: 'localhost' })
      // Empty CA path clears (R1: empty = keep the env fallback).
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.ca_cert_path', value: '' })
    })
  })

  it('shows the inline error and refuses to save when the proxy is invalid at save time (no blur)', async () => {
    render(wrap(<NetworkSettings />))
    const proxy = await screen.findByTestId('network-proxy-input')
    fireEvent.change(proxy, { target: { value: 'socks5://corp:1080' } })
    fireEvent.click(screen.getByTestId('network-save'))
    expect(screen.getByTestId('network-proxy-error')).toBeInTheDocument()
    expect(api.configure).not.toHaveBeenCalled()
  })

  it('still saves when only the proxy is filled (no_proxy / CA stay empty = cleared)', async () => {
    vi.mocked(api.getConfig).mockResolvedValue({
      ...baseConfig,
      network_proxy_url: 'http://old:1',
      network_no_proxy: 'stale',
      network_ca_cert_path: '/gone.pem',
    })
    render(wrap(<NetworkSettings />))
    // Hydration fills the fields from config, then the user clears them.
    await waitFor(() => {
      expect((screen.getByTestId('network-proxy-input') as HTMLInputElement).value).toBe('http://old:1')
    })
    fireEvent.change(screen.getByTestId('network-proxy-input'), { target: { value: '' } })
    fireEvent.change(screen.getByTestId('network-no-proxy-input'), { target: { value: '' } })
    fireEvent.change(screen.getByTestId('network-ca-input'), { target: { value: '' } })
    fireEvent.click(screen.getByTestId('network-save'))
    await waitFor(() => {
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.proxy_url', value: '' })
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.no_proxy', value: '' })
      expect(api.configure).toHaveBeenCalledWith({ key: 'network.ca_cert_path', value: '' })
    })
  })
})

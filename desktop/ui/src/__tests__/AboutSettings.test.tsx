import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import AboutSettings from '@/components/settings/AboutSettings'
import * as api from '@/lib/tauri-api'

// The version line uses the real @tauri-apps/api/app helper (the same source
// Advanced's dev card reads) — mock it so the test controls the value.
vi.mock('@tauri-apps/api/app', () => ({
  getVersion: vi.fn().mockResolvedValue('0.11.0'),
}))

// Settings R3 (T1): 关于 absorbs the update check from the dev-gated 高级 and
// adds a read-only data-directory line. The global setup mock covers
// checkAppUpdate / openReleasePage / getShannonHome with defaults; tests
// override per scenario.
describe('AboutSettings', () => {
  beforeEach(() => {
    vi.mocked(api.checkAppUpdate).mockClear()
    vi.mocked(api.openReleasePage).mockClear()
    vi.mocked(api.getShannonHome).mockClear()
  })

  it('renders the about card with title, help copy, and the real version badge', async () => {
    render(<AboutSettings />)
    expect(screen.getByText('About Shannon')).toBeInTheDocument()
    expect(screen.getByText('Version')).toBeInTheDocument()
    expect(screen.getByText('Version, updates, and where your data lives.')).toBeInTheDocument()
    await waitFor(() => expect(screen.getByText('v0.11.0')).toBeInTheDocument())
  })

  it('renders the read-only data directory from getShannonHome', async () => {
    render(<AboutSettings />)
    expect(screen.getByText('Data directory')).toBeInTheDocument()
    const path = await screen.findByTestId('about-datadir-path')
    expect(path).toHaveTextContent('/home/tester/.shannon')
  })

  it('shows the up-to-date badge and release-page entry after a check', async () => {
    render(<AboutSettings />)
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() => expect(screen.getByTestId('about-update-badge')).toHaveTextContent('Up to date'))
    expect(screen.getByRole('button', { name: /Open release page/ })).toBeInTheDocument()
  })

  it('announces an available update and opens the release page', async () => {
    vi.mocked(api.checkAppUpdate).mockResolvedValueOnce({
      currentVersion: '0.11.0',
      latestVersion: 'v0.12.0',
      updateAvailable: true,
      releaseUrl: 'https://github.com/diff-lab-com/shannon-agent/releases/tag/v0.12.0',
      error: null,
    })
    render(<AboutSettings />)
    fireEvent.click(screen.getByRole('button', { name: 'Check for updates' }))
    await waitFor(() =>
      expect(screen.getByTestId('about-update-badge')).toHaveTextContent('v0.12.0 available'),
    )
    fireEvent.click(screen.getByRole('button', { name: /Open release page/ }))
    await waitFor(() =>
      expect(api.openReleasePage).toHaveBeenCalledWith(
        'https://github.com/diff-lab-com/shannon-agent/releases/tag/v0.12.0',
      ),
    )
  })
})

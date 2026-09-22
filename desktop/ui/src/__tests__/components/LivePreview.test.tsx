// Tests for the P1-5 C-1 Live preview (the dock's 预览 tab). The retired
// ArtifactPanel "Live tab" section is gone with the panel — batch D4 keeps
// live preview only inside RightDock.
// start/stop/reload dispatch, detected/not-detected/error states, the iframe
// sandbox baseline, and i18n key parity (en + zh-CN).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { LivePreview } from '@/components/artifact/LivePreview'
import * as api from '@/lib/tauri-api'
import en from '@/i18n/locales/en.json'
import zhCN from '@/i18n/locales/zh-CN.json'

vi.mock('@/lib/tauri-api', () => ({
  previewStatus: vi.fn(),
  previewDetect: vi.fn(),
  previewStart: vi.fn(),
  previewStop: vi.fn(),
  previewLogs: vi.fn(),
  previewCapture: vi.fn(),
}))

const wrapper = ({ children }: { children: React.ReactNode }) => (
  <I18nProvider>{children}</I18nProvider>
)

function mockRunning(url = 'http://127.0.0.1:5173') {
  vi.mocked(api.previewStatus).mockResolvedValue({ running: true, url, startedAtMs: 1_000 })
  vi.mocked(api.previewLogs).mockResolvedValue([
    { tsMs: 1_000, stream: 'system', text: 'dev server ready' },
  ])
}

function mockStopped(detected: { command: string; url: string } | null) {
  vi.mocked(api.previewStatus).mockResolvedValue({ running: false, url: null, startedAtMs: null })
  vi.mocked(api.previewDetect).mockResolvedValue({ devServer: detected })
}

beforeEach(() => {
  vi.clearAllMocks()
  vi.mocked(api.previewLogs).mockResolvedValue([])
  vi.mocked(api.previewDetect).mockResolvedValue({ devServer: null })
})

describe('LivePreview (idle states)', () => {
  it('offers start with the detected command when stopped', async () => {
    mockStopped({ command: 'npm run dev', url: 'http://localhost:5173' })
    render(<LivePreview />, { wrapper })
    expect(await screen.findByText(/npm run dev/)).toBeTruthy()
    const start = screen.getByRole('button', { name: /start the project dev server/i })
    expect(start).toBeTruthy()
  })

  it('dispatches preview_start and switches to the running frame', async () => {
    mockStopped({ command: 'pnpm run dev', url: 'http://localhost:3000' })
    vi.mocked(api.previewStart).mockResolvedValue({ url: 'http://127.0.0.1:3000' })
    render(<LivePreview />, { wrapper })
    const start = await screen.findByRole('button', { name: /start the project dev server/i })
    fireEvent.click(start)
    await waitFor(() => {
      expect(api.previewStart).toHaveBeenCalledTimes(1)
    })
    const frame = await screen.findByTitle('Live preview content')
    expect(frame.getAttribute('src')).toBe('http://127.0.0.1:3000')
    // Address bar is read-only and shows the url.
    const address = screen.getByLabelText(/preview url/i) as HTMLInputElement
    expect(address.readOnly).toBe(true)
    expect(address.value).toBe('http://127.0.0.1:3000')
  })

  it('shows the not-detected state when there is no dev server', async () => {
    mockStopped(null)
    render(<LivePreview />, { wrapper })
    expect(
      await screen.findByText(/No dev server detected in this project/),
    ).toBeTruthy()
    expect(screen.queryByRole('button', { name: /start the project dev server/i })).toBeNull()
  })

  it('surfaces backend errors with a retry affordance', async () => {
    vi.mocked(api.previewStatus).mockRejectedValue(new Error('backend boom'))
    render(<LivePreview />, { wrapper })
    expect(await screen.findByText(/backend boom/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Retry' })).toBeTruthy()
  })
})

describe('LivePreview (running state)', () => {
  it('renders the running iframe with the extended-minimal sandbox', async () => {
    mockRunning()
    render(<LivePreview />, { wrapper })
    const frame = await screen.findByTitle('Live preview content')
    expect(frame.getAttribute('src')).toBe('http://127.0.0.1:5173')
    const sandbox = frame.getAttribute('sandbox') ?? ''
    expect(sandbox).toContain('allow-scripts')
    // Security baseline: no same-origin grant for a real network page.
    expect(sandbox.includes('allow-same-origin')).toBe(false)
  })

  it('dispatches preview_stop from the stop button and returns to idle', async () => {
    mockRunning()
    vi.mocked(api.previewStop).mockResolvedValue(undefined)
    vi.mocked(api.previewDetect).mockResolvedValue({ devServer: null })
    render(<LivePreview />, { wrapper })
    const stop = await screen.findByRole('button', { name: /stop the dev server/i })
    fireEvent.click(stop)
    await waitFor(() => {
      expect(api.previewStop).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(api.previewDetect).toHaveBeenCalledTimes(1)
    })
    await waitFor(() => {
      expect(screen.queryByTitle('Live preview content')).toBeNull()
    })
  })

  it('reloads the frame from the refresh button', async () => {
    mockRunning()
    render(<LivePreview />, { wrapper })
    const refresh = await screen.findByRole('button', { name: /reload the live preview/i })
    fireEvent.click(refresh)
    await waitFor(() => {
      expect(api.previewLogs).toHaveBeenCalledTimes(2)
    })
    expect(screen.getByTitle('Live preview content')).toBeTruthy()
  })
})

describe('Live preview i18n keys', () => {
  it('every live-preview key exists in en and zh-CN', () => {
    const liveKeys = Object.keys(en).filter(
      k => k.startsWith('chat.artifact.live') || k === 'chat.artifact.tab.live',
    )
    expect(liveKeys.length).toBeGreaterThanOrEqual(15)
    for (const key of liveKeys) {
      expect(zhCN, `zh-CN missing ${key}`).toHaveProperty(key)
      expect(String(zhCN[key as keyof typeof zhCN])).not.toBe('')
    }
  })
})

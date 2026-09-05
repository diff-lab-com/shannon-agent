// Tests for the P1-5 D integrated terminal panel: open/close + Ctrl+`,
// tab management with the 4-instance cap, base64 output decoding into
// xterm, stdin dispatch, exit notice, history-warning hint, i18n key
// parity (en + zh-CN), and the xterm theme mapping floor.
//
// jsdom cannot run the real xterm renderer — @xterm/xterm and
// @xterm/addon-fit are replaced with fakes that expose the same surface
// the panel uses (write/onData/onResize/open/dispose, fit). The event
// transport is captured from `listenTerminalOutput` so tests drive the
// exact payload shape the Rust pump emits.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import { xtermThemeFor } from '@/components/terminal/xtermTheme'
import * as api from '@/lib/tauri-api'
import en from '@/i18n/locales/en.json'
import zhCN from '@/i18n/locales/zh-CN.json'
import type { TerminalInfo } from '@/types'

const h = vi.hoisted(() => ({
  terminals: [] as {
    written: string[]
    options: Record<string, unknown>
    element: HTMLElement | null
    dataHandler: ((data: string) => void) | null
    resizeHandler: ((size: { cols: number; rows: number }) => void) | null
    open: (container: HTMLElement) => void
    loadAddon: (addon: unknown) => void
    scrollToBottom: () => void
    focus: () => void
    dispose: () => void
    write: (text: string) => void
    onData: (cb: (data: string) => void) => { dispose: () => void }
    onResize: (cb: (size: { cols: number; rows: number }) => void) => { dispose: () => void }
  }[],
  outputHandler: null as ((payload: { terminalId: string; data: string }) => void) | null,
  unsubscribed: false,
}))

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    written: string[] = []
    options: Record<string, unknown> = {}
    element: HTMLElement | null = null
    dataHandler: ((data: string) => void) | null = null
    resizeHandler: ((size: { cols: number; rows: number }) => void) | null = null
    constructor(options: Record<string, unknown>) {
      this.options = options
      h.terminals.push(this)
    }
    write(text: string) {
      this.written.push(text)
    }
    onData(cb: (data: string) => void) {
      this.dataHandler = cb
      return { dispose: () => {} }
    }
    onResize(cb: (size: { cols: number; rows: number }) => void) {
      this.resizeHandler = cb
      return { dispose: () => {} }
    }
    open(container: HTMLElement) {
      this.element = document.createElement('div')
      this.element.dataset.fake = 'xterm'
      container.appendChild(this.element)
    }
    loadAddon() {}
    scrollToBottom() {}
    focus() {}
    dispose() {}
  },
}))

vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    fit() { /* jsdom has no layout */ }
    dispose() {}
  },
}))

vi.mock('@/lib/tauri-api', () => ({
  terminalSpawn: vi.fn(),
  terminalWrite: vi.fn(),
  terminalResize: vi.fn(),
  terminalKill: vi.fn(),
  terminalList: vi.fn(),
}))

vi.mock('@/lib/runtime/terminalEvents', async () => {
  const actual = await import('@/lib/runtime/terminalEvents')
  return {
    ...actual,
    listenTerminalOutput: (handler: (payload: { terminalId: string; data: string }) => void) => {
      h.outputHandler = handler
      h.unsubscribed = false
      return Promise.resolve(() => {
        h.unsubscribed = true
      })
    },
  }
})

// base64 helpers matching the Rust wire format
function encode(raw: string): string {
  const bytes = new TextEncoder().encode(raw)
  let binary = ''
  bytes.forEach(b => { binary += String.fromCharCode(b) })
  return btoa(binary)
}

// The panel reads `<html data-theme>` (no ThemeProvider needed) — tests
// render it bare and set the attribute directly where relevant.

function info(id: string, dir = '/home/u/demo'): TerminalInfo {
  return { terminalId: id, projectDir: dir, shell: '/bin/bash', startedAtMs: 1_000 }
}

beforeEach(() => {
  vi.clearAllMocks()
  h.terminals.length = 0
  h.outputHandler = null
  localStorage.clear()
  vi.mocked(api.terminalList).mockResolvedValue([])
  vi.mocked(api.terminalSpawn).mockImplementation(async (dir?: string | null) => {
    return { terminalId: `t-${h.terminals.length + 1}-${(dir ?? '').length}` }
  })
  vi.mocked(api.terminalWrite).mockResolvedValue(undefined)
  vi.mocked(api.terminalResize).mockResolvedValue(undefined)
  vi.mocked(api.terminalKill).mockResolvedValue(undefined)
})

async function openPanel() {
  render(<TerminalPanel projectDir="/home/u/demo" />)
  fireEvent.keyDown(window, { key: '`', ctrlKey: true })
  await screen.findByRole('region', { name: 'Integrated terminal' })
}

describe('TerminalPanel (open/close)', () => {
  it('renders collapsed with an accessible toggle', () => {
    render(<TerminalPanel projectDir="/home/u/demo" />)
    const toggle = screen.getByRole('button', { name: /toggle terminal/i })
    expect(toggle.getAttribute('aria-expanded')).toBe('false')
    expect(screen.queryByRole('region')).toBeNull()
  })

  it('Ctrl+` opens the panel and spawns the first terminal', async () => {
    await openPanel()
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalledWith('/home/u/demo'))
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(1))
    // The spawn tab is derived from the passed projectDir.
    expect(screen.getByRole('tab', { name: /demo/ })).toBeTruthy()
  })

  it('Ctrl+` also closes the panel again', async () => {
    await openPanel()
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => {
      expect(screen.queryByRole('region', { name: 'Integrated terminal' })).toBeNull()
    })
  })

  it('reconciles live terminals from terminal_list on first open', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-live-1'), info('t-live-2', '/other')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(2))
    // No duplicate spawn when live terminals exist.
    expect(api.terminalSpawn).not.toHaveBeenCalled()
    // Reconnect hint is shown.
    expect(await screen.findByText(/output history/i)).toBeTruthy()
  })

  it('shows the full-height toggle as aria-pressed', async () => {
    await openPanel()
    const full = screen.getByRole('button', { name: /full-height/i })
    expect(full.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(full)
    expect(full.getAttribute('aria-pressed')).toBe('true')
  })
})

describe('TerminalPanel (output + input)', () => {
  it('decodes base64 terminal:output into the xterm buffer, id-filtered', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    // Mismatched id must be ignored; matching id decoded (base64 → text,
    // incl. multi-byte) and written into the xterm buffer.
    h.outputHandler?.({ terminalId: 'other-terminal', data: encode('not-for-you') })
    h.outputHandler?.({ terminalId: terminalId, data: encode('pty-bytes-✓') })
    expect(h.terminals[0].written.join('')).not.toContain('not-for-you')
    expect(h.terminals[0].written.join('')).toContain('pty-bytes-✓')
  })

  it('routes xterm onData to terminal_write', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals[0]).toBeTruthy())
    h.terminals[0].dataHandler?.('ls\r')
    await waitFor(() => expect(api.terminalWrite).toHaveBeenCalledWith(
      expect.any(String), 'ls\r',
    ))
  })

  it('marks the tab ended when the backend exit notice arrives', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({
      terminalId,
      data: encode('\r\n\x1b[2m[shannon: process exited — done]\x1b[0m\r\n'),
    })
    expect(await screen.findByText(/ended/)).toBeTruthy()
  })
})

describe('TerminalPanel (tab management)', () => {
  it('spawns additional terminals via + and closes them with ×', async () => {
    await openPanel()
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(1))
    const plus = screen.getByRole('button', { name: 'New terminal' })
    fireEvent.click(plus)
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(2))
    expect(api.terminalSpawn).toHaveBeenCalledTimes(2)

    const closeButtons = screen.getAllByRole('button', { name: /close terminal:/i })
    fireEvent.click(closeButtons[0])
    await waitFor(() => expect(api.terminalKill).toHaveBeenCalledTimes(1))
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(1))
  })

  it('disables + at the 4-terminal cap with an explanatory title', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([
      info('t-1'), info('t-2'), info('t-3'), info('t-4'),
    ])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    const plus = await screen.findByRole('button', { name: 'New terminal' })
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(4))
    expect((plus as HTMLButtonElement).disabled).toBe(true)
    expect(plus.getAttribute('title')).toContain('limit reached (4)')
  })

  it('switches tabs on click', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a'), info('t-b', '/other')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    const tabA = await screen.findByRole('tab', { name: /demo/ })
    const tabB = await screen.findByRole('tab', { name: /other/ })
    // The newest listed terminal is selected initially (openPanel picks
    // the last entry, oldest-first ordering from the backend).
    expect(tabB.getAttribute('aria-selected')).toBe('true')
    fireEvent.click(tabA)
    await waitFor(() => expect(tabA.getAttribute('aria-selected')).toBe('true'))
    expect(tabB.getAttribute('aria-selected')).toBe('false')
  })
})

describe('TerminalPanel (i18n + theme)', () => {
  it('en and zh-CN define the same terminal.* keys', () => {
    const enKeys = Object.keys(en).filter(k => k.startsWith('terminal.')).sort()
    const zhKeys = Object.keys(zhCN).filter(k => k.startsWith('terminal.')).sort()
    expect(enKeys.length).toBeGreaterThan(5)
    expect(zhKeys).toEqual(enKeys)
  })

  it('maps the material theme to the light ANSI floor', () => {
    const theme = xtermThemeFor('material', () => 'light')
    expect(theme.red).toBe('#ba1a1a')
    expect(theme.brightWhite).toBe('#ffffff')
  })

  it('maps a dark theme to the dark ANSI floor', () => {
    const theme = xtermThemeFor('tokyo-night', () => 'dark')
    expect(theme.red).toBe('#f7768e')
    expect(theme.background).toBe('#1a1b26')
  })
})

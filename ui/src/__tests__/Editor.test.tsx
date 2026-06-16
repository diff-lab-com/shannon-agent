// Editor page tests.
//
// Phase E1 v2: auto-LSP diagnostics + manual squiggle UI. Mocks
// @tauri-apps/api and @/lib/tauri-api so we don't need a real Tauri runtime.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Editor from '@/pages/Editor'

const readSourceFile = vi.hoisted(() => vi.fn())
const runFileDiagnostics = vi.hoisted(() => vi.fn())
const defaultDiagnosticsServer = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri-api')>(
    '@/lib/tauri-api',
  )
  return {
    ...actual,
    readSourceFile: (...args: unknown[]) => readSourceFile(...args),
    runFileDiagnostics: (...args: unknown[]) => runFileDiagnostics(...args),
    defaultDiagnosticsServer: (...args: unknown[]) =>
      defaultDiagnosticsServer(...args),
  }
})

function renderEditor() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <Editor />
      </MemoryRouter>
    </I18nProvider>,
  )
}

const RUST_SERVER = { cmd: 'rust-analyzer', args: [] as string[] }

beforeEach(() => {
  readSourceFile.mockReset()
  runFileDiagnostics.mockReset()
  defaultDiagnosticsServer.mockReset()
  // Most tests load rust files; default to a working server config.
  defaultDiagnosticsServer.mockReturnValue(RUST_SERVER)
  runFileDiagnostics.mockResolvedValue({ diagnostics: [], timed_out: false })
})

describe('Editor page — Phase E1 v2', () => {
  it('renders the file path form on first load', () => {
    renderEditor()
    expect(screen.getByText(/Code Editor/i)).toBeInTheDocument()
    expect(screen.getByPlaceholderText(/abs\/path/i)).toBeInTheDocument()
    expect(
      screen.queryByRole('heading', { name: 'Diagnostics' }),
    ).not.toBeInTheDocument()
  })

  it('loads a file and shows the language id', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/src/lib.rs',
      content: 'fn main() {}\n',
      language_id: 'rust',
    })
    renderEditor()
    const input = screen.getByPlaceholderText(/abs\/path/i) as HTMLInputElement
    fireEvent.change(input, { target: { value: '/tmp/src/lib.rs' } })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await waitFor(() => expect(readSourceFile).toHaveBeenCalled())
    expect(await screen.findByText('rust')).toBeInTheDocument()
    expect(screen.getByText('lib.rs')).toBeInTheDocument()
  })

  it('auto-fetches LSP diagnostics after file load', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/src/lib.rs',
      content: 'fn main() {}\n',
      language_id: 'rust',
    })
    runFileDiagnostics.mockResolvedValue({
      diagnostics: [
        {
          start_line: 0,
          start_character: 0,
          end_line: 0,
          end_character: 8,
          message: 'unused function: main',
          severity: 'warning',
          source: 'rustc',
          code: 'dead_code',
        },
      ],
      timed_out: false,
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/src/lib.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))

    // runFileDiagnostics must be called with the right server + content
    await waitFor(() => expect(runFileDiagnostics).toHaveBeenCalledTimes(1))
    expect(runFileDiagnostics).toHaveBeenCalledWith(
      expect.objectContaining({
        file_path: '/tmp/src/lib.rs',
        server_cmd: 'rust-analyzer',
        language_id: 'rust',
        content: 'fn main() {}\n',
      }),
    )

    // Auto diagnostic shows in the list with source tag
    expect(await screen.findByText(/unused function: main/)).toBeInTheDocument()
    expect(screen.getByText('rustc')).toBeInTheDocument()
    expect(screen.getByText(/1 diagnostic/i)).toBeInTheDocument()
  })

  it('shows timeout banner when server exceeds deadline', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/src/lib.rs',
      content: 'fn main() {}\n',
      language_id: 'rust',
    })
    runFileDiagnostics.mockResolvedValue({
      diagnostics: [],
      timed_out: true,
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/src/lib.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    expect(
      await screen.findByText(/timed out/i),
    ).toBeInTheDocument()
  })

  it('shows error banner when runFileDiagnostics rejects', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/src/lib.rs',
      content: 'fn main() {}\n',
      language_id: 'rust',
    })
    runFileDiagnostics.mockRejectedValue('server crashed')
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/src/lib.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    expect(
      await screen.findByText(/Diagnostics failed/i),
    ).toBeInTheDocument()
  })

  it('skips diagnostics call when no default server for language', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/notes.md',
      content: '# hello\n',
      language_id: 'markdown',
    })
    defaultDiagnosticsServer.mockReturnValue({ cmd: '', args: [] })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/notes.md' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await waitFor(() => expect(readSourceFile).toHaveBeenCalled())
    // Wait a tick to ensure no diagnostic fetch fires
    await new Promise((r) => setTimeout(r, 10))
    expect(runFileDiagnostics).not.toHaveBeenCalled()
  })

  it('shows an error when read_source_file rejects', async () => {
    readSourceFile.mockRejectedValue('not a file: /no/such')
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/no/such' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await waitFor(() => expect(readSourceFile).toHaveBeenCalled())
    expect(await screen.findByText(/not a file/i)).toBeInTheDocument()
  })

  it('adds a squiggle via the manual form', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/foo.rs',
      content: 'let x = 1;\n',
      language_id: 'rust',
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/foo.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await screen.findByText('rust')

    fireEvent.change(screen.getByPlaceholderText(/unused variable/i), {
      target: { value: 'unused x' },
    })
    fireEvent.click(screen.getByRole('button', { name: /add squiggle/i }))

    expect(await screen.findByText(/unused x/)).toBeInTheDocument()
    expect(screen.getByText(/1 diagnostic/i)).toBeInTheDocument()
  })

  it('labels manual squiggles distinctly from auto diagnostics', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/foo.rs',
      content: 'let x = 1;\n',
      language_id: 'rust',
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/foo.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await screen.findByText('rust')
    fireEvent.change(screen.getByPlaceholderText(/unused variable/i), {
      target: { value: 'note: refactor me' },
    })
    fireEvent.click(screen.getByRole('button', { name: /add squiggle/i }))
    expect(await screen.findByText('manual')).toBeInTheDocument()
  })

  it('disables the add-squiggle button when message is empty', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/foo.rs',
      content: 'x\n',
      language_id: 'rust',
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/foo.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await screen.findByText('rust')
    const btn = screen.getByRole('button', { name: /add squiggle/i }) as HTMLButtonElement
    expect(btn.disabled).toBe(true)
  })

  it('opens quick-fix drawer when a diagnostic list item is clicked', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/foo.rs',
      content: 'let x = 1;\n',
      language_id: 'rust',
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/foo.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await screen.findByText('rust')
    fireEvent.change(screen.getByPlaceholderText(/unused variable/i), {
      target: { value: 'unused x' },
    })
    fireEvent.click(screen.getByRole('button', { name: /add squiggle/i }))
    const item = await screen.findByRole('button', { name: /unused x/ })
    fireEvent.click(item)
    expect(
      await screen.findByRole('dialog', { name: /quick fix drawer/i }),
    ).toBeInTheDocument()
  })

  it('re-runs diagnostics when re-run button clicked', async () => {
    readSourceFile.mockResolvedValue({
      path: '/tmp/foo.rs',
      content: 'let x = 1;\n',
      language_id: 'rust',
    })
    renderEditor()
    fireEvent.change(screen.getByPlaceholderText(/abs\/path/i), {
      target: { value: '/tmp/foo.rs' },
    })
    fireEvent.click(screen.getByRole('button', { name: /load file/i }))
    await waitFor(() => expect(runFileDiagnostics).toHaveBeenCalledTimes(1))

    fireEvent.click(screen.getByRole('button', { name: /re-run diagnostics/i }))
    await waitFor(() => expect(runFileDiagnostics).toHaveBeenCalledTimes(2))
  })
})

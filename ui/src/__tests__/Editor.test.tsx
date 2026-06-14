// Editor page tests.
//
// Phase E1: file loader + manual squiggle UI. Mocks @tauri-apps/api and
// @/lib/tauri-api so we don't need a real Tauri runtime or files on disk.

import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import Editor from '@/pages/Editor'

const readSourceFile = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri-api', () => ({
  readSourceFile: (...args: unknown[]) => readSourceFile(...args),
}))

function renderEditor() {
  return render(
    <MemoryRouter>
      <Editor />
    </MemoryRouter>,
  )
}

beforeEach(() => {
  readSourceFile.mockReset()
})

describe('Editor page', () => {
  it('renders the file path form on first load', () => {
    renderEditor()
    expect(screen.getByText(/Code Editor/i)).toBeInTheDocument()
    expect(screen.getByPlaceholderText(/abs\/path/i)).toBeInTheDocument()
    expect(screen.queryByText(/Diagnostics/i)).not.toBeInTheDocument()
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

    // Fill the manual squiggle form
    fireEvent.change(screen.getByPlaceholderText(/unused variable/i), {
      target: { value: 'unused x' },
    })
    fireEvent.click(screen.getByRole('button', { name: /add squiggle/i }))

    expect(await screen.findByText(/unused x/)).toBeInTheDocument()
    expect(screen.getByText(/1 diagnostic/)).toBeInTheDocument()
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
    expect(await screen.findByRole('dialog', { name: /quick fix drawer/i })).toBeInTheDocument()
  })
})

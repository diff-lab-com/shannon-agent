/**
 * §4 P0-B / §5-4 — FileRefChip's existence gating: a path the backend
 * confirms becomes an interactive chip routing through openFileRef; a
 * missing (or unprobeable) path degrades to plain inline code.
 */
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const pathExistsMock = vi.fn()

vi.mock('@/lib/tauri-api', () => ({
  pathExists: (...args: unknown[]) => pathExistsMock(...args),
  revealInFolder: vi.fn(),
  openWithDefaultApp: vi.fn(),
}))

vi.mock('@/lib/openFileRef', async (importOriginal) => {
  const actual = await importOriginal()
  return { ...actual, openFileRef: vi.fn(actual.openFileRef) }
})

import { FileRefChip } from '@/components/shared/FileRefChip'
import { setActiveWorkingDir } from '@/lib/fileRefs'
import { openFileRef } from '@/lib/openFileRef'

function renderChip(raw = 'src/main.rs') {
  return render(<FileRefChip raw={raw} />)
}

describe('FileRefChip', () => {
  beforeEach(() => {
    pathExistsMock.mockReset()
    setActiveWorkingDir('/proj')
  })

  afterEach(() => {
    setActiveWorkingDir(null)
  })

  it('renders as an interactive chip when the file exists and opens it on click', async () => {
    pathExistsMock.mockResolvedValue(true)
    const spy = vi.mocked(openFileRef)
    renderChip()
    const chip = await screen.findByTestId('file-ref-chip')
    expect(chip).toBeEnabled()
    fireEvent.click(chip)
    await waitFor(() => expect(spy).toHaveBeenCalledWith('/proj/src/main.rs'))
  })

  it('degrades to plain inline code when the file is missing', async () => {
    pathExistsMock.mockResolvedValue(false)
    const { container } = renderChip('ghost.rs')
    await waitFor(() => expect(pathExistsMock).toHaveBeenCalledWith('/proj/ghost.rs'))
    await waitFor(() => expect(screen.queryByTestId('file-ref-chip')).toBeNull())
    expect(container.querySelector('code')?.textContent).toBe('ghost.rs')
  })

  it('degrades to plain inline code when the probe fails', async () => {
    pathExistsMock.mockRejectedValue(new Error('bridge down'))
    const { container } = renderChip('src/flaky.rs')
    await waitFor(() => expect(screen.queryByTestId('file-ref-chip')).toBeNull())
    expect(container.querySelector('code')?.textContent).toBe('src/flaky.rs')
  })

  it('never probes non-path tokens', async () => {
    render(<FileRefChip raw="just some text" />)
    await waitFor(() => expect(screen.queryByTestId('file-ref-chip')).toBeNull())
    expect(pathExistsMock).not.toHaveBeenCalled()
  })

  it('gives the context menu full keyboard semantics (review §5)', async () => {
    pathExistsMock.mockResolvedValue(true)
    renderChip()
    const chip = await screen.findByTestId('file-ref-chip')
    fireEvent.contextMenu(chip)
    const menu = await screen.findByRole('menu')
    const items = within(menu).getAllByRole('menuitem')
    expect(items).toHaveLength(3)

    // Focus moves to the first item on open.
    await waitFor(() => expect(items[0]).toHaveFocus())

    // ArrowDown cycles forward…
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(items[1]).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(items[2]).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    expect(items[0]).toHaveFocus()
    // …ArrowUp cycles backward…
    fireEvent.keyDown(menu, { key: 'ArrowUp' })
    expect(items[2]).toHaveFocus()
    // …and Home/End jump.
    fireEvent.keyDown(menu, { key: 'Home' })
    expect(items[0]).toHaveFocus()
    fireEvent.keyDown(menu, { key: 'End' })
    expect(items[2]).toHaveFocus()

    // Escape closes the menu.
    fireEvent.keyDown(menu, { key: 'Escape' })
    expect(screen.queryByRole('menu')).toBeNull()
  })
})

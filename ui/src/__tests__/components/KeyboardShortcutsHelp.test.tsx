import { describe, it, expect, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import KeyboardShortcutsHelp from '@/components/KeyboardShortcutsHelp'
import { I18nProvider } from '@/i18n'

function renderHelp(open = true) {
  const onClose = vi.fn()
  const utils = render(
    <I18nProvider>
      <KeyboardShortcutsHelp open={open} onClose={onClose} />
    </I18nProvider>,
  )
  return { onClose, ...utils }
}

describe('KeyboardShortcutsHelp', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('renders nothing when open is false', () => {
    renderHelp(false)
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument()
  })

  it('renders the dialog with title when open', () => {
    renderHelp()
    expect(screen.getByRole('dialog')).toBeInTheDocument()
    expect(screen.getByText('Keyboard Shortcuts')).toBeInTheDocument()
  })

  it('renders grouped sections', () => {
    renderHelp()
    expect(screen.getByText('Global')).toBeInTheDocument()
    expect(screen.getByText('Navigation')).toBeInTheDocument()
    expect(screen.getByText('Chat')).toBeInTheDocument()
    expect(screen.getByText('Diff Review')).toBeInTheDocument()
  })

  it('includes the Plan Mode shortcut under Chat section', () => {
    renderHelp()
    expect(screen.getByText('Toggle Plan mode')).toBeInTheDocument()
  })

  it('includes the Artifact cycle shortcut under Chat section', () => {
    renderHelp()
    expect(screen.getByText('Cycle active artifact in panel')).toBeInTheDocument()
  })

  it('includes diff-review shortcuts', () => {
    renderHelp()
    expect(screen.getAllByText('Next diff hunk').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Accept current hunk').length).toBeGreaterThan(0)
    expect(screen.getAllByText('Apply accepted hunks').length).toBeGreaterThan(0)
  })

  it('renders the search input', () => {
    renderHelp()
    expect(screen.getByPlaceholderText('Search shortcuts')).toBeInTheDocument()
  })

  it('filters shortcuts by query', () => {
    renderHelp()
    const input = screen.getByPlaceholderText('Search shortcuts') as HTMLInputElement
    expect(screen.getByText('Toggle sidebar')).toBeInTheDocument()
    fireEvent.change(input, { target: { value: 'plan' } })
    expect(screen.queryByText('Toggle sidebar')).not.toBeInTheDocument()
    expect(screen.getByText('Toggle Plan mode')).toBeInTheDocument()
  })

  it('shows no-results message when query matches nothing', () => {
    renderHelp()
    const input = screen.getByPlaceholderText('Search shortcuts') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'zzzznotreal' } })
    expect(screen.getByText('No matching shortcuts')).toBeInTheDocument()
  })

  it('calls onClose when close button is clicked', () => {
    const { onClose } = renderHelp()
    fireEvent.click(screen.getByLabelText('Close'))
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('calls onClose when backdrop is clicked', () => {
    const { onClose } = renderHelp()
    const backdrop = screen.getByRole('dialog').parentElement!
    fireEvent.click(backdrop)
    expect(onClose).toHaveBeenCalledTimes(1)
  })
})

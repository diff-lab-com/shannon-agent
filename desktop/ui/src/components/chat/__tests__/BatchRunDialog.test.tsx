// office Wave 3 C2 — BatchRunDialog tests: render / edit / Build pushes the
// batch draft (never sends) / cancel pushes nothing / blank instruction
// keeps Build disabled.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import BatchRunDialog, { batchDraftOf } from '@/components/chat/BatchRunDialog'
import { COMPOSER_DRAFT_EVENT } from '@/lib/composerBridge'

const CSV = { path: '/tmp/shannon/inventory.csv', name: 'inventory.csv' }

function renderDialog(props: Partial<Parameters<typeof BatchRunDialog>[0]> = {}) {
  const onClose = vi.fn()
  const utils = render(<BatchRunDialog open {...CSV} onClose={onClose} {...props} />)
  return { onClose, ...utils }
}

/** Collect pushed drafts while `run` executes. */
function collectPushes(run: () => void): string[] {
  const texts: string[] = []
  const handler = (e: Event) => texts.push((e as CustomEvent<{ text: string }>).detail.text)
  window.addEventListener(COMPOSER_DRAFT_EVENT, handler)
  try {
    run()
  } finally {
    window.removeEventListener(COMPOSER_DRAFT_EVENT, handler)
  }
  return texts
}

describe('BatchRunDialog', () => {
  it('renders the title, the instruction textarea and the hint', () => {
    renderDialog()
    expect(screen.getByRole('heading', { name: 'Batch run over table rows' })).toBeInTheDocument()
    expect(screen.getByTestId('batch-instruction-input')).toBeInTheDocument()
    expect(screen.getByText(/apply this instruction row by row/i)).toBeInTheDocument()
  })

  it('starts with an empty instruction and re-arms on reopen', () => {
    const { rerender } = renderDialog()
    const input = screen.getByTestId('batch-instruction-input') as HTMLTextAreaElement
    fireEvent.change(input, { target: { value: 'translate the name column' } })
    expect(input).toHaveValue('translate the name column')

    // Close + reopen — the stale instruction must not survive.
    rerender(<BatchRunDialog open={false} {...CSV} onClose={vi.fn()} />)
    rerender(<BatchRunDialog open {...CSV} onClose={vi.fn()} />)
    expect(screen.getByTestId('batch-instruction-input')).toHaveValue('')
  })

  it('Build pushes a draft containing the path and the instruction, then closes — never sends', () => {
    const { onClose } = renderDialog()
    fireEvent.change(screen.getByTestId('batch-instruction-input'), {
      target: { value: 'Classify each row into a category column' },
    })

    const texts = collectPushes(() =>
      fireEvent.click(screen.getByTestId('batch-build-prompt')),
    )
    expect(texts).toEqual([
      batchDraftOf(CSV.path, CSV.name, 'Classify each row into a category column'),
    ])
    expect(texts[0]).toContain('/tmp/shannon/inventory.csv')
    expect(texts[0]).toContain('inventory-enriched.csv')
    expect(texts[0]).toContain('Classify each row into a category column')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('cancel (close button) closes without pushing anything', () => {
    const { onClose } = renderDialog()
    fireEvent.change(screen.getByTestId('batch-instruction-input'), {
      target: { value: 'would-be instruction' },
    })
    const texts = collectPushes(() => fireEvent.click(screen.getByRole('button', { name: /close/i })))
    expect(texts).toEqual([])
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('Build is disabled while the instruction is blank', () => {
    renderDialog()
    expect(screen.getByTestId('batch-build-prompt')).toBeDisabled()
    fireEvent.change(screen.getByTestId('batch-instruction-input'), { target: { value: '   ' } })
    expect(screen.getByTestId('batch-build-prompt')).toBeDisabled()
  })

  it('batchDraftOf derives the enriched file name from the card name and path', () => {
    expect(batchDraftOf('/a/b/sales.xlsx', 'sales.xlsx', 'do X')).toContain('sales-enriched.xlsx')
    expect(batchDraftOf('/a/b/t', 't', 'do X')).toContain('t-enriched.csv')
  })
})

// office Wave 2 B2 v1 — PptOutlineDialog tests: render / edit / Generate
// pushes the outline draft (never sends) / cancel pushes nothing.

import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import PptOutlineDialog, { PPT_DRAFT_PREFIX, SAMPLE_PPT_OUTLINE, pptDraftOf } from '@/components/chat/PptOutlineDialog'
import { COMPOSER_DRAFT_EVENT } from '@/lib/composerBridge'

function renderDialog(props: Partial<Parameters<typeof PptOutlineDialog>[0]> = {}) {
  const onClose = vi.fn()
  const utils = render(<PptOutlineDialog open onClose={onClose} {...props} />)
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

describe('PptOutlineDialog', () => {
  it('renders the title and a textarea pre-filled with the 6-line sample outline', () => {
    renderDialog()
    expect(screen.getByRole('heading', { name: 'Build a presentation' })).toBeInTheDocument()
    const input = screen.getByTestId('ppt-outline-input') as HTMLTextAreaElement
    expect(input.value).toBe(SAMPLE_PPT_OUTLINE)
    expect(input.value.split('\n').filter(Boolean)).toHaveLength(6)
    expect(screen.getByText(/each non-empty line becomes one slide/i)).toBeInTheDocument()
  })

  it('lets the user edit the outline', () => {
    renderDialog()
    const input = screen.getByTestId('ppt-outline-input')
    fireEvent.change(input, { target: { value: 'Slide A\nSlide B' } })
    expect(input).toHaveValue('Slide A\nSlide B')
  })

  it('Generate pushes a composer draft containing the prefix and the outline, then closes — never sends', () => {
    const { onClose } = renderDialog()
    const input = screen.getByTestId('ppt-outline-input')
    fireEvent.change(input, { target: { value: 'Intro\nDetails' } })

    const texts = collectPushes(() => fireEvent.click(screen.getByRole('button', { name: 'Generate with agent' })))
    expect(texts).toEqual([pptDraftOf('Intro\nDetails')])
    expect(texts[0]).toContain(PPT_DRAFT_PREFIX)
    expect(texts[0]).toContain('Intro\nDetails')
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('cancel (close button) closes without pushing anything', () => {
    const { onClose } = renderDialog()
    const texts = collectPushes(() => fireEvent.click(screen.getByRole('button', { name: /close/i })))
    expect(texts).toEqual([])
    expect(onClose).toHaveBeenCalledTimes(1)
  })

  it('Generate is disabled when the outline is blank', () => {
    renderDialog()
    const input = screen.getByTestId('ppt-outline-input')
    fireEvent.change(input, { target: { value: '   ' } })
    expect(screen.getByRole('button', { name: 'Generate with agent' })).toBeDisabled()
  })
})

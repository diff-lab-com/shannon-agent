// B2 P1-7: tool-card headers were click-only (role="button" tabIndex=0 with
// an onClick) — keyboard users could not expand/collapse any tool card. The
// header now activates on Enter/Space like a real button.

import { describe, it, expect, vi } from 'vitest'
import { render, fireEvent } from '@testing-library/react'
import { ToolHeader } from '@/components/ai-elements'

describe('ToolHeader keyboard operability', () => {
  it('activates on Enter', () => {
    const onClick = vi.fn()
    const { container } = render(<ToolHeader onClick={onClick}>header</ToolHeader>)
    fireEvent.keyDown(container.firstElementChild!, { key: 'Enter' })
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('activates on Space', () => {
    const onClick = vi.fn()
    const { container } = render(<ToolHeader onClick={onClick}>header</ToolHeader>)
    fireEvent.keyDown(container.firstElementChild!, { key: ' ' })
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('ignores unrelated keys', () => {
    const onClick = vi.fn()
    const { container } = render(<ToolHeader onClick={onClick}>header</ToolHeader>)
    fireEvent.keyDown(container.firstElementChild!, { key: 'ArrowDown' })
    fireEvent.keyDown(container.firstElementChild!, { key: 'Escape' })
    expect(onClick).not.toHaveBeenCalled()
  })

  it('keeps click activation', () => {
    const onClick = vi.fn()
    const { container } = render(<ToolHeader onClick={onClick}>header</ToolHeader>)
    fireEvent.click(container.firstElementChild!)
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('stays inert without an onClick (no button semantics)', () => {
    const { container } = render(<ToolHeader>static header</ToolHeader>)
    const el = container.firstElementChild!
    expect(el.getAttribute('role')).toBeNull()
    expect(el.getAttribute('tabindex')).toBeNull()
    fireEvent.keyDown(el, { key: 'Enter' })
  })

  it('keeps the button role and focusability', () => {
    const onClick = vi.fn()
    const { container } = render(<ToolHeader onClick={onClick}>header</ToolHeader>)
    const el = container.firstElementChild!
    expect(el.getAttribute('role')).toBe('button')
    expect(el.getAttribute('tabindex')).toBe('0')
  })
})

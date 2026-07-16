import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { Tooltip } from '@/components/ui/tooltip'

describe('Tooltip', () => {
  it('does not show content immediately on hover', () => {
    render(
      <Tooltip content="Helpful tip" delay={300}>
        <button>Hover me</button>
      </Tooltip>
    )
    fireEvent.mouseEnter(screen.getByText('Hover me'))
    expect(screen.queryByText('Helpful tip')).not.toBeInTheDocument()
  })

  it('shows content after delay', async () => {
    render(
      <Tooltip content="tip" delay={0}>
        <button>Hover me</button>
      </Tooltip>
    )
    fireEvent.mouseEnter(screen.getByText('Hover me'))
    await waitFor(() => expect(screen.getByText('tip')).toBeInTheDocument())
  })

  it('hides on mouse leave', async () => {
    render(
      <Tooltip content="tip" delay={0}>
        <button>Hover me</button>
      </Tooltip>
    )
    fireEvent.mouseEnter(screen.getByText('Hover me'))
    await waitFor(() => expect(screen.getByText('tip')).toBeInTheDocument())
    fireEvent.mouseLeave(screen.getByText('Hover me'))
    await waitFor(() => expect(screen.queryByText('tip')).not.toBeInTheDocument())
  })

  it('hides on blur', async () => {
    render(
      <Tooltip content="tip" delay={0}>
        <button>Hover me</button>
      </Tooltip>
    )
    fireEvent.focus(screen.getByText('Hover me'))
    await waitFor(() => expect(screen.getByText('tip')).toBeInTheDocument())
    fireEvent.blur(screen.getByText('Hover me'))
    await waitFor(() => expect(screen.queryByText('tip')).not.toBeInTheDocument())
  })

  it('sets role=tooltip on the content', async () => {
    render(
      <Tooltip content="tip" delay={0}>
        <button>x</button>
      </Tooltip>
    )
    fireEvent.mouseEnter(screen.getByText('x'))
    await waitFor(() => expect(screen.getByRole('tooltip')).toBeInTheDocument())
  })

  it('applies side classes', async () => {
    render(
      <Tooltip content="tip" delay={0} side="bottom">
        <button>x</button>
      </Tooltip>
    )
    fireEvent.mouseEnter(screen.getByText('x'))
    await waitFor(() => expect(screen.getByRole('tooltip')).toBeInTheDocument())
    expect(screen.getByRole('tooltip').className).toContain('top-full')
  })
})

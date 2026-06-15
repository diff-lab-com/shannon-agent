import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import WelcomeState from '@/components/WelcomeState'

describe('WelcomeState', () => {
  it('renders hero heading and subtitle', () => {
    render(<WelcomeState onSelectPrompt={() => {}} />)
    expect(screen.getByText('What can I help with?')).toBeInTheDocument()
    expect(screen.getByText(/Pick a starting point or type your own/)).toBeInTheDocument()
  })

  it('renders exactly 4 template cards', () => {
    render(<WelcomeState onSelectPrompt={() => {}} />)
    expect(screen.getByText('Draft an email')).toBeInTheDocument()
    expect(screen.getByText('Summarize')).toBeInTheDocument()
    expect(screen.getByText('Research')).toBeInTheDocument()
    expect(screen.getByText('Write code')).toBeInTheDocument()
  })

  it('calls onSelectPrompt with email prompt when email card clicked', () => {
    const spy = vi.fn()
    render(<WelcomeState onSelectPrompt={spy} />)
    fireEvent.click(screen.getByText('Draft an email'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/follow-up email/i)
  })

  it('calls onSelectPrompt with summary prompt when summary card clicked', () => {
    const spy = vi.fn()
    render(<WelcomeState onSelectPrompt={spy} />)
    fireEvent.click(screen.getByText('Summarize'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/5 bullet points/i)
  })

  it('calls onSelectPrompt with research prompt when research card clicked', () => {
    const spy = vi.fn()
    render(<WelcomeState onSelectPrompt={spy} />)
    fireEvent.click(screen.getByText('Research'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/Rust web frameworks/i)
  })

  it('calls onSelectPrompt with code prompt when code card clicked', () => {
    const spy = vi.fn()
    render(<WelcomeState onSelectPrompt={spy} />)
    fireEvent.click(screen.getByText('Write code'))
    expect(spy).toHaveBeenCalledTimes(1)
    expect(spy.mock.calls[0][0]).toMatch(/REST API endpoint in Rust/i)
  })

  it('shows keyboard hint chips', () => {
    render(<WelcomeState onSelectPrompt={() => {}} />)
    expect(screen.getByText('Commands')).toBeInTheDocument()
    expect(screen.getByText('Shortcuts')).toBeInTheDocument()
    expect(screen.getByText('History')).toBeInTheDocument()
  })
})

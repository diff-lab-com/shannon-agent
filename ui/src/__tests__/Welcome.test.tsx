import { describe, it, expect, beforeEach, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { MemoryRouter } from 'react-router-dom'
import Welcome, { shouldShowWelcome, markWelcomeSeen, WELCOME_SEEN_KEY } from '@/pages/Welcome'

describe('shouldShowWelcome', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('returns false while config is still loading', () => {
    expect(shouldShowWelcome(true, false)).toBe(false)
  })

  it('returns true when not loading, no provider, no seen flag', () => {
    expect(shouldShowWelcome(false, false)).toBe(true)
  })

  it('returns false when provider is already configured', () => {
    expect(shouldShowWelcome(false, true)).toBe(false)
  })

  it('returns false when seen flag is set even without provider (skip path)', () => {
    window.localStorage.setItem(WELCOME_SEEN_KEY, '1')
    expect(shouldShowWelcome(false, false)).toBe(false)
  })
})

describe('markWelcomeSeen', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  it('writes the seen flag to localStorage', () => {
    markWelcomeSeen()
    expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
  })
})

describe('Welcome component', () => {
  beforeEach(() => {
    window.localStorage.clear()
  })

  function wrap() {
    return render(
      <AppProvider>
        <MemoryRouter>
          <Welcome />
        </MemoryRouter>
      </AppProvider>
    )
  }

  it('renders provider picker as step 1', () => {
    wrap()
    expect(screen.getByText('Choose your AI provider')).toBeInTheDocument()
    expect(screen.getByText('Anthropic')).toBeInTheDocument()
    expect(screen.getByText('OpenAI')).toBeInTheDocument()
    expect(screen.getByText('Ollama')).toBeInTheDocument()
    expect(screen.getByText('DeepSeek')).toBeInTheDocument()
  })

  it('shows API key field for non-Ollama providers', () => {
    wrap()
    expect(screen.getByLabelText('API key')).toBeInTheDocument()
  })

  it('hides API key field when Ollama is selected', () => {
    wrap()
    fireEvent.click(screen.getByText('Ollama'))
    expect(screen.queryByLabelText('API key')).not.toBeInTheDocument()
  })

  it('shows Skip button that marks welcome seen', () => {
    wrap()
    const skip = screen.getByRole('button', { name: /skip welcome/i })
    fireEvent.click(skip)
    expect(window.localStorage.getItem(WELCOME_SEEN_KEY)).toBe('1')
  })
})

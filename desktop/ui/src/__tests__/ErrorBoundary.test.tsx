import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { ErrorBoundary } from '@/components/ErrorBoundary'

function ThrowError({ error }: { error: Error }) {
  throw error
}

describe('ErrorBoundary', () => {
  // Suppress console.error for expected errors
  const originalError = console.error
  beforeEach(() => { console.error = vi.fn() })
  afterEach(() => { console.error = originalError })

  it('renders children when no error', () => {
    render(
      <I18nProvider>
        <ErrorBoundary>
          <div>Content</div>
        </ErrorBoundary>
      </I18nProvider>
    )
    expect(screen.getByText('Content')).toBeInTheDocument()
  })

  it('renders error UI when child throws', () => {
    render(
      <I18nProvider>
        <ErrorBoundary>
          <ThrowError error={new Error('Test error message')} />
        </ErrorBoundary>
      </I18nProvider>
    )
    expect(screen.getByText('Something went wrong')).toBeInTheDocument()
    expect(screen.getByText('Test error message')).toBeInTheDocument()
  })

  it('renders custom fallback when provided', () => {
    render(
      <I18nProvider>
        <ErrorBoundary fallback={<div>Custom fallback</div>}>
          <ThrowError error={new Error('boom')} />
        </ErrorBoundary>
      </I18nProvider>
    )
    expect(screen.getByText('Custom fallback')).toBeInTheDocument()
  })

  it('shows try again button', () => {
    render(
      <I18nProvider>
        <ErrorBoundary>
          <ThrowError error={new Error('fail')} />
        </ErrorBoundary>
      </I18nProvider>
    )
    expect(screen.getByText('Try Again')).toBeInTheDocument()
  })
})

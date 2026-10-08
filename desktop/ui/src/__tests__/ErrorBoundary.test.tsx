import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
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

  // R3-V-01 — pane mode: the Settings shell renders one boundary per pane.
  // The raw exception string must NOT be the pane's headline; it stays in
  // the DOM but folded into a collapsed 「技术详情」<details> block.
  describe('pane mode (Settings sub-page boundary)', () => {
    it('renders the friendly title + description and retry, not the raw message', () => {
      render(
        <I18nProvider>
          <ErrorBoundary pane>
            <ThrowError error={new Error('TypeError: cannot read properties of undefined')} />
          </ErrorBoundary>
        </I18nProvider>
      )
      expect(screen.getByText('This section failed to load')).toBeInTheDocument()
      expect(
        screen.getByText(/The rest of Settings is unaffected/),
      ).toBeInTheDocument()
      expect(screen.getByTestId('pane-error-retry')).toHaveTextContent('Retry')
      // No app-level buttons in pane mode.
      expect(screen.queryByText('Try Again')).not.toBeInTheDocument()
      expect(screen.queryByText('Reload Page')).not.toBeInTheDocument()
    })

    it('folds the raw message into a collapsed technical-details block', () => {
      render(
        <I18nProvider>
          <ErrorBoundary pane>
            <ThrowError error={new Error('boom: raw stack line')} />
          </ErrorBoundary>
        </I18nProvider>
      )
      const details = screen.getByTestId('pane-error-details')
      // Collapsed by default — the raw string is one disclosure away, never
      // rendered bare as the pane body.
      expect(details).not.toHaveAttribute('open')
      expect(details).toHaveTextContent('Technical details')
      expect(details).toHaveTextContent('boom: raw stack line')
      // …and the raw string appears nowhere outside the details block.
      expect(screen.getByTestId('pane-error').textContent).toContain('boom: raw stack line')
      expect(screen.queryByText(/boom: raw stack line/, { selector: 'p' })).not.toBeInTheDocument()
    })

    it('retry resets the boundary and remounts the children', () => {
      let shouldThrow = true
      function CrashOnce() {
        if (shouldThrow) throw new Error('first mount fails')
        return <div>pane recovered</div>
      }
      const { rerender } = render(
        <I18nProvider>
          <ErrorBoundary pane>
            <CrashOnce />
          </ErrorBoundary>
        </I18nProvider>
      )
      expect(screen.getByTestId('pane-error')).toBeInTheDocument()

      shouldThrow = false
      fireEvent.click(screen.getByTestId('pane-error-retry'))
      rerender(
        <I18nProvider>
          <ErrorBoundary pane>
            <CrashOnce />
          </ErrorBoundary>
        </I18nProvider>
      )
      expect(screen.getByText('pane recovered')).toBeInTheDocument()
      expect(screen.queryByTestId('pane-error')).not.toBeInTheDocument()
    })
  })
})

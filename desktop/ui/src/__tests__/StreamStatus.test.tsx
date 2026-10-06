// B2 P2-17 (§4-17): the single aria-live region for run state transitions.
// The streaming log used to announce every token (whole-log
// aria-live=polite); now only "generating" on start and "reply complete" on
// end reach screen readers, via this region in MessageArea.

import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { StreamStatusRegion } from '@/pages/chat/MessageArea'

function renderRegion(active: boolean) {
  // Inline wrapper (not the `wrapper` option): rerender() re-applies the
  // wrapper option, which would double-nest the provider on the first
  // rerender and remount the region under test.
  return render(
    <I18nProvider>
      <StreamStatusRegion active={active} />
    </I18nProvider>,
  )
}

describe('StreamStatusRegion', () => {
  it('announces nothing while idle', () => {
    renderRegion(false)
    expect(screen.getByTestId('stream-status-region')).toBeEmptyDOMElement()
  })

  it('announces the generating state when a run starts', () => {
    const view = renderRegion(false)
    view.rerender(
      <I18nProvider>
        <StreamStatusRegion active />
      </I18nProvider>,
    )
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Generating reply…')
  })

  it('announces completion when a run ends', () => {
    const view = renderRegion(true)
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Generating reply…')
    view.rerender(
      <I18nProvider>
        <StreamStatusRegion active={false} />
      </I18nProvider>,
    )
    // Transitions strictly alternate, so this write is always a fresh text
    // change the polite live region announces.
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Reply complete')
  })

  it('keeps announcing start/done across consecutive runs', () => {
    const view = renderRegion(false)
    view.rerender(
      <I18nProvider>
        <StreamStatusRegion active />
      </I18nProvider>,
    )
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Generating reply…')
    view.rerender(
      <I18nProvider>
        <StreamStatusRegion active={false} />
      </I18nProvider>,
    )
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Reply complete')
    // A new run re-announces the active state over the "done" text.
    view.rerender(
      <I18nProvider>
        <StreamStatusRegion active />
      </I18nProvider>,
    )
    expect(screen.getByTestId('stream-status-region')).toHaveTextContent('Generating reply…')
  })

  it('exposes a polite status live region', () => {
    renderRegion(false)
    const region = screen.getByTestId('stream-status-region')
    expect(region).toHaveAttribute('role', 'status')
    expect(region).toHaveAttribute('aria-live', 'polite')
  })
})

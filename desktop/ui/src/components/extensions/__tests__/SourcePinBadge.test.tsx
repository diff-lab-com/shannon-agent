import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'

import { SourcePinBadge } from '../SourcePinBadge'
import { IntlProvider } from 'react-intl'
import messages from '@/i18n/locales/en.json'

function renderBadge(sourceCommit: string | null, refFloating: boolean) {
  return render(
    <IntlProvider locale="en" messages={messages}>
      <SourcePinBadge sourceCommit={sourceCommit} refFloating={refFloating} />
    </IntlProvider>,
  )
}

describe('SourcePinBadge', () => {
  it('renders nothing for a pinned install without a commit (non-git source)', () => {
    const { container } = renderBadge(null, false)
    expect(container).toBeEmptyDOMElement()
  })

  it('renders the floating warning chip with its explanatory tooltip', () => {
    renderBadge(null, true)
    const chip = screen.getByTestId('source-pin-floating')
    expect(chip).toHaveTextContent('Floating ref')
    expect(chip).toHaveAttribute('title', expect.stringContaining('mutable branch'))
  })

  it('shows the short SHA for pinned installs', () => {
    renderBadge('9f2c1de77a04b5c8d3e6f0a1b2c3d4e5f6071829', false)
    expect(screen.getByTestId('source-pin')).toHaveTextContent('@9f2c1de')
    expect(screen.queryByTestId('source-pin-floating')).not.toBeInTheDocument()
  })

  it('renders both the floating chip and the SHA for a floating repo install', () => {
    renderBadge('c47d8e21fa90b6c5d3e2f1a0b9c8d7e6f5a4b3c2', true)
    expect(screen.getByTestId('source-pin-floating')).toBeInTheDocument()
    expect(screen.getByTestId('source-pin')).toHaveTextContent('@c47d8e2')
  })
})

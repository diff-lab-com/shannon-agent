// S3-1 — the shared picker row: source badge (declared/overlay/catalog),
// why-active badge (session/tier/profile/global), vision/tools marks and the
// context·price meta. One renderer feeds the composer chip AND the Header
// selector, so these tests pin the parity structurally.

import { describe, expect, it } from 'vitest'
import { render, screen } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import {
  ModelPickerRowContent,
  ModelSourceBadge,
  ModelWhyBadge,
} from '@/components/shared/ModelPickerRow'
import type { ModelInfo } from '@/types'
import type { ModelWhy } from '@/lib/modelWhy'

function row(over: Partial<ModelInfo> = {}): ModelInfo {
  return {
    id: 'm-1',
    name: 'Model One',
    provider: 'anthropic',
    context_window: 200000,
    price_in: 3,
    price_out: 15,
    vision: true,
    ...over,
  }
}

function renderRow(ui: React.ReactElement) {
  return render(<I18nProvider>{ui}</I18nProvider>)
}

describe('ModelSourceBadge', () => {
  it('renders the curated-vault badge for declared rows', () => {
    renderRow(<ModelSourceBadge source="declared" />)
    expect(screen.getByTestId('source-badge-declared')).toHaveTextContent('Curated')
  })

  it('renders the models.dev badge for overlay rows', () => {
    renderRow(<ModelSourceBadge source="overlay" />)
    expect(screen.getByTestId('source-badge-overlay')).toHaveTextContent('models.dev')
  })

  it('catalog (and unknown) rows stay unbadged', () => {
    renderRow(
      <span>
        <ModelSourceBadge source="catalog" />
        <ModelSourceBadge source={null} />
        <ModelSourceBadge source={undefined} />
      </span>,
    )
    expect(screen.queryByTestId('source-badge-declared')).toBeNull()
    expect(screen.queryByTestId('source-badge-overlay')).toBeNull()
  })
})

describe('ModelWhyBadge', () => {
  const cases: Array<[ModelWhy | null, string | null, string]> = [
    [{ kind: 'session' }, 'why-badge-session', 'Session override active'],
    [{ kind: 'tier', phase: 'plan' }, 'why-badge-tier', 'Plan tier active'],
    [{ kind: 'tier', phase: 'act' }, 'why-badge-tier', 'Act tier active'],
    [{ kind: 'profile', profile: 'lab' }, 'why-badge-profile', 'Pinned by profile lab'],
    [{ kind: 'global' }, 'why-badge-global', 'Global default'],
    [null, null, ''],
  ]
  it.each(cases)('%j', (why, testid, label) => {
    const { unmount } = renderRow(<ModelWhyBadge why={why} />)
    if (testid == null) {
      expect(document.querySelector('[data-testid^="why-badge-"]')).toBeNull()
    } else {
      expect(screen.getByTestId(testid)).toHaveTextContent(label)
    }
    unmount()
  })
})

describe('ModelPickerRowContent', () => {
  it('renders name, vision mark, badges and the context·price meta', () => {
    renderRow(
      <ModelPickerRowContent
        model={row({ source: 'declared', tools: true })}
        why={{ kind: 'session' }}
      />,
    )
    expect(screen.getByText('Model One')).toBeInTheDocument()
    // R2-3 meta contract: 200k context + per-million prices.
    expect(screen.getByText('200k · $3.00/$15.00')).toBeInTheDocument()
    expect(screen.getByLabelText('Vision input')).toBeInTheDocument()
    expect(screen.getByLabelText('Tool calling')).toBeInTheDocument()
    expect(screen.getByTestId('source-badge-declared')).toBeInTheDocument()
    expect(screen.getByTestId('why-badge-session')).toBeInTheDocument()
  })

  it('compact rows (Header menu) skip the tools mark but keep the rest', () => {
    renderRow(
      <ModelPickerRowContent
        model={row({ source: 'overlay', tools: true })}
        why={{ kind: 'global' }}
        compact
      />,
    )
    expect(screen.queryByLabelText('Tool calling')).toBeNull()
    expect(screen.getByLabelText('Vision input')).toBeInTheDocument()
    expect(screen.getByTestId('source-badge-overlay')).toBeInTheDocument()
    expect(screen.getByTestId('why-badge-global')).toBeInTheDocument()
  })

  it('unknown capability and absent badges render nothing (honest metadata)', () => {
    renderRow(
      <ModelPickerRowContent model={row({ vision: null, tools: null, source: 'catalog' })} why={null} />,
    )
    expect(screen.queryByLabelText('Vision input')).toBeNull()
    expect(screen.queryByLabelText('Tool calling')).toBeNull()
    expect(document.querySelector('[data-testid^="source-badge-"]')).toBeNull()
    expect(document.querySelector('[data-testid^="why-badge-"]')).toBeNull()
  })
})

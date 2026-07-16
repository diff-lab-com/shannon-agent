import { describe, it, expect, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { MemoryRouter, Routes, Route, useLocation } from 'react-router-dom'
import { I18nProvider } from '@/i18n'
import Extensions from '@/pages/Extensions'

function renderWithRoute(path: string) {
  return render(
    <I18nProvider>
      <MemoryRouter initialEntries={[path]}>
        <Routes>
          <Route path="/*" element={<Extensions />} />
        </Routes>
      </MemoryRouter>
    </I18nProvider>
  )
}

describe('Extensions', () => {
  it('renders default search placeholder', () => {
    renderWithRoute('/extensions')
    expect(screen.getByPlaceholderText('Search extensions...')).toBeInTheDocument()
  })

  it('does not show CTA on default extensions route', () => {
    renderWithRoute('/extensions')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })

  it('renders agents search placeholder on agents route', () => {
    renderWithRoute('/extensions/agents')
    expect(screen.getByPlaceholderText('Search agents...')).toBeInTheDocument()
  })

  // P0-5: "Create Agent" and "Add Source" CTAs were removed because they
  // navigated to the current page (no-op). Install/create flows live inside
  // each tab's content instead.
  it('does not show dead Create Agent CTA on agents route', () => {
    renderWithRoute('/extensions/agents')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
  })

  it('renders datasources search placeholder on datasources route', () => {
    renderWithRoute('/extensions/datasources')
    expect(screen.getByPlaceholderText('Search data sources...')).toBeInTheDocument()
  })

  it('does not show dead Add Source CTA on datasources route', () => {
    renderWithRoute('/extensions/datasources')
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })

  // US-EXT-05: Search Extensions
  it('updates search input value on typing', () => {
    renderWithRoute('/extensions')
    const input = screen.getByPlaceholderText('Search extensions...') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'my query' } })
    expect(input.value).toBe('my query')
  })

  it('updates search on agents route', () => {
    renderWithRoute('/extensions/agents')
    const input = screen.getByPlaceholderText('Search agents...') as HTMLInputElement
    fireEvent.change(input, { target: { value: 'agent1' } })
    expect(input.value).toBe('agent1')
  })

  // CTA button navigation — removed in P0-5 (CTAs were no-ops).
  // The search input is now the only top-bar widget on all sub-routes.
})

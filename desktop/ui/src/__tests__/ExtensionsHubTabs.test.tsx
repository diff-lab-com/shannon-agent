import { describe, it, expect, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
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

describe('Extensions hub sub-tabs (P1)', () => {
  it('renders all 7 sub-tabs', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.getByText('Featured')).toBeInTheDocument()
    expect(screen.getByText('MCP Servers')).toBeInTheDocument()
    expect(screen.getByText('Skills')).toBeInTheDocument()
    expect(screen.getByText('Agents')).toBeInTheDocument()
    expect(screen.getByText('Data Sources')).toBeInTheDocument()
    expect(screen.getByText('Plugins')).toBeInTheDocument()
    expect(screen.getByText('Installed')).toBeInTheDocument()
  })

  it('still renders default search placeholder on featured route', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.getByPlaceholderText('Search extensions...')).toBeInTheDocument()
  })

  it('does not show dead CTA on featured route', () => {
    renderWithRoute('/extensions/featured')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })

  // P0-5: the "Create Agent" and "Add Source" CTAs used to navigate to the
  // current page (no-op). They were removed; the install/create flows live
  // inside each tab's content (e.g. the install dialog in Agents.tsx).
  it('does not show dead CTA on agents route', () => {
    renderWithRoute('/extensions/agents')
    expect(screen.queryByText(/Create Agent/)).not.toBeInTheDocument()
  })

  it('does not show dead CTA on datasources route', () => {
    renderWithRoute('/extensions/datasources')
    expect(screen.queryByText(/Add Source/)).not.toBeInTheDocument()
  })
})

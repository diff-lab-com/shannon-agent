import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { MemoryRouter, Routes, Route } from 'react-router-dom'
import Settings from '@/pages/Settings'

function renderSettings() {
  return render(
    <MemoryRouter initialEntries={['/settings']}>
      <Routes>
        <Route path="/settings" element={<Settings />} />
      </Routes>
    </MemoryRouter>
  )
}

describe('Settings', () => {
  it('renders without crashing', () => {
    const { container } = renderSettings()
    expect(container.firstChild).toBeTruthy()
  })

  it('has scrolling content area', () => {
    const { container } = renderSettings()
    const scrollable = container.querySelector('[class*="overflow-y-auto"]')
    expect(scrollable).toBeTruthy()
  })
})

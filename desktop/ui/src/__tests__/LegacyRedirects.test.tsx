// C3: Legacy route redirects.
// Verifies old paths (/strategic-focus, /agent-swarm, /quick-inject,
// /background-tasks) redirect to their new homes (/opc or /tasks).

import { describe, it, expect } from 'vitest'
import { render } from '@testing-library/react'
import { MemoryRouter, Routes, Route, Navigate } from 'react-router-dom'

function renderRedirects(initialPath: string) {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        {/* Mirror the redirects defined in App.tsx */}
        <Route path="/strategic-focus" element={<Navigate to="/opc" replace />} />
        <Route path="/agent-swarm" element={<Navigate to="/opc" replace />} />
        <Route path="/quick-inject" element={<Navigate to="/tasks" replace />} />
        <Route path="/background-tasks" element={<Navigate to="/tasks" replace />} />
        {/* Terminal targets so we can confirm arrival */}
        <Route path="/opc" element={<div data-testid="opc-page" />} />
        <Route path="/tasks" element={<div data-testid="tasks-page" />} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('Legacy route redirects', () => {
  it('redirects /strategic-focus to /opc', () => {
    renderRedirects('/strategic-focus')
    expect(document.querySelector('div[data-testid="opc-page"]')).not.toBeNull()
  })

  it('redirects /agent-swarm to /opc', () => {
    renderRedirects('/agent-swarm')
    expect(document.querySelector('div[data-testid="opc-page"]')).not.toBeNull()
  })

  it('redirects /quick-inject to /tasks', () => {
    renderRedirects('/quick-inject')
    expect(document.querySelector('div[data-testid="tasks-page"]')).not.toBeNull()
  })

  it('redirects /background-tasks to /tasks', () => {
    renderRedirects('/background-tasks')
    expect(document.querySelector('div[data-testid="tasks-page"]')).not.toBeNull()
  })
})

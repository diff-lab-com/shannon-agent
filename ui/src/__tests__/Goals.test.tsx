import { describe, it, expect } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import { I18nProvider } from '@/i18n'
import { MemoryRouter } from 'react-router-dom'
import Goals from '@/pages/Goals'

function wrap(ui: React.ReactElement) {
  return (
    <I18nProvider>
      <AppProvider>
        <MemoryRouter>
          {ui}
        </MemoryRouter>
      </AppProvider>
    </I18nProvider>
  )
}

describe('Goals page', () => {
  it('renders task management heading', async () => {
    render(wrap(<Goals />))
    await waitFor(() => expect(screen.getByText('Task Management')).toBeInTheDocument())
  })

  it('renders functional search input', async () => {
    render(wrap(<Goals />))
    await waitFor(() => expect(screen.getByPlaceholderText('Search tasks...')).toBeInTheDocument())
    const input = screen.getByPlaceholderText('Search tasks...')
    fireEvent.change(input, { target: { value: 'my task' } })
    expect(input).toHaveValue('my task')
  })

  it('renders task summary sidebar', async () => {
    render(wrap(<Goals />))
    await waitFor(() => expect(screen.getByText('Task Summary')).toBeInTheDocument())
  })

  it('renders active agents section', async () => {
    render(wrap(<Goals />))
    await waitFor(() => expect(screen.getByText(/Active Agents/)).toBeInTheDocument())
  })
})

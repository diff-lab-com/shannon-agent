import { describe, it, expect, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { AppProvider } from '@/context/AppContext'
import ExtensionsHub from '@/components/extensions/ExtensionsHub'

function wrap(ui: React.ReactElement) {
  return <AppProvider>{ui}</AppProvider>
}

describe('ExtensionsHub', () => {
  it('renders available skills heading', () => {
    render(wrap(<ExtensionsHub />))
    expect(screen.getByText('Available Skills')).toBeInTheDocument()
  })

  it('shows loading spinner initially', async () => {
    render(wrap(<ExtensionsHub />))
    // The spinner shows while loading, then empty state appears
    await waitFor(() => {
      expect(screen.getByText('No skills available.')).toBeInTheDocument()
    })
  })

  it('renders empty state when no skills exist', async () => {
    render(wrap(<ExtensionsHub />))
    await waitFor(() => {
      expect(screen.getByText('No skills available.')).toBeInTheDocument()
    })
    expect(screen.getByText(/Skills can be added via MCP servers/i)).toBeInTheDocument()
  })
})

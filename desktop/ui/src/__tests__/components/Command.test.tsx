import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import {
  CommandDialog,
  CommandInput,
  CommandList,
  CommandEmpty,
} from '@/components/ui/command'

describe('CommandDialog', () => {
  // Regression (2026-08-26 audit): CommandDialog used to render cmdk
  // children without the <Command> root, so every child that subscribes
  // to the cmdk store crashed on open with "Cannot read properties of
  // undefined (reading 'subscribe')". The root must stay.
  it('renders cmdk children inside a Command root (store present)', () => {
    render(
      <CommandDialog open onOpenChange={() => {}}>
        <CommandInput placeholder="Type a command…" />
        <CommandList>
          <CommandEmpty>No results</CommandEmpty>
        </CommandList>
      </CommandDialog>
    )
    expect(screen.getByRole('combobox')).toBeInTheDocument()
    expect(screen.getByText('No results')).toBeInTheDocument()
  })
})

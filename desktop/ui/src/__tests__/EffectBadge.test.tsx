import { describe, it, expect } from 'vitest'
import { render, screen } from '@testing-library/react'
import EffectBadge from '@/components/settings/EffectBadge'

// Settings R3 (T1): the four effect kinds must resolve to their brief-exact
// copy (en is the fallback locale under the auto-wrapped I18nProvider).
describe('EffectBadge', () => {
  it('renders the instant-effect copy for kind="instant"', () => {
    render(<EffectBadge kind="instant" />)
    expect(screen.getByText('Instant effect')).toBeInTheDocument()
  })

  it('renders the new-session copy for kind="new-session"', () => {
    render(<EffectBadge kind="new-session" />)
    expect(screen.getByText('Applies to new sessions')).toBeInTheDocument()
  })

  it('renders the restart copy for kind="restart-app"', () => {
    render(<EffectBadge kind="restart-app" />)
    expect(screen.getByText('Restart required')).toBeInTheDocument()
  })

  it('renders the gateway-restart copy for kind="restart-gateway"', () => {
    render(<EffectBadge kind="restart-gateway" />)
    expect(screen.getByText('Gateway restart required')).toBeInTheDocument()
  })

  it('distinguishes the two restart kinds visually (app vs gateway tone)', () => {
    const { container, rerender } = render(<EffectBadge kind="restart-app" />)
    const app = container.firstElementChild!.className
    rerender(<EffectBadge kind="restart-gateway" />)
    const gateway = container.firstElementChild!.className
    expect(app).toContain('warning-container')
    expect(gateway).toContain('tertiary-container')
    expect(app).not.toEqual(gateway)
  })
})

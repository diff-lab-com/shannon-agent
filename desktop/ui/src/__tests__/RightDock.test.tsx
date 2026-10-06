// B3 (2026-09-26 round 2) — RightDock regression coverage for the batch's
// dock items:
//   §P1-12  web tabs get the stable `web:<normalized-url>` id (one tab per URL)
//   §P2-21  tablist structure — labelledby resolution, utility buttons
//           outside the tablist, real sibling close button, roving-tabIndex
//           arrow-key navigation, keyboard-operable resizer
//   §P2-22  width clamp — 60% viewport cap wins over MIN_WIDTH on narrow
//           windows, and drag suppresses the width transition

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, within, act } from '@testing-library/react'
import { I18nProvider } from '@/i18n'
import { ArtifactProvider, useArtifact } from '@/components/artifact/ArtifactContext'
import RightDock from '@/pages/chat/RightDock'
import { openLink, resetLinkPanelRouterForTests } from '@/lib/openLink'

// The context tab body pulls the whole session-provider stack; its content
// is irrelevant here.
vi.mock('@/pages/chat/ContextPanel', () => ({
  default: () => null,
  ContextPanelContent: () => <div data-testid="context-panel-stub" />,
}))

function ArtifactSeeder() {
  const { open, artifacts } = useArtifact()
  return (
    <div>
      <button onClick={() => open({ kind: 'document', source: '# Seeded\n\nbody', title: 'Seeded doc', confidence: 'high', id: 'document:seed1', origin: 'chat' })}>
        seed-artifact
      </button>
      <div data-testid="artifact-count">{artifacts.length}</div>
    </div>
  )
}

function renderDock() {
  return render(
    <I18nProvider>
      <ArtifactProvider>
        <RightDock
          open
          onOpen={vi.fn()}
          onClose={vi.fn()}
          usage={null}
          activeToolCalls={[]}
          workingDir={null}
          planModeActive={false}
          diffPath={null}
          onCloseDiff={vi.fn()}
        />
        <ArtifactSeeder />
      </ArtifactProvider>
    </I18nProvider>,
  )
}

beforeEach(() => {
  localStorage.clear()
  resetLinkPanelRouterForTests()
})

afterEach(() => {
  // Restore the jsdom default mutated by the narrow-window clamp test.
  window.innerWidth = 1024
  window.dispatchEvent(new Event('resize'))
})

describe('RightDock tablist a11y (§P2-21)', () => {
  it('tabpanel aria-labelledby resolves to a real tab button id for artifact tabs', () => {
    const { container } = renderDock()
    // Default utility tab.
    expect(screen.getByRole('tabpanel').getAttribute('aria-labelledby')).toBe('dock-tab-context')

    // Artifact tab ids contain ':' — the old `dock-tab-${tab}` template
    // produced `dock-tab-a:document:seed1`, which matched no element.
    fireEvent.click(screen.getByText('seed-artifact'))
    const labelledby = screen.getByRole('tabpanel').getAttribute('aria-labelledby') ?? ''
    expect(labelledby).toBe('dock-tab-a-document:seed1')
    expect(document.getElementById(labelledby)).not.toBeNull()
    expect(container).toBeTruthy()
  })

  it('keeps the utility buttons (+ / fullscreen / collapse) outside the tablist', () => {
    renderDock()
    const tablist = screen.getByRole('tablist')
    // Tabs themselves stay in.
    expect(within(tablist).getAllByRole('tab').length).toBeGreaterThanOrEqual(4)
    // Utility controls must not be children of role=tablist …
    expect(within(tablist).queryByRole('button', { name: 'Open file as document tab' })).toBeNull()
    expect(within(tablist).queryByRole('button', { name: 'Close dock' })).toBeNull()
    expect(within(tablist).queryByRole('button', { name: 'Enter fullscreen' })).toBeNull()
    // …while remaining on the same visual row (shared wrapper).
    // "Close dock" matches the hint dismiss too — assert by presence.
    expect(screen.getAllByRole('button', { name: 'Close dock' }).length).toBeGreaterThanOrEqual(1)
    expect(screen.getByRole('button', { name: 'Open file as document tab' })).toBeTruthy()
  })

  it('close control is a real sibling button — one click closes without nesting', () => {
    renderDock()
    fireEvent.click(screen.getByText('seed-artifact'))
    expect(screen.getByTestId('artifact-count')).toHaveTextContent('1')

    const tab = screen.getByRole('tab', { name: /Seeded doc/ })
    const closeBtn = screen.getByRole('button', { name: 'Close Seeded doc' })
    // Not nested inside the tab button (the old interactive-span-in-button).
    expect(tab.contains(closeBtn)).toBe(false)

    fireEvent.click(closeBtn)
    expect(screen.getByTestId('artifact-count')).toHaveTextContent('0')
  })

  it('arrow keys move selection with focus (roving tabIndex), skipping disabled tabs', () => {
    renderDock()
    fireEvent.click(screen.getByText('seed-artifact'))
    // Seeding activated the artifact tab (auto-dock). Tab order skips the
    // disabled Diff tab: context → plan → live → artifact.
    const tablist = screen.getByRole('tablist')
    const selectedId = () =>
      within(tablist).getAllByRole('tab').find(t => t.getAttribute('aria-selected') === 'true')?.id

    expect(selectedId()).toBe('dock-tab-a-document:seed1')

    fireEvent.keyDown(tablist, { key: 'ArrowRight' }) // wraps to the first tab
    expect(selectedId()).toBe('dock-tab-context')
    expect(document.activeElement?.id).toBe('dock-tab-context')

    fireEvent.keyDown(tablist, { key: 'ArrowRight' })
    expect(selectedId()).toBe('dock-tab-plan')
    expect(document.activeElement?.id).toBe('dock-tab-plan')

    fireEvent.keyDown(tablist, { key: 'End' })
    expect(selectedId()).toBe('dock-tab-a-document:seed1')
    expect(document.activeElement?.id).toBe('dock-tab-a-document:seed1')

    fireEvent.keyDown(tablist, { key: 'ArrowLeft' }) // live, not the disabled diff
    expect(selectedId()).toBe('dock-tab-live')

    fireEvent.keyDown(tablist, { key: 'Home' })
    expect(selectedId()).toBe('dock-tab-context')
  })

  it('implements roving tabIndex — only the selected tab is in the Tab order', () => {
    renderDock()
    fireEvent.click(screen.getByText('seed-artifact'))
    for (const tab of within(screen.getByRole('tablist')).getAllByRole('tab')) {
      const expected = tab.getAttribute('aria-selected') === 'true' ? '0' : '-1'
      expect(tab.getAttribute('tabindex')).toBe(expected)
    }
  })

  it('resizer is keyboard-operable and steps by 16px (ArrowLeft widens)', () => {
    const { container } = renderDock()
    const aside = container.querySelector('aside') as HTMLElement
    const before = parseInt(aside.style.width, 10)
    const sep = screen.getByRole('separator')
    expect(sep.getAttribute('tabindex')).toBe('0')

    fireEvent.keyDown(sep, { key: 'ArrowLeft' })
    expect(parseInt(aside.style.width, 10)).toBe(before + 16)
    fireEvent.keyDown(sep, { key: 'ArrowRight' })
    expect(parseInt(aside.style.width, 10)).toBe(before)
  })
})

describe('RightDock resize (§P2-22)', () => {
  it('clamps the restored width to 60% of the current window (cap beats MIN_WIDTH)', () => {
    window.innerWidth = 400 // cap = 240 → effectiveMin = min(280, 240) = 240
    localStorage.setItem('shannon.dock.width', '720')
    const { container } = renderDock()
    const aside = container.querySelector('aside') as HTMLElement
    expect(parseInt(aside.style.width, 10)).toBe(240)
  })

  it('suppresses the width transition while dragging', () => {
    const { container } = renderDock()
    const aside = container.querySelector('aside') as HTMLElement
    expect(aside.className).toContain('transition-all')

    fireEvent.pointerDown(screen.getByRole('separator'))
    expect(aside.className).not.toContain('transition-all')

    fireEvent.pointerUp(window)
    expect(aside.className).toContain('transition-all')
  })
})

describe('RightDock web tab dedup (§P1-12)', () => {
  it('reuses one tab per normalized URL and activates it', async () => {
    renderDock()
    await act(async () => {
      await openLink('https://example.com/docs/')
    })
    expect(screen.getByTestId('artifact-count')).toHaveTextContent('1')

    await act(async () => {
      await openLink('https://example.com/docs') // trailing slash folds away
    })
    expect(screen.getByTestId('artifact-count')).toHaveTextContent('1')

    await act(async () => {
      await openLink('https://example.com/other')
    })
    expect(screen.getByTestId('artifact-count')).toHaveTextContent('2')
  })
})

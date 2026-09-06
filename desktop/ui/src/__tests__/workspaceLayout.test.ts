// P1-5 C-2 — layout model pure-function tests (rect math, collision,
// presets, version reset, persistence-key derivation).

import { describe, expect, it } from 'vitest'
import {
  GRID_COLUMNS,
  GRID_ROWS,
  addPanel,
  findPanel,
  matchesPreset,
  movePanel,
  normalizeLayout,
  presetLayout,
  rectsOverlap,
  rectFits,
  removePanel,
  resizePanel,
  resolveLayout,
  swapPanels,
  withRect,
  workspaceProjectKey,
  WORKSPACE_LAYOUT_VERSION,
  type PanelRect,
  type WorkspaceLayout,
} from '@/components/workspace/layout'

const r = (col: number, row: number, w: number, h: number): PanelRect => ({ col, row, w, h })

describe('rect math', () => {
  it('rectsOverlap detects partial and contained intersections only', () => {
    expect(rectsOverlap(r(1, 1, 4, 4), r(4, 1, 4, 4))).toBe(true) // edge-adjacent cols overlap at col 4
    expect(rectsOverlap(r(1, 1, 4, 4), r(5, 1, 4, 4))).toBe(false)
    expect(rectsOverlap(r(1, 1, 12, 12), r(2, 2, 1, 1))).toBe(true)
    expect(rectsOverlap(r(1, 1, 4, 4), r(1, 5, 4, 4))).toBe(false)
  })

  it('rectFits enforces the 12×12 grid and 1-based positive ints', () => {
    expect(rectFits(r(1, 1, 12, 12))).toBe(true)
    expect(rectFits(r(12, 12, 1, 1))).toBe(true)
    expect(rectFits(r(1, 1, 13, 1))).toBe(false)
    expect(rectFits(r(1, 1, 1, 13))).toBe(false)
    expect(rectFits(r(0, 1, 1, 1))).toBe(false)
    expect(rectFits(r(1.5, 1, 1, 1))).toBe(false)
  })
})

describe('presets', () => {
  it('exposes the three presets with valid, collision-free geometry', () => {
    for (const name of ['focus', 'review', 'build'] as const) {
      const layout = presetLayout(name)
      expect(layout.version).toBe(WORKSPACE_LAYOUT_VERSION)
      expect(normalizeLayout(layout)).toEqual(layout)
      const kinds = layout.panels.map(p => p.kind)
      expect(kinds).toContain('chat')
      for (let i = 0; i < layout.panels.length; i++) {
        expect(rectFits(layout.panels[i].rect)).toBe(true)
        for (let j = i + 1; j < layout.panels.length; j++) {
          expect(rectsOverlap(layout.panels[i].rect, layout.panels[j].rect)).toBe(false)
        }
      }
    }
  })

  it('focus preset is a single full-grid chat panel (default look = current page)', () => {
    const focus = presetLayout('focus')
    expect(focus.panels).toHaveLength(1)
    expect(focus.panels[0]).toEqual({ id: 'chat', kind: 'chat', rect: r(1, 1, GRID_COLUMNS, GRID_ROWS) })
  })

  it('review = chat+diff, build = chat+terminal+preview', () => {
    expect(presetLayout('review').panels.map(p => p.kind).sort()).toEqual(['chat', 'diff'])
    expect(presetLayout('build').panels.map(p => p.kind).sort()).toEqual(['chat', 'preview', 'terminal'])
  })

  it('matchesPreset detects presets and ignores edited layouts', () => {
    const focus = presetLayout('focus')
    expect(matchesPreset(focus, 'focus')).toBe(true)
    expect(matchesPreset(focus, 'review')).toBe(false)
    // The review preset tiles the whole grid (every move is blocked), so
    // edit it via a swap — the ids no longer sit at their preset rects.
    const swapped = swapPanels(presetLayout('review'), 'chat', 'diff')
    expect(matchesPreset(swapped, 'review')).toBe(false)
  })

  it('presetLayout returns independent copies', () => {
    const a = presetLayout('review')
    a.panels[0].rect.w = 12
    expect(presetLayout('review').panels[0].rect.w).toBe(8)
  })
})

describe('normalizeLayout / resolveLayout (restore semantics)', () => {
  it('accepts a valid saved layout', () => {
    const review = presetLayout('review')
    expect(normalizeLayout(JSON.parse(JSON.stringify(review)))).toEqual(review)
  })

  it('resets on version mismatch', () => {
    const saved = { ...presetLayout('review'), version: WORKSPACE_LAYOUT_VERSION + 1 }
    expect(normalizeLayout(saved)).toBeNull()
  })

  it('resets on unknown kind, missing chat, duplicate kind, or bad rect', () => {
    const base = presetLayout('build')

    const badKind = structuredClone(base)
    ;(badKind.panels[1].kind as string) = 'editor'
    expect(normalizeLayout(badKind)).toBeNull()

    const noChat = structuredClone(base)
    noChat.panels = noChat.panels.filter(p => p.kind !== 'chat')
    expect(normalizeLayout(noChat)).toBeNull()

    const dup = structuredClone(base)
    dup.panels[2] = { id: 'another-terminal', kind: 'terminal', rect: r(1, 1, 3, 3) }
    expect(normalizeLayout(dup)).toBeNull()

    const badRect = structuredClone(base)
    badRect.panels[0].rect = r(1, 1, 13, 12)
    expect(normalizeLayout(badRect)).toBeNull()

    const zeroRect = structuredClone(base)
    zeroRect.panels[0].rect = r(0, 1, 12, 12)
    expect(normalizeLayout(zeroRect)).toBeNull()
  })

  it('resets on overlapping panels', () => {
    const overlap = presetLayout('review')
    overlap.panels[1].rect = r(8, 1, 4, 12) // chat spans cols 1-8 → overlaps at col 8
    expect(normalizeLayout(overlap)).toBeNull()
  })

  it('resets on non-object / malformed payloads', () => {
    expect(normalizeLayout(null)).toBeNull()
    expect(normalizeLayout('layout')).toBeNull()
    expect(normalizeLayout({ version: 1, panels: 'nope' })).toBeNull()
    expect(normalizeLayout({ version: 1, panels: [42] })).toBeNull()
    expect(normalizeLayout({ version: 1, panels: [{ id: '', kind: 'chat', rect: r(1, 1, 12, 12) }] })).toBeNull()
  })

  it('resolveLayout: null → focus preset; invalid → focus preset; valid → kept', () => {
    expect(resolveLayout(null)).toEqual(presetLayout('focus'))
    expect(resolveLayout({ version: 42, panels: [] })).toEqual(presetLayout('focus'))
    expect(resolveLayout(presetLayout('build'))).toEqual(presetLayout('build'))
  })
})

describe('withRect / movePanel / swapPanels', () => {
  // A layout with free space (the presets tile the entire grid): chat on
  // the left, diff parked in the bottom-right, top-right block empty.
  const sparse: WorkspaceLayout = {
    version: WORKSPACE_LAYOUT_VERSION,
    panels: [
      { id: 'chat', kind: 'chat', rect: r(1, 1, 8, 12) },
      { id: 'diff', kind: 'diff', rect: r(9, 8, 4, 5) },
    ],
  }

  it('withRect places a panel into free space and rejects overlaps', () => {
    const moved = withRect(sparse, 'diff', r(9, 1, 4, 7))
    expect(moved).not.toBeNull()
    expect(findPanel(moved!, 'diff')?.rect).toEqual(r(9, 1, 4, 7))

    expect(withRect(sparse, 'diff', r(6, 1, 4, 12))).toBeNull() // overlaps chat (cols 1-8)
    expect(withRect(sparse, 'ghost', r(1, 1, 1, 1))).toBeNull() // unknown id
  })

  it('movePanel steps one cell and blocks at grid edges and collisions', () => {
    expect(findPanel(movePanel(sparse, 'diff', 'up'), 'diff')?.rect).toEqual(r(9, 7, 4, 5))
    expect(findPanel(movePanel(sparse, 'diff', 'down'), 'diff')?.rect).toEqual(r(9, 8, 4, 5)) // blocked at row 12
    expect(findPanel(movePanel(sparse, 'diff', 'left'), 'diff')?.rect).toEqual(r(9, 8, 4, 5)) // blocked by chat
    expect(findPanel(movePanel(sparse, 'diff', 'right'), 'diff')?.rect).toEqual(r(9, 8, 4, 5)) // blocked by col 12
    // A blocked move never shrinks the panel (chat right would collide with
    // diff at cols 9-12 rows 8-12).
    expect(findPanel(movePanel(sparse, 'chat', 'up'), 'chat')?.rect).toEqual(r(1, 1, 8, 12))
    expect(findPanel(movePanel(sparse, 'chat', 'right'), 'chat')?.rect).toEqual(r(1, 1, 8, 12))
  })

  it('swapPanels exchanges rects and ignores unknown/equal ids', () => {
    const swapped = swapPanels(sparse, 'chat', 'diff')
    expect(findPanel(swapped, 'chat')?.rect).toEqual(r(9, 8, 4, 5))
    expect(findPanel(swapped, 'diff')?.rect).toEqual(r(1, 1, 8, 12))
    expect(swapPanels(sparse, 'chat', 'chat')).toEqual(sparse)
    expect(swapPanels(sparse, 'chat', 'nope')).toEqual(sparse)
  })
})

describe('resizePanel', () => {
  it('grows into free space', () => {
    const layout = presetLayout('review')
    const grown = resizePanel(layout, 'diff', { col: 9, row: 1, w: 4, h: 12 })
    expect(findPanel(grown, 'diff')?.rect).toEqual(r(9, 1, 4, 12))
  })

  it('shrinks instead of overlapping (drag into a neighbour is clamped)', () => {
    const layout = presetLayout('build')
    // preview at cols 9-12; dragging its left edge into the chat panel
    // (cols 1-8) must shrink, not overlap.
    const resized = resizePanel(layout, 'preview', { col: 9, row: 1, w: 8, h: 8 })
    const rect = findPanel(resized, 'preview')!.rect
    expect(rect.w).toBe(4)
    expect(rectsOverlap(rect, findPanel(resized, 'chat')!.rect)).toBe(false)
  })
})

describe('addPanel / removePanel', () => {
  it('addPanel places the default size in the first free spot', () => {
    const layout = addPanel(presetLayout('review'), 'terminal')
    const terminal = findPanel(layout, 'terminal')
    expect(terminal?.kind).toBe('terminal')
    expect(rectFits(terminal!.rect)).toBe(true)
    expect(rectsOverlap(terminal!.rect, findPanel(layout, 'chat')!.rect)).toBe(false)
  })

  it('addPanel from the full-grid focus preset shrinks the chat panel to make room', () => {
    const layout = addPanel(presetLayout('focus'), 'terminal')
    expect(findPanel(layout, 'chat')?.rect).toEqual(r(1, 1, 12, 9))
    expect(findPanel(layout, 'terminal')?.rect).toEqual(r(1, 10, 12, 3))

    const layoutDiff = addPanel(presetLayout('focus'), 'diff')
    expect(findPanel(layoutDiff, 'chat')?.rect).toEqual(r(1, 1, 8, 12))
    expect(findPanel(layoutDiff, 'diff')?.rect).toEqual(r(9, 1, 4, 12))
  })

  it('addPanel is idempotent and never adds a second chat', () => {
    const build = presetLayout('build')
    expect(addPanel(build, 'terminal')).toEqual(build)
    expect(addPanel(presetLayout('focus'), 'chat')).toEqual(presetLayout('focus'))
  })

  it('removePanel drops non-chat panels and keeps chat', () => {
    const build = presetLayout('build')
    const withoutTerminal = removePanel(build, 'terminal')
    expect(withoutTerminal.panels.map(p => p.kind).sort()).toEqual(['chat', 'preview'])
    expect(removePanel(build, 'chat')).toEqual(build)
    expect(removePanel(build, 'ghost')).toEqual(build)
  })
})

describe('workspaceProjectKey (frozen: frontend-computed)', () => {
  it('is deterministic and normalizes separators / trailing slashes', () => {
    expect(workspaceProjectKey('/home/ed/proj')).toBe(workspaceProjectKey('/home/ed/proj/'))
    expect(workspaceProjectKey('/home/ed/proj')).toBe(workspaceProjectKey('\\home\\ed\\proj'.replace(/\\/g, '/')))
    expect(workspaceProjectKey('/home/ed/proj')).toMatch(/^p-[0-9a-f]{8}-1[34]$/)
  })

  it('differs per directory', () => {
    const keys = new Set(['/home/ed/proj-a', '/home/ed/proj-b', ''].map(workspaceProjectKey))
    expect(keys.size).toBe(3)
  })
})

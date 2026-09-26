// CodeEditor — CodeMirror 6 wrapper that renders diagnostic squiggles.
//
// jsdom cannot give CodeMirror a meaningful layout (it has no real DOM
// measurements or canvas), so we can't drive gutter clicks or squiggle
// rendering here. These tests focus on the parts that DON'T depend on
// CodeMirror's runtime: that the component mounts under various
// prop combinations, that the controlled-vs-uncontrolled wiring doesn't
// blow up, and that the linter-derived diagnostic offsets are clamped to
// safe ranges (so out-of-bounds diagnostics don't crash the linter).
//
// The integration paths (squiggles, gutter click → onDiagnosticClick)
// are owned by E2E coverage.

import { describe, it, expect, vi, afterEach } from 'vitest'
import { render } from '@testing-library/react'
import CodeEditor, { type EditorDiagnostic } from '@/components/editor/CodeEditor'
import { cmThemeColorsFor, cmThemeFor } from '@/components/editor/cmTheme'

const baseDiag: EditorDiagnostic = {
  start_line: 0,
  start_character: 0,
  end_line: 0,
  end_character: 1,
  message: 'unused variable',
  severity: 'warning',
}

describe('CodeEditor — mount smoke', () => {
  it('mounts with the minimum required props (no diagnostics)', () => {
    expect(() =>
      render(<CodeEditor value="" language="rust" diagnostics={[]} />),
    ).not.toThrow()
  })

  it('mounts with a single diagnostic', () => {
    expect(() =>
      render(
        <CodeEditor
          value="fn main() {}"
          language="rust"
          diagnostics={[baseDiag]}
        />,
      ),
    ).not.toThrow()
  })

  it('mounts for every supported language without crashing', () => {
    const langs = ['rust', 'typescript', 'typescriptreact', 'javascript', 'javascriptreact', 'python', 'go']
    langs.forEach((lang) => {
      expect(() =>
        render(<CodeEditor value="let x = 1" language={lang} diagnostics={[]} />),
      ).not.toThrow()
    })
  })

  it('mounts in read-only mode without a value-change handler', () => {
    expect(() =>
      render(
        <CodeEditor
          value="const x = 1"
          language="typescript"
          diagnostics={[]}
          readOnly
        />,
      ),
    ).not.toThrow()
  })

  it('mounts with out-of-range diagnostic coordinates without crashing', () => {
    // The internal linter clamps line/character to the document bounds;
    // an out-of-range diagnostic from the backend shouldn't throw.
    expect(() =>
      render(
        <CodeEditor
          value="hi"
          language="rust"
          diagnostics={[
            { start_line: 999, start_character: 999, end_line: 999, end_character: 999,
              message: 'way out of range', severity: 'error' },
          ]}
        />,
      ),
    ).not.toThrow()
  })

  it('mounts with a callback for diagnostic clicks (handler stored, not called in jsdom)', () => {
    const onDiagnosticClick = vi.fn()
    expect(() =>
      render(
        <CodeEditor
          value="let x = 1"
          language="javascript"
          diagnostics={[baseDiag]}
          onDiagnosticClick={onDiagnosticClick}
        />,
      ),
    ).not.toThrow()
    // No real layout → no click is dispatched → handler stays at 0 calls.
    expect(onDiagnosticClick).not.toHaveBeenCalled()
  })
})

// ─── P1-34: the editor theme follows <html data-theme> ────────────────────

describe('cmTheme — resolved-theme palette selection', () => {
  afterEach(() => {
    document.documentElement.removeAttribute('data-theme')
    vi.restoreAllMocks()
  })

  it('uses the light floor for light and unknown theme ids', () => {
    const light = cmThemeColorsFor('material', () => 'light')
    expect(light.mode).toBe('light')
    expect(light.background).toBe('#ffffff')

    const unknown = cmThemeColorsFor('not-a-theme', () => undefined)
    expect(unknown.mode).toBe('light')
    expect(unknown.background).toBe('#ffffff')
  })

  it('uses the dark floor for dark theme ids', () => {
    const dark = cmThemeColorsFor('tokyo-night', () => 'dark')
    expect(dark.mode).toBe('dark')
    expect(dark.background).toBe('#1a1b26')
    expect(dark.foreground).toBe('#a9b1d6')
  })

  it('layers live CSS variables over the floor when the stylesheet is readable', () => {
    vi.spyOn(window, 'getComputedStyle').mockReturnValue({
      getPropertyValue: (name: string) =>
        name === '--color-surface' ? '#101014' : '',
    } as unknown as CSSStyleDeclaration)
    const dark = cmThemeColorsFor('tokyo-night', () => 'dark')
    expect(dark.background).toBe('#101014') // token wins
    expect(dark.foreground).toBe('#a9b1d6') // floor retained
  })

  it('builds a CodeMirror theme extension flagged with the right mode', () => {
    // Extension objects are opaque; the contract is that construction
    // succeeds and the palette resolution above drives its colors.
    expect(cmThemeFor('tokyo-night', () => 'dark')).toBeTruthy()
    expect(cmThemeFor('material', () => 'light')).toBeTruthy()
  })

  it('CodeEditor mounts under a dark data-theme without crashing', () => {
    document.documentElement.setAttribute('data-theme', 'tokyo-night')
    expect(() =>
      render(<CodeEditor value="fn main() {}" language="rust" diagnostics={[]} />),
    ).not.toThrow()
  })
})

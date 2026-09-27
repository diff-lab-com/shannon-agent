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
import {
  cmSyntaxHighlightFor,
  cmSyntaxPaletteFor,
  cmThemeColorsFor,
  cmThemeFor,
  type CmSyntaxPalette,
} from '@/components/editor/cmTheme'

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
    // succeeds and the palette resolution above drives its colors. The
    // highlight-extension wiring is asserted in the B6b block below via
    // cmSyntaxHighlightFor (the function cmThemeFor's dark branch embeds).
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

// ─── B6b: dark mode mounts a syntax HighlightStyle instead of going mono ───
//
// Pre-B6b the dark path had NO token coloring (basicSetup's light-tuned
// defaultHighlightStyle was disabled and nothing replaced it). The contract
// now: dark mode's extension stack includes a dark HighlightStyle; light
// mode stays exactly as before (basicSetup's defaultHighlightStyle, see
// CodeEditor.tsx's `syntaxHighlighting: !dark` — cmSyntaxHighlightFor
// returning `null` there is the testable "light path unchanged" assertion).

describe('cmTheme — dark syntax highlighting (B6b)', () => {
  afterEach(() => {
    document.documentElement.removeAttribute('data-theme')
    vi.restoreAllMocks()
  })

  it('dark mode mounts a syntax-highlight extension, light mode does not', () => {
    const darkHl = cmSyntaxHighlightFor('dark')
    expect(darkHl).toBeTruthy()
    // Light keeps basicSetup's defaultHighlightStyle — no second highlighter.
    expect(cmSyntaxHighlightFor('light')).toBeNull()
  })

  it('dark palette falls back to the fixed AA floor when tokens are unreadable (jsdom)', () => {
    const palette = cmSyntaxPaletteFor('dark') as CmSyntaxPalette
    expect(palette.keyword).toBe('#7aa2f7') // primary family, 6.79:1 on #1a1b26
    expect(palette.string).toBe('#e0af68') // 8.55:1
    expect(palette.comment).toBe('#a9b1d6') // 8.10:1, italic applied at style level
    expect(palette.constant).toBe('#9d7cd8') // 5.13:1
    expect(palette.tag).toBe('#f7768e') // 6.46:1
    expect(palette.attribute).toBe('#7aa2f7') // shares the keyword family
    expect(palette.heading).toBe('#a9b1d6') // on-surface, bold applied at style level
  })

  it('dark palette prefers the live theme tokens when the stylesheet is readable', () => {
    const tokens: Record<string, string> = {
      '--color-primary': '#aabb00',
      '--color-tertiary': '#112233',
      '--color-on-surface-variant': '#445566',
      '--color-secondary': '#778899',
      '--color-error': '#ff0000',
      '--color-on-surface': '#c0caf5',
    }
    vi.spyOn(window, 'getComputedStyle').mockReturnValue({
      getPropertyValue: (name: string) => tokens[name] ?? '',
    } as unknown as CSSStyleDeclaration)
    const palette = cmSyntaxPaletteFor('dark') as CmSyntaxPalette
    expect(palette.keyword).toBe('#aabb00') // token wins
    expect(palette.attribute).toBe('#aabb00') // follows the keyword family
    expect(palette.string).toBe('#112233')
    expect(palette.comment).toBe('#445566')
    expect(palette.constant).toBe('#778899')
    expect(palette.tag).toBe('#ff0000')
    expect(palette.heading).toBe('#c0caf5')
  })

  it('falls back per-role when only some tokens are readable', () => {
    vi.spyOn(window, 'getComputedStyle').mockReturnValue({
      getPropertyValue: (name: string) => (name === '--color-primary' ? '#aabb00' : ''),
    } as unknown as CSSStyleDeclaration)
    const palette = cmSyntaxPaletteFor('dark') as CmSyntaxPalette
    expect(palette.keyword).toBe('#aabb00') // token wins
    expect(palette.string).toBe('#e0af68') // floor retained
  })
})

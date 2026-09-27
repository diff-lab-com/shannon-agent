/**
 * P1-34 / B6b — CodeMirror 6 theme mapping for the editor.
 *
 * Mirrors the TerminalPanel's `xtermTheme` strategy: the app's themes are
 * generated from the single source `scripts/theme-source.json` →
 * `src/theme/generated/themes.css` (one `[data-theme]` block per theme) +
 * `THEME_REGISTRY`, which records each theme's light/dark scheme (see
 * `ThemeContext.themeModeOf`).
 *
 * Mapping strategy: read the *live* CSS custom properties the active theme
 * block defines (surface / on-surface / primary / secondary-container /
 * surface containers / outline) so the editor chrome follows every
 * registered theme for free, and keep a light/dark base palette keyed by
 * `themeModeOf()` from the generated registry as the floor — used when
 * computed styles are unavailable (jsdom, SSR).
 *
 * Syntax tokens: the light path keeps basicSetup's light-tuned
 * `defaultHighlightStyle` (wired via `basicSetup.syntaxHighlighting` in
 * CodeEditor.tsx). Dark themes mount their own dark HighlightStyle instead
 * (the light-tuned default is unreadable on dark surfaces and stays
 * disabled there). The dark token palette follows the same two-layer
 * strategy as the chrome: Material tokens from the live theme block when
 * readable (keyword→primary, string→tertiary, comment→on-surface-variant,
 * number/constant→secondary, tag→error, attribute→primary, heading→
 * on-surface), falling back to a fixed dark floor palette (see
 * `DARK_SYNTAX_FLOOR` for the WCAG AA contrast rationale).
 *
 * Dependencies: `@uiw/codemirror-themes` (the @uiw/react-codemirror
 * ecosystem's companion package, pinned to the same 4.25.x line) provides
 * `createTheme`, i.e. the `@codemirror/language` HighlightStyle machinery
 * without adding `@codemirror/language` as a direct dependency. Called
 * with empty `settings` it emits only empty chrome rules, so the
 * `EditorView.theme` below remains the single source of chrome styling and
 * just the token recoloring is picked up. Tag roles come from
 * `@lezer/highlight` (already in the dependency tree via
 * `@codemirror/language`; pinned exactly to the tree's version — the tag
 * objects are module singletons and a second copy would silently unmatch
 * every token).
 */
import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
import { tags as t } from '@lezer/highlight';
import { createTheme } from '@uiw/codemirror-themes';
import { themeModeOf } from '@/context/ThemeContext';

/** A concrete theme id (ThemeContext keeps `ResolvedTheme` module-local). */
type ResolvedTheme = Parameters<typeof themeModeOf>[0];

function readVar(name: string): string | null {
  if (typeof window === 'undefined' || typeof document === 'undefined') return null;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value.length > 0 ? value : null;
}

/** The resolved colors behind the editor chrome (pure data — unit-testable). */
export interface CmThemeColors {
  mode: 'light' | 'dark';
  background: string;
  foreground: string;
  cursor: string;
  selection: string;
  selectionDim: string;
  gutterBackground: string;
  gutterBorder: string;
  activeLine: string;
}

/**
 * The syntax-token palette a dark theme resolves to (pure data —
 * unit-testable). `attribute` deliberately shares the keyword color: both
 * are "structure" roles in the same primary family.
 */
export interface CmSyntaxPalette {
  keyword: string;
  string: string;
  comment: string;
  constant: string;
  tag: string;
  attribute: string;
  heading: string;
}

/** Light-scheme floor (AA on light surfaces) when tokens can't be read. */
const LIGHT_FLOOR: Omit<CmThemeColors, 'mode'> = {
  background: '#ffffff',
  foreground: '#191c1e',
  cursor: '#0051d5',
  selection: '#d7e3ff',
  selectionDim: '#e6e8ea',
  gutterBackground: '#ffffff',
  gutterBorder: '#d8dadc',
  activeLine: '#f2f4f6',
};

/** Dark-scheme floor (AA on dark surfaces) when tokens can't be read. */
const DARK_FLOOR: Omit<CmThemeColors, 'mode'> = {
  background: '#1a1b26',
  foreground: '#a9b1d6',
  cursor: '#7aa2f7',
  selection: '#33467c',
  selectionDim: '#24283b',
  gutterBackground: '#1a1b26',
  gutterBorder: '#2f334d',
  activeLine: '#24283b',
};

/**
 * Dark-scheme syntax-token floor, for when the live theme tokens can't be
 * read (jsdom, SSR). Values follow Tokyo Night (the registry's dark floor
 * theme, same source as DARK_FLOOR) and every one reaches WCAG AA (≥ 4.5:1)
 * against DARK_FLOOR's background `#1a1b26` — ratios computed with the
 * WCAG 2.x relative-luminance formula:
 *   string    #e0af68  8.55:1
 *   comment   #a9b1d6  8.10:1   (+ heading #a9b1d6, bolded)
 *   keyword   #7aa2f7  6.79:1   (+ attribute, same primary family)
 *   tag       #f7768e  6.46:1
 *   constant  #9d7cd8  5.13:1
 */
const DARK_SYNTAX_FLOOR: CmSyntaxPalette = {
  keyword: '#7aa2f7',
  string: '#e0af68',
  comment: '#a9b1d6',
  constant: '#9d7cd8',
  tag: '#f7768e',
  attribute: '#7aa2f7',
  heading: '#a9b1d6',
};

/**
 * Resolve the dark syntax palette (pure — jsdom/SSR safe): the floor above,
 * overlaid with the active theme's Material tokens where readable. Returns
 * `null` for light mode — light keeps basicSetup's defaultHighlightStyle and
 * must not be re-styled here.
 */
export function cmSyntaxPaletteFor(mode: 'light' | 'dark'): CmSyntaxPalette | null {
  if (mode !== 'dark') return null;
  const palette: CmSyntaxPalette = { ...DARK_SYNTAX_FLOOR };
  const keyword = readVar('--color-primary');
  if (keyword) {
    palette.keyword = keyword;
    palette.attribute = keyword;
  }
  const string = readVar('--color-tertiary');
  if (string) palette.string = string;
  const comment = readVar('--color-on-surface-variant');
  if (comment) palette.comment = comment;
  const constant = readVar('--color-secondary');
  if (constant) palette.constant = constant;
  const tag = readVar('--color-error');
  if (tag) palette.tag = tag;
  const heading = readVar('--color-on-surface') ?? readVar('--foreground');
  if (heading) palette.heading = heading;
  return palette;
}

/**
 * The syntax-highlighting extension for a resolved mode: the dark token
 * HighlightStyle for dark mode, `null` for light (basicSetup's light-tuned
 * defaultHighlightStyle owns the light path — see CodeEditor.tsx's
 * `basicSetup.syntaxHighlighting` wiring).
 */
export function cmSyntaxHighlightFor(mode: 'light' | 'dark'): Extension | null {
  const palette = cmSyntaxPaletteFor(mode);
  if (!palette) return null;
  return createTheme({
    theme: 'dark',
    // Empty settings → createTheme emits only its token HighlightStyle (the
    // paired EditorView.theme collapses to empty rules); chrome stays owned
    // by the EditorView.theme in cmThemeFor below.
    settings: {},
    styles: [
      {
        tag: [t.keyword, t.controlKeyword, t.moduleKeyword, t.definitionKeyword, t.operatorKeyword],
        color: palette.keyword,
      },
      { tag: [t.string, t.docString], color: palette.string },
      { tag: t.comment, color: palette.comment, fontStyle: 'italic' },
      { tag: [t.number, t.bool, t.atom, t.null], color: palette.constant },
      { tag: t.tagName, color: palette.tag },
      { tag: t.attributeName, color: palette.attribute },
      { tag: t.heading, color: palette.heading, fontWeight: 'bold' },
    ],
  });
}

/**
 * Resolve the editor palette for a `<html data-theme>` id (pure — jsdom/SSR
 * safe; the floor comes from the registry's light/dark mode, token-backed
 * colors are layered on top when the live stylesheet is readable).
 */
export function cmThemeColorsFor(
  themeId: string,
  modeOf: (theme: string) => 'light' | 'dark' | undefined = id =>
    themeModeOf(id as ResolvedTheme),
): CmThemeColors {
  const mode = modeOf(themeId) ?? 'light';
  const colors: CmThemeColors = { mode, ...(mode === 'dark' ? DARK_FLOOR : LIGHT_FLOOR) };
  const background = readVar('--color-surface') ?? readVar('--background');
  if (background) colors.background = background;
  const foreground = readVar('--color-on-surface') ?? readVar('--foreground');
  if (foreground) colors.foreground = foreground;
  const cursor = readVar('--color-primary');
  if (cursor) colors.cursor = cursor;
  const selection = readVar('--color-secondary-container');
  if (selection) colors.selection = selection;
  const selectionDim = readVar('--color-surface-container');
  if (selectionDim) colors.selectionDim = selectionDim;
  const gutterBackground = readVar('--color-surface-container-lowest');
  if (gutterBackground) colors.gutterBackground = gutterBackground;
  const border = readVar('--color-outline-variant');
  if (border) colors.gutterBorder = border;
  const activeLine = readVar('--color-surface-container-low');
  if (activeLine) colors.activeLine = activeLine;
  return colors;
}

/**
 * Build the CodeMirror theme extension for the resolved app theme. The
 * `{ dark }` flag tells CM6 the palette is dark so its built-in
 * dark-mode-aware defaults (selection blending, placeholder, …) agree with
 * ours. In dark mode the returned extension also carries the dark syntax
 * HighlightStyle (an `Extension[]`; light mode stays the single chrome
 * extension — see `cmSyntaxHighlightFor`).
 */
export function cmThemeFor(
  themeId: string,
  modeOf?: (theme: string) => 'light' | 'dark' | undefined,
): Extension {
  const c = cmThemeColorsFor(themeId, modeOf);
  const chrome = EditorView.theme(
    {
      '&': {
        color: c.foreground,
        backgroundColor: c.background,
      },
      '.cm-content': {
        caretColor: c.cursor,
      },
      '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection': {
        backgroundColor: c.selection,
      },
      '&.cm-focused': {
        outline: 'none',
      },
      '.cm-gutters': {
        backgroundColor: c.gutterBackground,
        color: c.foreground,
        border: 'none',
        borderRight: `1px solid ${c.gutterBorder}`,
      },
      '.cm-activeLine': {
        backgroundColor: c.activeLine,
      },
      '.cm-activeLineGutter': {
        backgroundColor: c.activeLine,
      },
      '.cm-selectionMatch': {
        backgroundColor: c.selectionDim,
      },
    },
    { dark: c.mode === 'dark' },
  );
  if (c.mode === 'dark') return [chrome, cmSyntaxHighlightFor(c.mode)!];
  return chrome;
}

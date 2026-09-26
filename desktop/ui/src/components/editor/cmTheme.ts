/**
 * P1-34 — CodeMirror 6 theme mapping for the editor.
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
 * Deliberately zero-dependency: @codemirror/language (HighlightStyle) is not
 * a direct dependency, so syntax-token recoloring is out of reach here. In
 * dark mode the basicSetup default highlight style (light-tuned, unreadable
 * on dark surfaces) is disabled instead — code renders uniformly in the
 * theme's foreground. Follow-up (needs a new dep, e.g.
 * @uiw/codemirror-themes): a dark token palette.
 */
import { EditorView } from '@codemirror/view';
import type { Extension } from '@codemirror/state';
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
 * ours.
 */
export function cmThemeFor(
  themeId: string,
  modeOf?: (theme: string) => 'light' | 'dark' | undefined,
): Extension {
  const c = cmThemeColorsFor(themeId, modeOf);
  return EditorView.theme(
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
}

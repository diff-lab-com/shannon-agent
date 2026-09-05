/**
 * P1-5 D — xterm.js theme mapping for the integrated terminal.
 *
 * The app's themes are generated from the single source
 * `scripts/theme-source.json` → `src/theme/generated/themes.css`
 * (one `[data-theme]` block per theme) + `THEME_REGISTRY`, which records
 * each theme's light/dark scheme (see `ThemeContext.themeModeOf`).
 *
 * Mapping strategy: read the *live* CSS custom properties the active theme
 * block defines (surface / on-surface / primary / secondary-container /
 * outline) so the terminal follows every registered theme for free, and
 * keep a dark/light base palette (ANSI-16) keyed by `themeModeOf()` from
 * the generated registry as the floor — used when computed styles are
 * unavailable (jsdom, SSR) and for the ANSI colors the tokens don't cover.
 */
import type { ITheme } from '@xterm/xterm';
import { themeModeOf } from '@/context/ThemeContext';

/** A concrete theme id (ThemeContext keeps `ResolvedTheme` module-local). */
type ResolvedTheme = Parameters<typeof themeModeOf>[0];

function readVar(name: string): string | null {
  if (typeof window === 'undefined' || typeof document === 'undefined') return null;
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return value.length > 0 ? value : null;
}

/** Light-scheme base + ANSI palette (AA on light surfaces). */
const ANSI_LIGHT: Pick<
  ITheme,
  | 'background' | 'foreground'
  | 'black' | 'red' | 'green' | 'yellow' | 'blue' | 'magenta' | 'cyan' | 'white' | 'brightBlack' | 'brightWhite'
> = {
  background: '#f7f9fb',
  foreground: '#191c1e',
  black: '#3b4045',
  red: '#ba1a1a',
  green: '#1b6e3d',
  yellow: '#855000',
  blue: '#0051d5',
  magenta: '#8a3fb8',
  cyan: '#00696d',
  white: '#f2f4f6',
  brightBlack: '#5f5b68',
  brightWhite: '#ffffff',
};

/** Dark-scheme base + ANSI palette (AA on dark surfaces). */
const ANSI_DARK: typeof ANSI_LIGHT = {
  background: '#1a1b26',
  foreground: '#a9b1d6',
  black: '#1a1b26',
  red: '#f7768e',
  green: '#73daca',
  yellow: '#e0af68',
  blue: '#7aa2f7',
  magenta: '#bb9af7',
  cyan: '#7dcfff',
  white: '#c0caf5',
  brightBlack: '#565f89',
  brightWhite: '#ffffff',
};

/**
 * Build the xterm theme for the resolved app theme (pure — jsdom/SSR safe;
 * the ANSI floor comes from the registry's light/dark mode, token-backed
 * colors are layered on top when the live stylesheet is readable).
 *
 * `themeId` is the `<html data-theme>` value — a THEME_REGISTRY id in
 * practice, but unknown ids degrade to the light floor instead of lying
 * about the union type.
 */
export function xtermThemeFor(
  themeId: string,
  modeOf: (theme: string) => 'light' | 'dark' | undefined = id =>
    themeModeOf(id as ResolvedTheme),
): ITheme {
  const mode = modeOf(themeId) ?? 'light';
  const theme: ITheme = mode === 'dark' ? { ...ANSI_DARK } : { ...ANSI_LIGHT };
  const background = readVar('--color-surface') ?? readVar('--background');
  const foreground = readVar('--color-on-surface') ?? readVar('--foreground');
  if (background) theme.background = background;
  if (foreground) theme.foreground = foreground;
  const cursor = readVar('--color-primary');
  if (cursor) {
    theme.cursor = cursor;
    theme.cursorAccent = readVar('--color-on-primary') ?? (mode === 'dark' ? '#1a1b26' : '#ffffff');
  }
  const selection = readVar('--color-secondary-container');
  if (selection) theme.selectionBackground = selection;
  const border = readVar('--color-outline-variant');
  if (border) theme.selectionInactiveBackground = border;
  return theme;
}

/** Convenience wrapper with the default mode lookup bound. */
export function xtermTheme(themeId: string): ITheme {
  return xtermThemeFor(themeId);
}

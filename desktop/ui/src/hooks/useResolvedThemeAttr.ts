import { useEffect, useState } from 'react';

/**
 * Resolved theme id read straight off `<html data-theme>` (what
 * ThemeProvider mirrors). Deliberately NOT `useTheme()`: consumers must be
 * mountable anywhere — including test trees that render without a
 * ThemeProvider — and the attribute is observable.
 *
 * Shared by the theme-following surfaces that build their own palettes from
 * the resolved id (TerminalPanel's xtermTheme, CodeEditor's cmTheme).
 */
export function useResolvedThemeAttr(): string {
  const read = () =>
    typeof document === 'undefined' ? 'material' : document.documentElement.getAttribute('data-theme') ?? 'material';
  const [theme, setTheme] = useState(read);
  useEffect(() => {
    if (typeof MutationObserver === 'undefined') return;
    const observer = new MutationObserver(() => setTheme(read()));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] });
    return () => observer.disconnect();
  }, []);
  return theme;
}

/**
 * Synchronous one-shot read of `<html data-theme>` — for imperative code
 * paths (e.g. creating an xterm instance inside a callback) that must see
 * the CURRENT theme instead of a value captured by a stale closure.
 */
export function readResolvedThemeAttr(): string {
  return typeof document === 'undefined'
    ? 'material'
    : document.documentElement.getAttribute('data-theme') ?? 'material';
}

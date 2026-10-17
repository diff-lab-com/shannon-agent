/**
 * Monospace font stacks — single source of truth (AI-chat-ux T1, 2026-10).
 *
 * The terminal's old TS-local stack led with the STATIC name
 * `'JetBrains Mono'`, but the self-hosted webfont (`@fontsource-variable/
 * jetbrains-mono`, imported in main.tsx) registers as
 * `'JetBrains Mono Variable'` — the first candidate never matched, and on
 * machines without dev fonts installed the terminal silently rendered the
 * platform's generic monospace fallback. The stack below leads with the
 * registered variable-font name so the bundled JetBrains Mono actually
 * renders everywhere, then falls back through the platform mono fonts.
 *
 * Why a TS constant instead of `var(--font-mono)`: xterm.js measures glyph
 * metrics through canvas `ctx.font`, which requires a CONCRETE font-family
 * list — `var()` never resolves in that context. `--font-mono` (index.css)
 * stays the CSS-side twin for DOM text (`font-mono` utility); keep the two
 * lists in sync when changing either.
 */
export const MONO_FONT_STACK =
  "'JetBrains Mono Variable', 'JetBrains Mono', ui-monospace, 'Cascadia Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace"

# Shannon desktop UI styles

This directory is the canonical reference for Shannon desktop's design
tokens — the named values that every component, animation, and layout
should consume instead of hardcoding hex codes, pixel sizes, or timings.

## What's where

| File                  | Purpose                                            |
| --------------------- | -------------------------------------------------- |
| `tokens.css`          | Documented inventory of every token name + intent  |
| `../index.css`        | Actual @theme {} declarations + dark theme overrides |
| `../tailwind.config`  | N/A — Tailwind v4 uses `@theme {}` in CSS, not JS |

> Tailwind v4 reads token declarations from CSS via `@theme {}`, not from
> a JS config file. Adding a token means adding a line to a `@theme {}`
> block in `index.css` and documenting it here.

## Token groups

| Group        | Prefix              | Example                                | Notes                                        |
| ------------ | ------------------- | -------------------------------------- | -------------------------------------------- |
| Color        | `--color-*`         | `--color-primary`, `--color-on-surface | Material 3 roles; auto-switched by theme     |
| Spacing      | `--spacing-*`       | `--spacing-sm` (8px)                   | 4-pixel scale; use over `p-{n}` in components |
| Type         | `--font-*`          | `--font-label-md`                      | Variable: Inter Variable (labels + body); monospace via font-mono opt-in     |
| Type size    | `--text-{role}-{n}` | `--text-body-md` (16px)                | Tailwind compiles to `text-body-md` utility |
| Radius       | `--radius-*`        | `--radius-2xl` (18px)                  | Message bubbles, modals                       |
| Shadow       | `--shadow-e{1..5}`  | `--shadow-e1`                          | Elevation levels (1..5)                       |
| Animation    | `--duration-*`      | `--duration-normal` (160ms)            | Hover 100ms / default 160ms / panel 240ms     |
| Z-index      | `--z-*`             | `--z-modal` (50)                       | Reserved set; most code should not need this |

## How to add a new token

1. Decide which group it belongs to.
2. Add the line to the matching block in `src/index.css` under `@theme {}`
   (or, for color overrides, inside a `[data-theme="..."] { ... }` block).
3. Document it in `tokens.css` next to its siblings — one line that
   captures intent (and warn about misuses).
4. Use it via the generated Tailwind utility (`bg-primary`,
   `text-on-surface-variant`) or as `var(--color-primary)` in inline
   styles.

## Dark mode

Dark mode is not a single theme — it's a swap-out surface that the
`<ThemeContext>` triggers by setting `data-theme` on the root element.
The eight curated themes (`tokyo-night`, `catppuccin`, `nord`, `ember`,
`slate`, `solarized`, `dracula`, `gruvbox`) all override the same set
of color tokens. Component code should never branch on the theme —
reach for the role-named token instead (`text-on-surface` always means
"the text color on a regular surface", regardless of theme).

## Accessibility constraints

All token color combinations are checked at AA contrast in light + at
least one dark theme (the `tokyo-night` baseline). When you add a new
token pair (e.g. a new "info" color), re-verify contrast — `axe-core`
(`@axe-core/cli` or DevTools "Accessibility" panel) catches failures
automatically.

## Reduced motion

`tokens.css` ends with a `prefers-reduced-motion` rule that nulls out
non-essential transitions. Don't add motion that bypasses this — the
user's OS preference always wins.

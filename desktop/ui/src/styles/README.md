# Shannon desktop UI styles

This directory documents Shannon desktop's design tokens — the named
values that every component, animation, and layout should consume instead
of hardcoding hex codes, pixel sizes, or timings.

## What's where

| File                  | Purpose                                                       |
| --------------------- | ------------------------------------------------------------- |
| `../index.css`        | Actual `@theme {}` declarations (shared scales) + glass/elevation utilities |
| `../theme/generated/themes.css` | GENERATED per-theme color token blocks — do not edit by hand |
| `tokens.css`          | Behavior rules only (reduced-motion, shared keyframes, compact density) + a header comment pointing at the value sources |
| `../tailwind.config`  | N/A — Tailwind v4 uses `@theme {}` in CSS, not JS             |

> Token values have a single source each (UI review 2026-09-29, T1/T3/T5):
>
> - Shared scales — spacing, type, radius, shadow/elevation, duration,
>   easing, z-index — live in the `@theme {}` blocks of `src/index.css`.
> - Theme color tokens (and each theme's `--radius`) are GENERATED from
>   `scripts/theme-source.json` via `pnpm generate:themes` into
>   `src/theme/generated/themes.css` (+ `registry.ts`, and the base
>   palette inside `index.css`'s `GENERATED:THEME_BASE` region).
>   `tokens.css` deliberately holds NO token values — it used to mirror
>   the scales under an unlayered `:root`, which made it a second source
>   of truth that silently out-prioritized `index.css` and drifted.

## Token groups

| Group        | Prefix              | Example                                | Notes                                        |
| ------------ | ------------------- | -------------------------------------- | -------------------------------------------- |
| Color        | `--color-*`         | `--color-primary`, `--color-on-surface` | Material 3 roles; generated per theme, auto-switched |
| Chart series | `--chart-series-1..8` | `--chart-series-3`                   | Data-viz palette, declared per theme in `theme-source.json` |
| Spacing      | `--spacing-*`       | `--spacing-sm` (8px)                   | 4-pixel scale; use over `p-{n}` in components |
| Type         | `--font-*`          | `--font-label-md`                      | Variable: Inter Variable (labels + body); monospace via font-mono opt-in     |
| Type size    | `--text-{role}-{n}` | `--text-body-md` (16px)                | Tailwind compiles to `text-body-md` utility  |
| Radius       | `--radius-*`        | `--radius-2xl` (18px)                  | Derived from each theme's `--radius` base unit |
| Shadow       | `--shadow-e{1..5}`  | `--shadow-e1`                          | Elevation levels (1..5); the `shadow-e{n}` utilities bind to these tokens |
| Animation    | `--duration-*`      | `--duration-normal` (160ms)            | fast 100ms / normal 160ms / slow 240ms / slower 400ms — declared once in `@theme`, no overrides |
| Easing       | `--ease-glass`      | —                                      | Apple sheet curve used by glass/panel motion |
| Z-index      | `--z-index-*`       | `--z-index-modal` (50)                 | Reserved scale (`z-modal` etc. utilities); most code should not need this |

## How to add a new token

1. Decide which group it belongs to.
2. Add the declaration to the matching block:
   - shared scale → the `@theme {}` block in `src/index.css`;
   - theme color / chart series → `scripts/theme-source.json` (base or
     per-theme `vars`), then run `pnpm generate:themes`.
3. Capture intent in a short comment next to the declaration.
4. Use it via the generated Tailwind utility (`bg-primary`,
   `text-on-surface-variant`) or as `var(--color-primary)` in inline
   styles.

## Themes

Theme color tokens are generated — there is no hand-maintained CSS
selector list. `scripts/theme-source.json` is the only place color values
are authored; `src/context/ThemeContext.tsx` maps each theme id to its
light/dark scheme via the generated `THEME_REGISTRY` and mirrors it onto
`<html data-theme>` + `<html data-theme-mode>`. The 12 themes (6 dark,
6 light):

- dark: `tokyo-night` (default), `catppuccin`, `nord`, `solarized`,
  `dracula`, `gruvbox`
- light: `material`, `tokyo-night-light`, `ember`, `slate`,
  `solarized-light`, `gruvbox-light`

Component code should never branch on the theme — reach for the role-named
token instead (`text-on-surface` always means "the text color on a regular
surface", regardless of theme). Chart colors follow the same rule via
`var(--chart-series-n)`, which every theme overrides.

## Accessibility constraints

All token color combinations are checked at AA contrast by
`scripts/generate-themes.mjs` (pair contract in `scripts/lib/contrast.mjs`)
across every theme — generation fails loudly on a failing pair. Per-theme
`--chart-series-*` values are additionally kept at ≥3:1 against their
theme background by convention. When you add a new token pair, the
generator gate catches failures automatically; `axe-core` e2e is the
second net.

## Reduced motion

`tokens.css` carries a global `prefers-reduced-motion` rule that nulls out
non-essential transitions (plus targeted fallbacks like `.search-flash`).
Don't add motion that bypasses this — the user's OS preference always wins.

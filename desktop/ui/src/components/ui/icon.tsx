/**
 * Icon policy wrapper around Material Symbols (outlined).
 *
 * This component is the single entry point for icons in the desktop UI.
 * The font (`@fontsource-variable/material-symbols-outlined`) is loaded once
 * at `src/index.css:4` via the `@fontsource-variable/material-symbols-outlined`
 * import.
 *
 * ## Usage
 *
 * ```tsx
 * <Icon name="add" />                          // default 16px (icon-sm)
 * <Icon name="close" size="md" />              // 20px
 * <Icon name="chat" size="lg" className="text-primary" />
 * ```
 *
 * Icons are decorative by default (`aria-hidden="true"`). Pass
 * `aria-label="..."` (or any other aria-* prop) to mark them informative —
 * the wrapper will drop `aria-hidden` when other aria-* props are set.
 *
 * ## Size mapping
 *
 * Matches the icon-* utility classes in `src/index.css:153-161`:
 * `xs | sm | md | lg | xl | 2xl` → 12 / 16 / 20 / 24 / 32 / 48 px.
 *
 * ## Lucide → Material name mapping
 *
 * Use this table when migrating shadcn-generated code or hand-written
 * components that still import from the `lucide` package:
 *
 * | Lucide name         | Material Symbols      | notes                |
 * |---------------------|-----------------------|----------------------|
 * | `X`                 | `close`               |                      |
 * | `Plus`              | `add`                 |                      |
 * | `Check`             | `check`               |                      |
 * | `ChevronDown`       | `expand_more`         |                      |
 * | `ChevronUp`         | `expand_less`         |                      |
 * | `ChevronLeft`       | `chevron_left`        |                      |
 * | `ChevronRight`      | `chevron_right`       |                      |
 * | `MessageSquare`     | `chat`                |                      |
 * | `Search`            | `search`              |                      |
 * | `Settings`          | `settings`            |                      |
 * | `Trash`             | `delete`              |                      |
 * | `AlertTriangle`     | `warning`             |                      |
 * | `Info`              | `info`                |                      |
 * | `Menu`              | `menu`                |                      |
 * | `Copy`              | `content_copy`        |                      |
 * | `Edit`              | `edit`                |                      |
 * | `Eye` / `EyeOff`    | `visibility` / `visibility_off` |           |
 * | `Loader2`           | `progress_activity`   | (with `animate-spin`) |
 *
 * Full catalog: https://fonts.google.com/icons (outlined variant).
 */

import { cn } from '@/lib/utils'

const SIZE_TO_CLASS = {
  xs: 'icon-xs',
  sm: 'icon-sm',
  md: 'icon-md',
  lg: 'icon-lg',
  xl: 'icon-xl',
  '2xl': 'icon-2xl',
} as const

export type IconSize = keyof typeof SIZE_TO_CLASS

export interface IconProps extends React.HTMLAttributes<HTMLSpanElement> {
  /** Material Symbols icon name (e.g. `close`, `add`, `expand_more`). */
  name: string
  /** Size token matching the icon-* utility scale. Defaults to `sm` (16px). */
  size?: IconSize
}

/**
 * Render a Material Symbols icon. Use this instead of importing from
 * the `lucide` package or hand-writing `<span class="material-symbols-outlined">`.
 *
 * The wrapper is intentionally minimal — it forwards className, onClick, and
 * any other span props so call sites can compose styling freely.
 */
export function Icon({
  name,
  size = 'sm',
  className,
  'aria-hidden': ariaHidden,
  'aria-label': ariaLabel,
  ...rest
}: IconProps) {
  // Default to decorative (aria-hidden). If a label is provided we drop
  // aria-hidden so the icon is announced by screen readers.
  const hidden = ariaLabel || ariaHidden === false ? undefined : true

  return (
    <span
      className={cn(
        'material-symbols-outlined',
        SIZE_TO_CLASS[size],
        className,
      )}
      aria-hidden={hidden}
      aria-label={ariaLabel}
      {...rest}
    >
      {name}
    </span>
  )
}
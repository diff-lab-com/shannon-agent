# i18n Migration Guide

This document describes how to migrate React components from hardcoded English strings to `react-intl` message IDs. The Shannon i18n layer is described in `./index.tsx`.

## Why migrate?

Phase 1 (`commit <pending>`) shipped the infrastructure plus `Welcome.tsx` as a reference migration. Roughly 120 components still render hardcoded English strings and need to migrate incrementally in follow-up PRs.

The goal: every user-visible string flows through `intl.formatMessage()` so that the `Locale` switcher in **Settings → General** can re-render the entire app in Chinese (`zh-CN`) or English (`en`) without code changes.

## Prerequisites

- `I18nProvider` is mounted at the root (`App.tsx`), so `useIntl()` works in any descendant.
- `useI18n()` (returning `{ locale, setLocale }`) is only needed in components that change the locale, e.g., settings pages.

## Step-by-step: migrating one component

### 1. Find user-visible strings

Open the component and look for JSX text, button labels, `aria-label`s, `placeholder=`s, `title=`s, toast messages, and error messages. **Leave untouched:** identifiers, CSS classes, log messages, and `console.warn` text.

### 2. Pick message IDs

Use the `{feature}.{subsection}.{key}` convention:

```ts
// bad
'welcome.heading'           // too vague
'welcomeTaskTitleLabel'     // wrong casing scheme

// good
'welcome.task.title'        // feature.subsection.key
'welcome.task.code.label'   // leaf keys can be nested under a subsection
```

### 3. Add the strings to both locale catalogs

Edit `./locales/en.json` and `./locales/zh-CN.json` in the same change. Keep keys sorted alphabetically within each feature block.

```json
// en.json
"welcome.task.title": "What will you use Shannon for?",

// zh-CN.json
"welcome.task.title": "你打算用 Shannon 做什么？",
```

### 4. Import the hook and replace strings

```tsx
// before
export function MyComponent() {
  return <h1>Hello, world</h1>
}

// after
import { useIntl } from 'react-intl'

export function MyComponent() {
  const intl = useIntl()
  return <h1>{intl.formatMessage({ id: 'myComponent.greeting' })}</h1>
}
```

### 5. Handle dynamic values

Pass values via the second argument. The placeholder name in the message matches the object key.

```tsx
// component
intl.formatMessage(
  { id: 'welcome.model.subtitle.recommended' },
  { task: 'Code', provider: 'Anthropic' },
)

// en.json
"welcome.model.subtitle.recommended": "For {task}, we recommend {provider}."

// zh-CN.json (positions can move — ICU handles reordering)
"welcome.model.subtitle.recommended": "针对 {task}，我们推荐 {provider}。"
```

### 6. Handle plurals

```tsx
intl.formatMessage({ id: 'welcome.done.setup.tools' }, { count: 3 })

// en.json — ICU plural syntax
"welcome.done.setup.tools": "{count, plural, =1 {{count} tool} other {{count} tools}} enabled",

// zh-CN.json — Chinese doesn't distinguish singular/plural
"welcome.done.setup.tools": "已启用 {count} 个工具",
```

### 7. Handle rich-text (inline elements)

When a string needs an inline `<button>` or `<kbd>`, use the chunk-callback form. **Only reach for this when a plain string genuinely won't do** — it adds complexity.

```tsx
intl.formatMessage(
  { id: 'welcome.tools.workingDir.help' },
  {
    link: (chunks: React.ReactNode) => (
      <button onClick={...}>{chunks}</button>
    ),
  },
)

// en.json
"welcome.tools.workingDir.help": "Need a different working directory? Adjust it in {link}."
```

The tag name (`link` here) becomes the placeholder in the catalog, and `chunks` is the inner text rendered inside the element.

### 8. Update tests

Tests rendering the migrated component need `<I18nProvider>` in the ancestor tree. Wrap the render:

```tsx
import { I18nProvider } from '@/i18n'

function wrap() {
  return render(
    <I18nProvider>
      <MemoryRouter>
        <MyComponent />
      </MemoryRouter>
    </I18nProvider>
  )
}
```

The default locale in tests is `en` (via `detectDefault()` falling back when `localStorage` and `navigator.language` are unset), so existing English assertions continue to work.

### 9. Run tests

```bash
pnpm vitest run src/__tests__/MyComponent.test.tsx
```

## Common patterns (from `Welcome.tsx`)

**Lookup tables with message IDs.** Convert `label: 'Code'` to `labelKey: 'welcome.task.code.label'`. Keep the lookup table as a module-level constant — only the `intl.formatMessage` call belongs in the component body.

```tsx
const TASKS = [
  { id: 'code', labelKey: 'welcome.task.code.label', ... },
  // ...
]

// in JSX
{TASKS.map(t => (
  <button>{intl.formatMessage({ id: t.labelKey })}</button>
))}
```

**Accessible names.** Interactive elements need an `aria-label` even when their visible text comes from a different message:

```tsx
<input
  type="checkbox"
  aria-label={intl.formatMessage({ id: 'welcome.tools.enableAria' }, { label: toolLabel })}
/>
```

Don't reuse the visible label as the aria-label — screen-reader users benefit from the action-verb form ("Enable Filesystem") rather than the bare noun ("Filesystem").

**Short stepper/progress labels.** When a heading text is reused as a short step indicator, define a separate `welcome.step.*` family with one-word labels:

```tsx
const STEP_LABEL_KEYS = ['welcome.step.task', 'welcome.step.model', ...]
```

This avoids duplicate-text collisions in tests (`getByText` matches both the heading and the stepper label).

## What NOT to migrate

- **Log messages and warnings** (`console.warn('provider setup failed')`). These are developer-facing.
- **TypeScript identifiers and string-literal union members** (`type TaskId = 'code' | 'writing'`).
- **CSS class names and Material Symbols icon names** (`'folder_open'`, `'auto_awesome'`).
- **Proper nouns that are the same across locales** ("Anthropic", "OpenAI", "Ollama"). Brand names stay untranslated.

## Verifying the migration

1. `pnpm tsc --noEmit` — type-checks.
2. `pnpm vitest run src/__tests__/Welcome.test.tsx` — reference suite.
3. Manually toggle **Settings → General → Language** and confirm the migrated component re-renders.

## Scope of Phase 1

- `ui/src/i18n/index.tsx`, `locales/en.json`, `locales/zh-CN.json`
- `App.tsx` wrapped with `<I18nProvider>`
- `Welcome.tsx` migrated as the reference
- `GeneralSettings.tsx` has the language switcher
- This document

Everything else migrates incrementally. Don't bundle more than ~5 components per PR — reviewers need to verify each locale catalog entry against the live UI.

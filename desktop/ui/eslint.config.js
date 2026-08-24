// ESLint 9 flat config for desktop/ui.
//
// Stack: typescript-eslint (recommended) + react-hooks (recommended).
// We intentionally do NOT pull in a formatter (prettier) — style stays
// out of the lint path to avoid bikeshedding. tsc handles type errors.
//
// Migration plan: T0.4 of docs/plans/desktop-ui-modernization.md. The
// pnpm lint script also chains `eslint src` after `tsc --noEmit`, so the
// existing local-check.sh pre-push gate (desktop/scripts/local-check.sh:30)
// picks up the new behavior automatically — no hook changes needed.

import js from '@eslint/js'
import tseslint from 'typescript-eslint'
import reactHooks from 'eslint-plugin-react-hooks'
import globals from 'globals'

export default [
  {
    ignores: [
      'dist/**',
      'coverage/**',
      'node_modules/**',
      'playwright-report/**',
      'test-results/**',
      'e2e/**',          // Playwright specs live here; vitest setup files don't apply
      '*.config.{js,ts}', // config files (vite, vitest, tailwind, postcss, etc.)
      'src/vite-env.d.ts',
      'src/__tests__/setup.ts', // mocks Tauri APIs intentionally
    ],
  },

  // Project-wide baseline
  js.configs.recommended,

  // TypeScript files: typescript-eslint recommended (no stylistic rules)
  ...tseslint.configs.recommended,

  // React hooks: catch the obvious bugs (missing deps, conditional hooks)
  reactHooks.configs['recommended-latest'],

  // Apply to source TypeScript
  {
    files: ['src/**/*.{ts,tsx}'],
    languageOptions: {
      ecmaVersion: 2024,
      sourceType: 'module',
      globals: {
        ...globals.browser,
        // Tauri APIs surface as globals through @tauri-apps/api.
        ...Object.fromEntries(
          ['invoke', 'convertFileSrc', 'listen', 'emit'].map((k) => [k, 'readonly']),
        ),
      },
      parserOptions: {
        ecmaFeatures: { jsx: true },
      },
    },
    rules: {
      // React 19: refresh rules are largely obsolete. Keep `recommended`
      // for `react-hooks/rules-of-hooks` and `react-hooks/exhaustive-deps`.
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',

      // typescript-eslint recommended but relaxed for incremental adoption.
      // Tighten as the codebase clears out.
      '@typescript-eslint/no-unused-vars': [
        'warn',
        { argsIgnorePattern: '^_', varsIgnorePattern: '^_', caughtErrorsIgnorePattern: '^_' },
      ],
      '@typescript-eslint/no-explicit-any': 'warn',
      '@typescript-eslint/consistent-type-imports': [
        'warn',
        { prefer: 'type-imports', fixStyle: 'inline-type-imports' },
      ],

      // Relax a few base rules that fight React/Vite idioms.
      'no-empty': ['warn', { allowEmptyCatch: true }],
    },
  },

  // Test files get a slightly looser pass — RTL helpers throw a lot of
  // `any`, mock-heavy setup is unavoidable.
  {
    files: ['src/**/__tests__/**/*.{ts,tsx}', 'src/**/*.test.{ts,tsx}', 'src/**/*.spec.{ts,tsx}'],
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
    },
  },
]
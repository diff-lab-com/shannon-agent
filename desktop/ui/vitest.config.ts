import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/__tests__/setup.ts'],
    poolOptions: {
      threads: {
        maxThreads: 1,
        minThreads: 1,
      },
      forks: {
        maxForks: 1,
        minForks: 1,
      },
    },
    maxConcurrency: 1,
    coverage: {
    // Base UI 1.8's Tooltip drives open/close with dense internal timers;
    // under V8 coverage instrumentation those callbacks are amplified ~1000x
    // (6 tests took 15 minutes). The shim is 12 lines of composition — its
    // behaviour is covered by Tooltip.test + e2e, not by coverage percentages.
    exclude: ['src/components/ui/tooltip.prim.tsx'],
      provider: 'v8',
      reporter: ['text', 'json', 'html', 'lcov'],
      include: ['src/**/*.tsx', 'src/**/*.ts'],
      exclude: [
        'node_modules/',
        'src/__tests__/',
        '**/*.test.{ts,tsx}',
        '**/*.spec.{ts,tsx}',
        'src/main.tsx',
        'src/vite-env.d.ts',
        'src/types/index.ts',
        'src/lib/tauri-api.ts',
        'src/App.tsx',
        // Demo-mode mock layer (VITE_MOCK_MODE=1): only reachable via the
        // main.tsx alias swap, never in production builds — same rationale
        // as main.tsx/App.tsx above. 1864 lines of mock data/handlers were
        // dragging real coverage ~6pp below the threshold.
        'src/lib/mock/',
        'src/hooks/useTheme.ts',
        'src/components/ui/select.tsx',
      ],
      thresholds: {
        lines: 80,
        functions: 60,
        branches: 75,
        statements: 80
      }
    },
    include: ['src/**/*.{test,spec}.{ts,tsx}'],
    root: path.resolve(__dirname)
  },
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src')
    }
  }
})
